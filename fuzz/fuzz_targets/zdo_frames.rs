//! ZDO frames for every cluster id from arbitrary bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let [lo, hi, rest @ ..] = data {
        let cluster = u16::from_le_bytes([*lo, *hi]);
        if let Ok(message) = zigbee_zdo::Message::parse(cluster, rest) {
            let (cluster_again, bytes) = message.encode();
            assert_eq!(cluster_again, cluster);
            assert_eq!(zigbee_zdo::Message::parse(cluster, &bytes).ok(), Some(message));
        }
    }
});
