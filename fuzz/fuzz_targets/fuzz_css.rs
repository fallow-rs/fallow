#![no_main]
use libfuzzer_sys::fuzz_target;

#[path = "common/worker_stack.rs"]
mod worker_stack;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Exercise both CSS and SCSS strip pipelines; the first input byte
        // toggles the is_scss flag so the fuzzer covers both branches.
        let is_scss = data.first().copied().unwrap_or(0) & 1 == 1;
        worker_stack::on_worker_stack(|| {
            let _ = fallow_core::extract::extract_css_module_exports(s, is_scss);
            // `fallow health --css` reads standard CSS only, and it parses
            // each color declaration value on its own.
            if !is_scss {
                let _ = fallow_extract::compute_css_analytics(s);
                let _ = fallow_extract::parse_css_color_rgb(s);
            }
        });
    }
});
