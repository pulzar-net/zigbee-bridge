//! Hostile frames and timer ticks into both ends of an ASH connection: the
//! state machine never panics and its queues stay bounded.
#![no_main]

use std::time::{Duration, Instant};

use libfuzzer_sys::fuzz_target;
use zigbee_ash::{Config, Connection, Decoder, Role};

fuzz_target!(|data: &[u8]| {
    let start = Instant::now();
    for role in [Role::Host, Role::Ncp] {
        let mut conn = Connection::new(role, Config::default());
        let mut decoder = Decoder::new();
        let mut now = start;
        conn.reset(now);
        for (i, byte) in data.iter().enumerate() {
            match byte {
                0xF0 => {
                    now += Duration::from_millis(500);
                    conn.handle_timeout(now);
                }
                0xF1 => {
                    let _ = conn.send(vec![i as u8, 0, 0, *byte], now);
                }
                _ => match decoder.push(*byte) {
                    Some(Ok(frame)) => conn.handle_frame(frame, now),
                    Some(Err(e)) => conn.handle_bad_frame(e, now),
                    None => {}
                },
            }
            let mut out = 0;
            while conn.poll_transmit().is_some() {
                out += 1;
                assert!(out < 1000);
            }
            while conn.poll_event().is_some() {}
        }
    }
});
