#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = fallow_core::extract::extract_sfc_scripts(s);
        let _ = fallow_core::extract::extract_sfc_template_regions(s);
        let _ = fallow_core::extract::extract_sfc_styles(s);
    }
});
