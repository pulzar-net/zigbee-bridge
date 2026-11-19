//! EZSP headers, responses, callbacks and commands from arbitrary bytes, in
//! both frame formats and both protocol layouts.
#![no_main]

use libfuzzer_sys::fuzz_target;
use zigbee_ezsp::frame::{self, Format};
use zigbee_ezsp::{Callback, Command, ProtocolVersion, Response};

fuzz_target!(|data: &[u8]| {
    for format in [Format::Legacy, Format::Extended] {
        if let Ok(h) = frame::parse(data, format) {
            for v in [ProtocolVersion(13), ProtocolVersion(16)] {
                let _ = Response::decode(v, h.frame_id, h.params);
                let _ = Callback::decode(v, h.frame_id, h.params);
                if let Ok(c) = Command::decode(v, h.frame_id, h.params) {
                    // A command that decodes must encode back to the same command.
                    assert_eq!(Command::decode(v, c.frame_id(), &c.encode(v)), Ok(c));
                }
            }
        }
    }
});
