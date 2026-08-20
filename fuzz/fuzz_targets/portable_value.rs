#![forbid(unsafe_code)]
#![no_main]

use libfuzzer_sys::fuzz_target;
use yanshu_runtime::json_to_value;

fuzz_target!(|data: &[u8]| {
    if let Ok(document) = serde_json::from_slice(data) {
        let _ = json_to_value(&document);
    }
});
