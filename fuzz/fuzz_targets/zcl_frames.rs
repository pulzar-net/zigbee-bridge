//! ZCL frames, global commands, values and every table-driven cluster
//! command from arbitrary bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use zigbee_primitives::wire::Reader;
use zigbee_zcl::{Direction, Frame, FrameType, Global, Value, CLUSTERS};

fuzz_target!(|data: &[u8]| {
    let _ = Value::decode_typed(&mut Reader::new(data));
    if let Ok(frame) = Frame::parse(data) {
        assert_eq!(Frame::parse(&frame.encode()), Ok(frame.clone()));
        match frame.header.frame_type {
            FrameType::Global => {
                let _ = Global::decode(frame.header.command, &frame.payload);
            }
            FrameType::Cluster => {
                for cluster in CLUSTERS {
                    for direction in [Direction::ClientToServer, Direction::ServerToClient] {
                        if let Some(def) = cluster.command(direction, frame.header.command) {
                            let _ = def.decode(&frame.payload);
                        }
                    }
                }
            }
        }
    }
});
