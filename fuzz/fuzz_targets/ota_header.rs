//! OTA file headers from arbitrary bytes: found or refused, never a panic,
//! and a header that parses describes an image at least as long as itself.
#![no_main]

use libfuzzer_sys::fuzz_target;
use zigbee_host::ota::OtaHeader;

fuzz_target!(|data: &[u8]| {
    if let Ok((offset, h)) = OtaHeader::find(data) {
        assert!(offset <= zigbee_host::ota::MAX_PREFIX);
        assert!(h.total_size >= u32::from(h.header_length));
        let _ = h.fits(h.manufacturer, h.image_type, None, zigbee_types::Ieee(0));
    }
});
