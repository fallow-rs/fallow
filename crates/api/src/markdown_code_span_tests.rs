use std::path::{Path, PathBuf};

use super::markdown_grouped_section;

#[test]
fn grouped_paths_use_safe_code_span_delimiters_and_padding() {
    let paths = vec![
        PathBuf::from("src/ordinary.ts"),
        PathBuf::from("src/one`# injected.md"),
        PathBuf::from("src/two``ticks.ts"),
        PathBuf::from(" leading and trailing "),
        PathBuf::from("`leading-tick.ts"),
    ];
    let mut output = String::new();

    markdown_grouped_section(
        &mut output,
        &paths,
        "Paths",
        Path::new("/project"),
        PathBuf::as_path,
        |_| "detail".to_string(),
    );

    assert!(output.contains("- `src/ordinary.ts`\n"));
    assert!(output.contains("- ``src/one`# injected.md``\n"));
    assert!(output.contains("- ```src/two``ticks.ts```\n"));
    assert!(output.contains("- `  leading and trailing  `\n"));
    assert!(output.contains("- `` `leading-tick.ts ``\n"));
    assert!(!output.contains("\\`"));
}
