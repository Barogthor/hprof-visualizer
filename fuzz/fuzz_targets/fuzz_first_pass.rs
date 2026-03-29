#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (records, id_size) =
        match hprof_parser::parse_header(data) {
            Ok(h) => {
                if h.records_start >= data.len() {
                    return;
                }
                (&data[h.records_start..], h.id_size)
            }
            Err(_) => (data, hprof_parser::IdSize::Eight),
        };
    let mut obs = hprof_api::NullProgressObserver;
    let mut notifier =
        hprof_api::ProgressNotifier::new(&mut obs);
    let _ =
        hprof_parser::indexer::first_pass::run_first_pass(
            records,
            id_size,
            0,
            &mut notifier,
            hprof_api::MemoryBudget::default(),
        );
});
