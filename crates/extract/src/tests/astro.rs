use std::path::Path;

use fallow_types::discover::FileId;

use crate::parse::parse_source_to_module;

#[test]
fn extracts_astro_frontmatter_imports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Layout.astro"),
        r#"---
import Layout from '../layouts/Layout.astro';
import { Card } from '../components/Card';
const title = "Hello";
---
<Layout title={title}>
  <Card />
</Layout>
"#,
        0,
        false,
    );
    assert_eq!(info.imports.len(), 2);
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "../layouts/Layout.astro")
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "../components/Card")
    );
}

#[test]
fn astro_no_frontmatter_returns_empty() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Simple.astro"),
        "<div>No frontmatter here</div>",
        0,
        false,
    );
    assert!(info.imports.is_empty());
    assert!(info.exports.is_empty());
}

#[test]
fn astro_empty_frontmatter() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Empty.astro"),
        "---\n---\n<div>Content</div>",
        0,
        false,
    );
    assert!(info.imports.is_empty());
}

#[test]
fn astro_frontmatter_with_dynamic_import() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Dynamic.astro"),
        r"---
const mod = await import('../utils/helper');
---
<div>{mod.value}</div>
",
        0,
        false,
    );
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "../utils/helper");
}

#[test]
fn astro_frontmatter_with_reexport() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("ReExport.astro"),
        r"---
export { default as Layout } from '../layouts/Layout.astro';
---
<div>Content</div>
",
        0,
        false,
    );
    assert_eq!(info.re_exports.len(), 1);
}

#[test]
fn astro_template_script_src_followed() {
    // Issue #295: <script src="..."> in the template body should produce
    // an import edge so the referenced file stays reachable.
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Page.astro"),
        r#"---
const title = "Hello";
---
<html>
  <body>
    <h1>{title}</h1>
    <script src="../scripts/foo.ts"></script>
  </body>
</html>
"#,
        0,
        false,
    );
    let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
    assert!(
        sources.contains(&"../scripts/foo.ts"),
        "expected ../scripts/foo.ts in {sources:?}"
    );
}

#[test]
fn astro_template_script_src_has_source_span() {
    let source = r#"---
---
<html>
  <script src="../scripts/foo.ts"></script>
</html>
"#;
    let info = parse_source_to_module(FileId(0), Path::new("Page.astro"), source, 0, false);
    let import = info
        .imports
        .iter()
        .find(|i| i.source == "../scripts/foo.ts")
        .expect("script src import extracted");
    let (line, _col) = fallow_types::extract::byte_offset_to_line_col(
        &info.line_offsets,
        import.source_span.start,
    );
    assert_eq!(line, 4);
    assert_eq!(
        &source[import.source_span.start as usize..import.source_span.end as usize],
        "../scripts/foo.ts"
    );
}

#[test]
fn astro_template_inline_script_imports_followed() {
    // Issue #295: ESM imports inside inline <script> blocks should be
    // followed so their targets stay reachable.
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Page.astro"),
        r"---
---
<html>
  <body>
    <script>
      import '../scripts/bar';
    </script>
  </body>
</html>
",
        0,
        false,
    );
    let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
    assert!(
        sources.contains(&"../scripts/bar"),
        "expected ../scripts/bar in {sources:?}"
    );
}

#[test]
fn astro_inline_script_import_spans_are_original_source_offsets() {
    let source = r"---
---
<html>
  <body>
    <script>
      import '../scripts/bar';
    </script>
  </body>
</html>
";
    let info = parse_source_to_module(FileId(0), Path::new("Page.astro"), source, 0, false);
    let import = info
        .imports
        .iter()
        .find(|i| i.source == "../scripts/bar")
        .expect("inline script import extracted");
    let (line, _col) =
        fallow_types::extract::byte_offset_to_line_col(&info.line_offsets, import.span.start);
    assert_eq!(line, 6);
    assert_eq!(
        &source[import.source_span.start as usize..import.source_span.end as usize],
        "'../scripts/bar'"
    );
}

#[test]
fn astro_template_combines_frontmatter_and_template_imports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Page.astro"),
        r#"---
import Layout from '../layouts/Layout.astro';
---
<Layout>
  <script src="../scripts/foo.ts"></script>
  <script>
    import '../scripts/bar';
  </script>
</Layout>
"#,
        0,
        false,
    );
    let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
    assert!(sources.contains(&"../layouts/Layout.astro"));
    assert!(sources.contains(&"../scripts/foo.ts"));
    assert!(sources.contains(&"../scripts/bar"));
}
