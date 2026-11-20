//! deCONZ frames from arbitrary bytes, then every request and response
//! that decodes from them must encode back to the same thing.
#![no_main]

use libfuzzer_sys::fuzz_target;
use zigbee_deconz::{Decoder, Frame, Request, Response};

fuzz_target!(|data: &[u8]| {
    let _ = Frame::parse(data);
    let mut decoder = Decoder::new();
    for frame in decoder.push(data).into_iter().flatten() {
        if let Ok(r) = Request::decode(&frame) {
            assert_eq!(Request::decode(&r.encode(frame.seq)), Ok(r));
        }
        if let Ok(r) = Response::decode(&frame) {
            assert_eq!(Response::decode(&r.encode(frame.seq, 0)), Ok(r));
        }
        let mut again = Decoder::new();
        assert_eq!(again.push(&frame.encode()), vec![Ok(frame)]);
    }
});
