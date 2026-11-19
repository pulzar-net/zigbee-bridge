//! The ASH link layer as a sans-IO state machine.
//!
//! [`Connection`] does no I/O and reads no clock: feed it received frames
//! with [`Connection::handle_frame`], timer expiries with
//! [`Connection::handle_timeout`], and payloads with [`Connection::send`];
//! take bytes to write from [`Connection::poll_transmit`] and events from
//! [`Connection::poll_event`]. The same machine serves both ends of the
//! link ([`Role`]), which is how the repository's NCP simulator speaks ASH.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::frame::{reset_code, Frame, FrameError, ASH_VERSION, MAX_DATA_LEN};

/// Which end of the link this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The host: sends RST, waits for RSTACK.
    Host,
    /// The network co-processor: answers RST with RSTACK.
    Ncp,
}

/// Link timing and window settings. The defaults follow UG101.
#[derive(Debug, Clone)]
pub struct Config {
    /// DATA frames in flight before an ACK, 1 to 7.
    pub tx_window: u8,
    /// Initial ACK timeout.
    pub ack_time_init: Duration,
    /// Lower bound of the adaptive ACK timeout.
    pub ack_time_min: Duration,
    /// Upper bound of the adaptive ACK timeout.
    pub ack_time_max: Duration,
    /// Consecutive ACK timeouts before the link is declared down.
    pub max_timeouts: u32,
    /// How long the host waits for RSTACK before sending RST again.
    pub rst_timeout: Duration,
    /// RST attempts before giving up.
    pub rst_attempts: u32,
    /// How long to hold DATA after the peer said it is not ready.
    pub not_ready_time: Duration,
    /// Payloads queued but not yet sent, beyond which [`Connection::send`] refuses.
    pub max_queue: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            tx_window: 3,
            ack_time_init: Duration::from_millis(800),
            ack_time_min: Duration::from_millis(400),
            ack_time_max: Duration::from_millis(2400),
            max_timeouts: 4,
            rst_timeout: Duration::from_millis(3200),
            rst_attempts: 3,
            not_ready_time: Duration::from_millis(1000),
            max_queue: 64,
        }
    }
}

/// Why the link went down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownReason {
    /// The NCP reset while connected (RSTACK out of the blue).
    NcpReset {
        /// The NCP's reset code.
        code: u8,
    },
    /// The NCP sent ERROR.
    NcpError {
        /// The NCP's error code.
        code: u8,
    },
    /// Too many DATA frames went unacknowledged.
    AckTimeouts,
    /// No RSTACK after every RST attempt.
    ResetTimeout,
    /// The peer speaks another ASH version.
    BadVersion(u8),
}

/// Something the link wants the layer above to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The link is up. On the host, `reset_code` is the NCP's reset reason.
    Connected {
        /// The reset code from RSTACK (host) or [`reset_code::SOFTWARE`] (NCP).
        reset_code: u8,
    },
    /// An EZSP frame arrived, in order and exactly once.
    Data(Vec<u8>),
    /// The link is down; queued and unacknowledged payloads were dropped.
    Down(DownReason),
}

/// [`Connection::send`] refused a payload.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    /// The link is not connected.
    #[error("ASH link is not connected")]
    NotConnected,
    /// The queue is full.
    #[error("ASH transmit queue is full")]
    QueueFull,
    /// The payload is not 3 to 220 bytes.
    #[error("payload of {0} bytes is outside 3..=220")]
    BadLength(usize),
}

/// Link counters, for diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counters {
    /// DATA frames sent, retransmissions included.
    pub tx_data: u64,
    /// DATA frames retransmitted.
    pub tx_retransmits: u64,
    /// DATA frames accepted.
    pub rx_data: u64,
    /// Frames dropped as malformed (CRC, length, UART error).
    pub rx_bad: u64,
    /// DATA frames out of sequence.
    pub rx_out_of_sequence: u64,
    /// ACK timer expiries.
    pub ack_timeouts: u64,
    /// NAKs sent.
    pub tx_naks: u64,
}

#[derive(Debug)]
enum State {
    Down,
    Resetting { deadline: Instant, attempts: u32 },
    Connected,
}

#[derive(Debug)]
struct Sent {
    num: u8,
    payload: Vec<u8>,
    sent_at: Instant,
    retransmitted: bool,
}

/// One end of an ASH link.
#[derive(Debug)]
pub struct Connection {
    role: Role,
    config: Config,
    state: State,
    /// Next frame number to give a new DATA frame.
    frm_tx: u8,
    /// Oldest frame number not yet acknowledged by the peer.
    ack_rx: u8,
    /// Next frame number expected from the peer.
    frm_rx: u8,
    queue: VecDeque<Vec<u8>>,
    unacked: VecDeque<Sent>,
    /// Index into `unacked` of the next frame to retransmit.
    retransmit_from: Option<usize>,
    ack_deadline: Option<Instant>,
    ack_period: Duration,
    timeouts: u32,
    /// A NAK was sent and no in-sequence frame has arrived since.
    rejecting: bool,
    send_ack: bool,
    send_nak: bool,
    peer_not_ready_until: Option<Instant>,
    out: VecDeque<Vec<u8>>,
    events: VecDeque<Event>,
    counters: Counters,
}

fn next(n: u8) -> u8 {
    (n + 1) & 7
}

/// Distance from `from` forward to `to`, modulo 8.
fn distance(from: u8, to: u8) -> u8 {
    to.wrapping_sub(from) & 7
}

impl Connection {
    /// A link in the down state. A host starts it with [`Connection::reset`];
    /// an NCP waits for the host's RST.
    pub fn new(role: Role, mut config: Config) -> Self {
        config.tx_window = config.tx_window.clamp(1, 7);
        let ack_period = config.ack_time_init;
        Connection {
            role,
            config,
            state: State::Down,
            frm_tx: 0,
            ack_rx: 0,
            frm_rx: 0,
            queue: VecDeque::new(),
            unacked: VecDeque::new(),
            retransmit_from: None,
            ack_deadline: None,
            ack_period,
            timeouts: 0,
            rejecting: false,
            send_ack: false,
            send_nak: false,
            peer_not_ready_until: None,
            out: VecDeque::new(),
            events: VecDeque::new(),
            counters: Counters::default(),
        }
    }

    /// True once RST/RSTACK has completed.
    pub fn is_connected(&self) -> bool {
        matches!(self.state, State::Connected)
    }

    /// Link counters.
    pub fn counters(&self) -> &Counters {
        &self.counters
    }

    /// The current adaptive ACK timeout.
    pub fn ack_period(&self) -> Duration {
        self.ack_period
    }

    fn clear_link(&mut self) {
        self.frm_tx = 0;
        self.ack_rx = 0;
        self.frm_rx = 0;
        self.queue.clear();
        self.unacked.clear();
        self.retransmit_from = None;
        self.ack_deadline = None;
        self.ack_period = self.config.ack_time_init;
        self.timeouts = 0;
        self.rejecting = false;
        self.send_ack = false;
        self.send_nak = false;
        self.peer_not_ready_until = None;
        self.out.clear();
    }

    /// Host: drop everything and send RST. The link reports
    /// [`Event::Connected`] when the NCP answers.
    pub fn reset(&mut self, now: Instant) {
        self.clear_link();
        if self.role == Role::Host {
            self.state = State::Resetting {
                deadline: now + self.config.rst_timeout,
                attempts: 1,
            };
            self.out.push_back(Frame::Rst.encode());
        } else {
            self.state = State::Down;
        }
    }

    fn go_down(&mut self, reason: DownReason) {
        self.clear_link();
        self.state = State::Down;
        self.events.push_back(Event::Down(reason));
    }

    /// Queue an EZSP frame for sending.
    pub fn send(&mut self, payload: Vec<u8>, now: Instant) -> Result<(), SendError> {
        if !self.is_connected() {
            return Err(SendError::NotConnected);
        }
        if !(crate::frame::MIN_DATA_LEN..=MAX_DATA_LEN).contains(&payload.len()) {
            return Err(SendError::BadLength(payload.len()));
        }
        if self.queue.len() >= self.config.max_queue {
            return Err(SendError::QueueFull);
        }
        self.queue.push_back(payload);
        self.pump(now);
        Ok(())
    }

    /// A frame the decoder rejected (bad CRC, length, UART error).
    pub fn handle_bad_frame(&mut self, _error: FrameError, now: Instant) {
        self.counters.rx_bad += 1;
        if self.is_connected() && !self.rejecting {
            self.rejecting = true;
            self.send_nak = true;
            self.pump(now);
        }
    }

    /// A well-formed frame from the peer.
    pub fn handle_frame(&mut self, frame: Frame, now: Instant) {
        match (&self.state, self.role, &frame) {
            // NCP: RST (re)starts the link at any time.
            (_, Role::Ncp, Frame::Rst) => {
                self.clear_link();
                self.state = State::Connected;
                self.out.push_back(
                    Frame::RstAck {
                        version: ASH_VERSION,
                        code: reset_code::SOFTWARE,
                    }
                    .encode(),
                );
                self.events.push_back(Event::Connected {
                    reset_code: reset_code::SOFTWARE,
                });
                return;
            }
            (State::Resetting { .. }, Role::Host, Frame::RstAck { version, code }) => {
                if *version != ASH_VERSION {
                    self.go_down(DownReason::BadVersion(*version));
                    return;
                }
                self.clear_link();
                self.state = State::Connected;
                self.events
                    .push_back(Event::Connected { reset_code: *code });
                return;
            }
            (State::Connected, Role::Host, Frame::RstAck { code, .. }) => {
                self.go_down(DownReason::NcpReset { code: *code });
                return;
            }
            (State::Connected, Role::Host, Frame::Error { code, .. }) => {
                self.go_down(DownReason::NcpError { code: *code });
                return;
            }
            (State::Connected, _, Frame::Data { .. } | Frame::Ack { .. } | Frame::Nak { .. }) => {}
            // Anything else outside a connection is ignored, as UG101 says.
            _ => return,
        }

        let (ack_num, not_ready) = match &frame {
            Frame::Data { ack_num, .. } => (*ack_num, None),
            Frame::Ack { ack_num, not_ready } | Frame::Nak { ack_num, not_ready } => {
                (*ack_num, Some(*not_ready))
            }
            _ => return,
        };
        // An acknowledgement outside the window is a corrupted or stale
        // frame; drop it whole.
        if distance(self.ack_rx, ack_num) > distance(self.ack_rx, self.frm_tx) {
            self.counters.rx_bad += 1;
            return;
        }
        self.acknowledge(ack_num, now);
        if let Some(not_ready) = not_ready {
            self.peer_not_ready_until = not_ready.then(|| now + self.config.not_ready_time);
        }

        match frame {
            Frame::Nak { .. } => {
                if !self.unacked.is_empty() {
                    self.retransmit_from = Some(0);
                }
            }
            Frame::Data {
                frm_num,
                re_tx,
                payload,
                ..
            } => {
                if frm_num == self.frm_rx {
                    self.frm_rx = next(self.frm_rx);
                    self.rejecting = false;
                    self.send_ack = true;
                    self.counters.rx_data += 1;
                    self.events.push_back(Event::Data(payload));
                } else if re_tx {
                    // A retransmission of a frame already delivered.
                    self.send_ack = true;
                } else {
                    self.counters.rx_out_of_sequence += 1;
                    if !self.rejecting {
                        self.rejecting = true;
                        self.send_nak = true;
                    }
                }
            }
            _ => {}
        }
        self.pump(now);
    }

    /// The peer acknowledged every frame before `ack_num`.
    fn acknowledge(&mut self, ack_num: u8, now: Instant) {
        let mut acked = 0usize;
        while self.ack_rx != ack_num {
            if let Some(sent) = self.unacked.pop_front() {
                if !sent.retransmitted {
                    self.adapt_ack_period(now.saturating_duration_since(sent.sent_at));
                }
            }
            self.ack_rx = next(self.ack_rx);
            acked += 1;
        }
        if acked > 0 {
            self.timeouts = 0;
            self.retransmit_from = self.retransmit_from.map(|i| i.saturating_sub(acked));
            self.ack_deadline = if self.unacked.is_empty() {
                None
            } else {
                Some(now + self.ack_period)
            };
        }
    }

    /// UG101: T_RX_ACK = 7/8 T_RX_ACK + 1/2 measured, within bounds.
    fn adapt_ack_period(&mut self, measured: Duration) {
        let period = self.ack_period * 7 / 8 + measured / 2;
        self.ack_period = period.clamp(self.config.ack_time_min, self.config.ack_time_max);
    }

    /// The earliest time [`Connection::handle_timeout`] must be called.
    pub fn poll_timeout(&self) -> Option<Instant> {
        let reset = match self.state {
            State::Resetting { deadline, .. } => Some(deadline),
            _ => None,
        };
        [reset, self.ack_deadline, self.peer_not_ready_until]
            .into_iter()
            .flatten()
            .min()
    }

    /// Run the timers.
    pub fn handle_timeout(&mut self, now: Instant) {
        if let State::Resetting { deadline, attempts } = self.state {
            if now >= deadline {
                if attempts >= self.config.rst_attempts {
                    self.go_down(DownReason::ResetTimeout);
                } else {
                    self.state = State::Resetting {
                        deadline: now + self.config.rst_timeout,
                        attempts: attempts + 1,
                    };
                    self.out.push_back(Frame::Rst.encode());
                }
            }
            return;
        }
        if self.peer_not_ready_until.is_some_and(|t| now >= t) {
            self.peer_not_ready_until = None;
        }
        if let Some(deadline) = self.ack_deadline {
            if now >= deadline && !self.unacked.is_empty() {
                self.counters.ack_timeouts += 1;
                self.timeouts += 1;
                if self.timeouts > self.config.max_timeouts {
                    self.go_down(DownReason::AckTimeouts);
                    return;
                }
                self.ack_period = (self.ack_period * 2).min(self.config.ack_time_max);
                self.retransmit_from = Some(0);
                self.ack_deadline = Some(now + self.ack_period);
            }
        }
        self.pump(now);
    }

    /// Move whatever may be sent now into the output queue.
    fn pump(&mut self, now: Instant) {
        if !self.is_connected() {
            return;
        }
        // Retransmissions first, oldest first.
        while let Some(index) = self.retransmit_from {
            let ack_num = self.frm_rx;
            match self.unacked.get_mut(index) {
                Some(sent) => {
                    sent.retransmitted = true;
                    let frame = Frame::Data {
                        frm_num: sent.num,
                        re_tx: true,
                        ack_num,
                        payload: sent.payload.clone(),
                    };
                    self.out.push_back(frame.encode());
                    self.counters.tx_data += 1;
                    self.counters.tx_retransmits += 1;
                    self.send_ack = false;
                    self.retransmit_from = Some(index + 1);
                }
                None => self.retransmit_from = None,
            }
        }
        let peer_ready = self.peer_not_ready_until.is_none_or(|t| now >= t);
        while peer_ready && self.unacked.len() < usize::from(self.config.tx_window) {
            let Some(payload) = self.queue.pop_front() else {
                break;
            };
            let num = self.frm_tx;
            self.frm_tx = next(self.frm_tx);
            let frame = Frame::Data {
                frm_num: num,
                re_tx: false,
                ack_num: self.frm_rx,
                payload,
            };
            self.out.push_back(frame.encode());
            self.counters.tx_data += 1;
            self.send_ack = false;
            if let Frame::Data { payload, .. } = frame {
                self.unacked.push_back(Sent {
                    num,
                    payload,
                    sent_at: now,
                    retransmitted: false,
                });
            }
            if self.ack_deadline.is_none() {
                self.ack_deadline = Some(now + self.ack_period);
            }
        }
        if self.send_nak {
            self.send_nak = false;
            self.send_ack = false;
            self.counters.tx_naks += 1;
            self.out.push_back(
                Frame::Nak {
                    ack_num: self.frm_rx,
                    not_ready: false,
                }
                .encode(),
            );
        } else if self.send_ack {
            self.send_ack = false;
            self.out.push_back(
                Frame::Ack {
                    ack_num: self.frm_rx,
                    not_ready: false,
                }
                .encode(),
            );
        }
    }

    /// Bytes to write to the serial port, if any.
    pub fn poll_transmit(&mut self) -> Option<Vec<u8>> {
        self.out.pop_front()
    }

    /// The next event for the layer above, if any.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Decoder;

    /// Two connections joined by a link that can drop or corrupt frames,
    /// on a virtual clock.
    struct Pair {
        host: Connection,
        ncp: Connection,
        host_rx: Decoder,
        ncp_rx: Decoder,
        now: Instant,
        /// Return true to drop the n-th frame host to NCP.
        drop_to_ncp: Box<dyn FnMut(usize) -> bool>,
        drop_to_host: Box<dyn FnMut(usize) -> bool>,
        sent_to_ncp: usize,
        sent_to_host: usize,
        host_events: Vec<Event>,
        ncp_events: Vec<Event>,
    }

    impl Pair {
        fn new() -> Self {
            Pair {
                host: Connection::new(Role::Host, Config::default()),
                ncp: Connection::new(Role::Ncp, Config::default()),
                host_rx: Decoder::new(),
                ncp_rx: Decoder::new(),
                now: Instant::now(),
                drop_to_ncp: Box::new(|_| false),
                drop_to_host: Box::new(|_| false),
                sent_to_ncp: 0,
                sent_to_host: 0,
                host_events: vec![],
                ncp_events: vec![],
            }
        }

        /// Deliver everything in flight until both sides are quiet.
        fn settle(&mut self) {
            for _ in 0..1000 {
                let mut moved = false;
                while let Some(bytes) = self.host.poll_transmit() {
                    moved = true;
                    self.sent_to_ncp += 1;
                    if (self.drop_to_ncp)(self.sent_to_ncp) {
                        continue;
                    }
                    for b in bytes {
                        if let Some(r) = self.ncp_rx.push(b) {
                            match r {
                                Ok(f) => self.ncp.handle_frame(f, self.now),
                                Err(e) => self.ncp.handle_bad_frame(e, self.now),
                            }
                        }
                    }
                }
                while let Some(bytes) = self.ncp.poll_transmit() {
                    moved = true;
                    self.sent_to_host += 1;
                    if (self.drop_to_host)(self.sent_to_host) {
                        continue;
                    }
                    for b in bytes {
                        if let Some(r) = self.host_rx.push(b) {
                            match r {
                                Ok(f) => self.host.handle_frame(f, self.now),
                                Err(e) => self.host.handle_bad_frame(e, self.now),
                            }
                        }
                    }
                }
                while let Some(e) = self.host.poll_event() {
                    self.host_events.push(e);
                }
                while let Some(e) = self.ncp.poll_event() {
                    self.ncp_events.push(e);
                }
                if !moved {
                    return;
                }
            }
            panic!("link did not settle");
        }

        /// Advance the clock to the next timer and run it.
        fn tick(&mut self) {
            let next = [self.host.poll_timeout(), self.ncp.poll_timeout()]
                .into_iter()
                .flatten()
                .min();
            if let Some(t) = next {
                self.now = self.now.max(t);
            }
            self.host.handle_timeout(self.now);
            self.ncp.handle_timeout(self.now);
            self.settle();
        }

        fn connect(&mut self) {
            self.host.reset(self.now);
            self.settle();
            assert!(self.host.is_connected() && self.ncp.is_connected());
            assert_eq!(
                self.host_events.remove(0),
                Event::Connected {
                    reset_code: reset_code::SOFTWARE
                }
            );
            assert_eq!(
                self.ncp_events.remove(0),
                Event::Connected {
                    reset_code: reset_code::SOFTWARE
                }
            );
        }

        fn data(events: &[Event]) -> Vec<Vec<u8>> {
            events
                .iter()
                .filter_map(|e| match e {
                    Event::Data(d) => Some(d.clone()),
                    _ => None,
                })
                .collect()
        }
    }

    fn payload(i: u8) -> Vec<u8> {
        vec![i, 0x80, 0x00, i, i ^ 0x7E]
    }

    #[test]
    fn delivers_in_order_both_ways() {
        let mut p = Pair::new();
        p.connect();
        for i in 0..20 {
            p.host.send(payload(i), p.now).unwrap();
            p.ncp.send(payload(100 + i), p.now).unwrap();
        }
        p.settle();
        assert_eq!(
            Pair::data(&p.ncp_events),
            (0..20).map(payload).collect::<Vec<_>>()
        );
        assert_eq!(
            Pair::data(&p.host_events),
            (100..120).map(payload).collect::<Vec<_>>()
        );
        assert_eq!(p.host.counters().tx_retransmits, 0);
        assert!(p.host.poll_timeout().is_none());
    }

    #[test]
    fn recovers_from_lost_frames_both_ways() {
        let mut p = Pair::new();
        p.connect();
        // Lose about a quarter of the frames in each direction, pseudo-randomly.
        fn lossy(seed: u32) -> Box<dyn FnMut(usize) -> bool> {
            let mut state = seed;
            Box::new(move |_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state % 4 == 0
            })
        }
        p.drop_to_ncp = lossy(0x1234_5678);
        p.drop_to_host = lossy(0x9abc_def1);
        for i in 0..30 {
            p.host.send(payload(i), p.now).unwrap();
            p.ncp.send(payload(100 + i), p.now).unwrap();
        }
        p.settle();
        for _ in 0..2000 {
            assert!(
                !p.host_events.iter().any(|e| matches!(e, Event::Down(_))),
                "{:?}",
                p.host_events
            );
            if Pair::data(&p.ncp_events).len() == 30 && Pair::data(&p.host_events).len() == 30 {
                break;
            }
            p.tick();
        }
        assert_eq!(
            Pair::data(&p.ncp_events),
            (0..30).map(payload).collect::<Vec<_>>()
        );
        assert_eq!(
            Pair::data(&p.host_events),
            (100..130).map(payload).collect::<Vec<_>>()
        );
        assert!(p.host.counters().tx_retransmits > 0);
        assert!(p.host.is_connected() && p.ncp.is_connected());
    }

    #[test]
    fn corrupted_frame_triggers_nak_and_resend() {
        let mut p = Pair::new();
        p.connect();
        p.host.send(payload(1), p.now).unwrap();
        // The DATA frame arrives with a bad CRC.
        assert!(p.host.poll_transmit().is_some());
        p.ncp.handle_bad_frame(FrameError::BadCrc, p.now);
        p.settle();
        assert_eq!(p.ncp.counters().tx_naks, 1);
        assert_eq!(Pair::data(&p.ncp_events), vec![payload(1)]);
        assert_eq!(p.host.counters().tx_retransmits, 1);
    }

    #[test]
    fn link_goes_down_after_repeated_ack_timeouts() {
        let mut p = Pair::new();
        p.connect();
        p.drop_to_ncp = Box::new(|_| true);
        p.host.send(payload(1), p.now).unwrap();
        p.settle();
        for _ in 0..10 {
            p.tick();
        }
        assert_eq!(
            p.host_events.last(),
            Some(&Event::Down(DownReason::AckTimeouts))
        );
        assert!(!p.host.is_connected());
        assert_eq!(p.host.send(payload(2), p.now), Err(SendError::NotConnected));
    }

    #[test]
    fn rst_is_retried_then_gives_up() {
        let mut p = Pair::new();
        p.drop_to_ncp = Box::new(|_| true);
        p.host.reset(p.now);
        p.settle();
        for _ in 0..5 {
            p.tick();
        }
        assert_eq!(p.sent_to_ncp, 3);
        assert_eq!(p.host_events, vec![Event::Down(DownReason::ResetTimeout)]);
    }

    #[test]
    fn unexpected_rstack_takes_the_link_down() {
        let mut p = Pair::new();
        p.connect();
        p.host.handle_frame(
            Frame::RstAck {
                version: 2,
                code: reset_code::WATCHDOG,
            },
            p.now,
        );
        assert_eq!(
            p.host.poll_event(),
            Some(Event::Down(DownReason::NcpReset {
                code: reset_code::WATCHDOG
            }))
        );
    }

    #[test]
    fn ack_period_adapts_within_bounds() {
        let mut p = Pair::new();
        p.connect();
        for i in 0..50 {
            p.host.send(payload(i), p.now).unwrap();
            p.settle();
        }
        // Instant replies pull the period down to the minimum.
        assert_eq!(p.host.ack_period(), Config::default().ack_time_min);
    }

    #[test]
    fn window_limits_frames_in_flight() {
        let mut p = Pair::new();
        p.connect();
        for i in 0..10 {
            p.host.send(payload(i), p.now).unwrap();
        }
        let mut frames = 0;
        while p.host.poll_transmit().is_some() {
            frames += 1;
        }
        assert_eq!(frames, 3);
    }
}
