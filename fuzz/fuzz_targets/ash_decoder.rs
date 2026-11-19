//! Any byte stream through the ASH decoder: never panics, and every frame it
//! yields re-encodes to bytes that decode to the same frame.
#![no_main]

use libfuzzer_sys::fuzz_target;
use zigbee_ash::{Decoder, Frame};

fuzz_target!(|data: &[u8]| {
    let mut decoder = Decoder::new();
    for byte in data {
        if let Some(Ok(frame)) = decoder.push(*byte) {
            let mut again = Decoder::new();
            let decoded: Vec<Frame> = frame.encode().iter().filter_map(|b| again.push(*b)).filter_map(Result::ok).collect();
            assert_eq!(decoded, vec![frame]);
        }
    }
    let _ = Frame::parse(data);
});
