#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = fallow_core::extract::extract_astro_frontmatter(s);
        let _ = fallow_core::extract::extract_astro_template_regions(s);
        let _ = fallow_core::extract::extract_astro_style_regions(s);
    }
});
