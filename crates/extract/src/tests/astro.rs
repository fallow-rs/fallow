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
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Page.astro"),
        r#"---
import Layout from '../layouts/Layout.astro';
---
<Layout>
  <script src="../scripts/foo.ts"></script>
</Layout>
"#,
        0,
        false,
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "../scripts/foo.ts"),
        "<script src> in template should be followed (issue #295)"
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "../layouts/Layout.astro"),
        "frontmatter imports should still be extracted"
    );
}

#[test]
fn astro_template_inline_script_imports_followed() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Page.astro"),
        r"---
---
<script>
  import '../scripts/bar';
</script>
",
        0,
        false,
    );
    assert!(
        info.imports.iter().any(|i| i.source == "../scripts/bar"),
        "side-effect import inside inline <script> should be followed (issue #295)"
    );
}

#[test]
fn astro_template_script_combines_both_patterns() {
    // Reproduces the exact issue #295 case: a single .astro file with both a
    // src= reference and a side-effect import inside an inline <script>.
    let info = parse_source_to_module(
        FileId(0),
        Path::new("index.astro"),
        r#"---
---
<script src="../scripts/foo.ts"></script>
<script>
  import '../scripts/bar';
</script>
"#,
        0,
        false,
    );
    assert!(info.imports.iter().any(|i| i.source == "../scripts/foo.ts"));
    assert!(info.imports.iter().any(|i| i.source == "../scripts/bar"));
}
