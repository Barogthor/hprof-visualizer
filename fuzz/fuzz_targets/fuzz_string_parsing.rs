#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    use hprof_parser::IdSize;
    use hprof_parser::RecordReader;

    let id_size = if data[0] & 1 == 0 {
        IdSize::Four
    } else {
        IdSize::Eight
    };
    let payload = &data[1..];
    let payload_len = payload.len() as u32;
    let mut reader =
        RecordReader::new(payload, id_size);
    let _ = reader.parse_string_ref(
        payload_len,
        0,
    );

    if payload.len() >= 12 {
        let sref = hprof_parser::HprofStringRef {
            id: 0,
            offset: u64::from_be_bytes(
                payload[..8].try_into().unwrap(),
            ),
            len: u32::from_be_bytes(
                payload[8..12].try_into().unwrap(),
            ),
        };
        let _ = sref.resolve(&payload[12..]);
    }
});
