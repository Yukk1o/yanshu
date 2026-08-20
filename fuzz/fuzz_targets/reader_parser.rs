#![forbid(unsafe_code)]
#![no_main]

use libfuzzer_sys::fuzz_target;
use yanshu_syntax::{ReaderLimits, load_program_source, read_source};

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let _ = read_source(source, ReaderLimits::default());
    let _ = load_program_source(source);
});
