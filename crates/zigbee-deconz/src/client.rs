//! An async deCONZ client: one request at a time, and every device state
//! the firmware reports as an event (it says there when a received frame
//! or a delivery confirm is waiting to be read).

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

use crate::frame::{Decoder, Frame};
use crate::ids::{command, platform};
use crate::message::{DecodeError, Request, Response};

/// Client errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DeconzError {
    /// The port closed or failed.
    #[error("deCONZ port down: {0}")]
    Down(String),
    /// Nothing answered `VERSION`.
    #[error("no deCONZ reply (is this a ConBee or RaspBee at the right baud rate?)")]
    NoReply,
    /// No response in time.
    #[error("no response to deCONZ command {0:#04x}")]
    Timeout(u8),
    /// The command failed.
    #[error("deCONZ command {command:#04x} failed with status {status}")]
    Status {
        /// Command.
        command: u8,
        /// Status ([`crate::ids::status`]).
        status: u8,
    },
    /// A response did not parse.
    #[error("bad deCONZ response: {0}")]
    Decode(#[from] DecodeError),
    /// The firmware is too old.
    #[error(
        "deCONZ firmware {0:#010x} is not supported: update it (ConBee II 0x26580700 or newer)"
    )]
    Unsupported(u32),
}

/// The firmware version, `0xMMmmPP00`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareVersion(pub u32);

impl FirmwareVersion {
    /// The platform byte ([`platform`]).
    pub fn platform(&self) -> u8 {
        (self.0 >> 8) as u8
    }

    /// The product the platform implies.
    pub fn product(&self) -> &'static str {
        match self.platform() {
            platform::AVR => "ConBee/RaspBee",
            platform::ARM => "ConBee II/RaspBee II",
            platform::EFR32 => "ConBee III",
            _ => "deCONZ",
        }
    }

    /// The firmware supports what the host needs: the network frame
    /// counter parameter and ZDP pass-through (ConBee II 0x26580700 and
    /// newer; every ConBee III).
    pub fn is_supported(&self) -> bool {
        self.0 >= 0x2658_0000 || self.platform() == platform::EFR32
    }

    /// `0x26780700`.
    pub fn text(&self) -> String {
        format!("{:#010x}", self.0)
    }
}

/// Something the firmware sent on its own, or a state it reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeconzEvent {
    /// The device state, from any frame that carries one.
    State(u8),
    /// An unsolicited frame.
    Notice(Response),
    /// The port is down; every later request fails.
    Down(String),
}

/// Client settings.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// How long to wait for each response.
    pub response_timeout: Duration,
    /// `VERSION` attempts at start.
    pub version_attempts: u32,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            response_timeout: Duration::from_secs(5),
            version_attempts: 3,
        }
    }
}

struct Pending {
    command: u8,
    seq: u8,
    reply: oneshot::Sender<Result<Response, DeconzError>>,
}

struct Inner {
    write: mpsc::Sender<Vec<u8>>,
    down: Mutex<Option<String>>,
    lock: tokio::sync::Mutex<u8>,
    pending: Mutex<Option<Pending>>,
    timeout: Duration,
    _close: oneshot::Sender<()>,
}

/// A deCONZ client. Clones share one connection.
#[derive(Clone)]
pub struct Deconz {
    inner: Arc<Inner>,
    version: FirmwareVersion,
}

impl std::fmt::Debug for Deconz {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Deconz")
            .field("version", &self.version.text())
            .finish_non_exhaustive()
    }
}

impl Deconz {
    /// Connect over `io` and read the firmware version, requiring a
    /// supported one.
    pub async fn start<T>(
        io: T,
        config: ClientConfig,
    ) -> Result<(Deconz, mpsc::UnboundedReceiver<DeconzEvent>), DeconzError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (d, events) = Deconz::open(io, config).await?;
        if !d.version.is_supported() {
            return Err(DeconzError::Unsupported(d.version.0));
        }
        Ok((d, events))
    }

    /// Connect over `io` and read the firmware version: what a probe needs.
    pub async fn open<T>(
        io: T,
        config: ClientConfig,
    ) -> Result<(Deconz, mpsc::UnboundedReceiver<DeconzEvent>), DeconzError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (inner, events) = spawn(io, &config);
        let mut d = Deconz {
            inner,
            version: FirmwareVersion(0),
        };
        for _ in 0..config.version_attempts.max(1) {
            match d.request(Request::Version).await {
                Ok(Response::Version { version }) => {
                    d.version = FirmwareVersion(version);
                    return Ok((d, events));
                }
                Err(DeconzError::Timeout(_)) => continue,
                Err(e) => return Err(e),
                Ok(_) => continue,
            }
        }
        Err(DeconzError::NoReply)
    }

    /// The firmware version.
    pub fn version(&self) -> FirmwareVersion {
        self.version
    }

    /// Send a request and wait for its response.
    pub async fn request(&self, request: Request) -> Result<Response, DeconzError> {
        let command = request.command();
        let mut seq = self.inner.lock.lock().await;
        if let Some(why) = self.inner.down.lock().ok().and_then(|d| d.clone()) {
            return Err(DeconzError::Down(why));
        }
        *seq = seq.wrapping_add(1);
        let this = *seq;
        let (tx, rx) = oneshot::channel();
        if let Ok(mut p) = self.inner.pending.lock() {
            *p = Some(Pending {
                command,
                seq: this,
                reply: tx,
            });
        }
        tracing::trace!(command, seq = this, "deconz tx");
        self.inner
            .write
            .send(request.encode(this).encode())
            .await
            .map_err(|_| DeconzError::Down(self.down_reason()))?;
        let result = match tokio::time::timeout(self.inner.timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(DeconzError::Down(self.down_reason())),
            Err(_) => Err(DeconzError::Timeout(command)),
        };
        if let Ok(mut p) = self.inner.pending.lock() {
            *p = None;
        }
        result
    }

    /// Read a parameter's value.
    pub async fn read_parameter(&self, param: u8, extra: &[u8]) -> Result<Vec<u8>, DeconzError> {
        match self
            .request(Request::ReadParameter {
                param,
                extra: extra.to_vec(),
            })
            .await?
        {
            Response::ReadParameter { value, .. } => Ok(value),
            _ => Err(DeconzError::Status {
                command: command::READ_PARAMETER,
                status: crate::ids::status::ERROR,
            }),
        }
    }

    /// Write a parameter.
    pub async fn write_parameter(&self, param: u8, value: &[u8]) -> Result<(), DeconzError> {
        self.request(Request::WriteParameter {
            param,
            value: value.to_vec(),
        })
        .await
        .map(|_| ())
    }

    fn down_reason(&self) -> String {
        self.inner
            .down
            .lock()
            .ok()
            .and_then(|d| d.clone())
            .unwrap_or_else(|| "port closed".into())
    }
}

fn spawn<T>(io: T, config: &ClientConfig) -> (Arc<Inner>, mpsc::UnboundedReceiver<DeconzEvent>)
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    let (read, mut write) = tokio::io::split(io);
    let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(16);
    let (close_tx, close_rx) = oneshot::channel();
    let inner = Arc::new(Inner {
        write: write_tx,
        down: Mutex::new(None),
        lock: tokio::sync::Mutex::new(0),
        pending: Mutex::new(None),
        timeout: config.response_timeout,
        _close: close_tx,
    });
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(bytes) = write_rx.recv().await {
            if write.write_all(&bytes).await.is_err() || write.flush().await.is_err() {
                break;
            }
        }
    });
    tokio::spawn(reader(Arc::downgrade(&inner), read, close_rx, event_tx));
    (inner, event_rx)
}

async fn reader<R>(
    weak: Weak<Inner>,
    mut read: R,
    mut close: oneshot::Receiver<()>,
    events: mpsc::UnboundedSender<DeconzEvent>,
) where
    R: AsyncRead + Unpin,
{
    let mut decoder = Decoder::new();
    let mut buf = [0u8; 256];
    let reason = loop {
        let n = tokio::select! {
            r = read.read(&mut buf) => r,
            _ = &mut close => return,
        };
        let n = match n {
            Ok(0) => break "port closed".to_string(),
            Ok(n) => n,
            Err(e) => break format!("read: {e}"),
        };
        let Some(inner) = weak.upgrade() else { return };
        for frame in decoder.push(buf.get(..n).unwrap_or(&[])) {
            match frame {
                Ok(frame) => handle(&inner, frame, &events),
                Err(e) => tracing::warn!(error = %e, "dropping deCONZ frame"),
            }
        }
    };
    tracing::warn!(%reason, "deCONZ port down");
    let Some(inner) = weak.upgrade() else { return };
    if let Ok(mut d) = inner.down.lock() {
        *d = Some(reason.clone());
    }
    if let Some(p) = inner.pending.lock().ok().and_then(|mut p| p.take()) {
        let _ = p.reply.send(Err(DeconzError::Down(reason.clone())));
    }
    let _ = events.send(DeconzEvent::Down(reason));
}

fn handle(inner: &Inner, frame: Frame, events: &mpsc::UnboundedSender<DeconzEvent>) {
    let unsolicited = matches!(
        frame.command,
        command::DEVICE_STATE_CHANGED | command::MAC_POLL | command::ZIGBEE_GREEN_POWER
    );
    if unsolicited {
        match Response::decode(&frame) {
            Ok(r) => {
                if let Some(s) = r.device_state() {
                    let _ = events.send(DeconzEvent::State(s));
                }
                let _ = events.send(DeconzEvent::Notice(r));
            }
            Err(e) => tracing::trace!(error = %e, "ignoring deCONZ notice"),
        }
        return;
    }
    let pending = inner.pending.lock().ok().and_then(|mut p| p.take());
    let Some(pending) = pending else {
        tracing::warn!(
            command = frame.command,
            seq = frame.seq,
            "deCONZ response with no request waiting"
        );
        return;
    };
    if frame.command != pending.command || frame.seq != pending.seq {
        tracing::warn!(
            command = frame.command,
            seq = frame.seq,
            "deCONZ response for another request"
        );
        if let Ok(mut p) = inner.pending.lock() {
            *p = Some(pending);
        }
        return;
    }
    let result = if frame.status != crate::ids::status::SUCCESS {
        Err(DeconzError::Status {
            command: frame.command,
            status: frame.status,
        })
    } else {
        Response::decode(&frame).map_err(DeconzError::from)
    };
    if let Ok(r) = &result {
        if let Some(s) = r.device_state() {
            let _ = events.send(DeconzEvent::State(s));
        }
    }
    let _ = pending.reply.send(result);
}
