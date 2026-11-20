//! MT frames from arbitrary bytes, then every request and indication that
//! decodes from them must encode back to the same thing.
#![no_main]

use libfuzzer_sys::fuzz_target;
use zigbee_znp::{Decoded, Decoder, Indication, Request};

fuzz_target!(|data: &[u8]| {
    let mut decoder = Decoder::new();
    for decoded in decoder.push(data) {
        let Decoded::Frame(frame) = decoded else { continue };
        if let Ok(r) = Request::decode(&frame) {
            assert_eq!(Request::decode(&r.encode()), Ok(r));
        }
        if let Ok(i) = Indication::decode(&frame) {
            assert_eq!(Indication::decode(&i.encode()), Ok(i));
        }
        if let Ok(bytes) = frame.encode() {
            let mut again = Decoder::new();
            assert_eq!(again.push(&bytes), vec![Decoded::Frame(frame)]);
        }
    }
});
