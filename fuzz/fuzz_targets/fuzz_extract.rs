#![no_main]
use std::path::Path;

use fallow_core::discover::FileId;
use libfuzzer_sys::fuzz_target;

#[path = "common/extract_paths.rs"]
mod extract_paths;

use extract_paths::PATHS;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    let Ok(source) = std::str::from_utf8(rest) else {
        return;
    };
    let path = Path::new(PATHS[usize::from(selector) % PATHS.len()]);
    let _ = fallow_core::extract::parse_from_content(FileId(0), path, source);
});
