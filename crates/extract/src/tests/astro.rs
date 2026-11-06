use std::path::Path;

use fallow_types::discover::FileId;

use crate::parse_source_to_module;

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
fn astro_template_only_rendered_component_is_not_unused() {
    // `Header` is imported in frontmatter and used ONLY as a `<Header/>` tag in
    // the markup; the frontmatter semantic pass alone would call it unused, but
    // the template-used credit must keep it out of `unused_import_bindings` so
    // `referenced_import_bindings` (imports minus unused) includes it.
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Page.astro"),
        r"---
import Header from '../components/Header.astro';
import { fmt } from '../lib/util.ts';
const title = fmt('hi');
---
<html><body><h1>{title}</h1><Header /></body></html>
",
        0,
        false,
    );
    assert!(
        !info.unused_import_bindings.contains(&"Header".to_string()),
        "template-rendered component must not be unused: {:?}",
        info.unused_import_bindings
    );
    // `fmt` is used in the frontmatter script, so it is a real value reference.
    assert!(
        info.value_referenced_import_bindings
            .contains(&"fmt".to_string()),
        "frontmatter-used import must be value-referenced: {:?}",
        info.value_referenced_import_bindings
    );
}

#[test]
fn astro_template_expression_binding_is_credited() {
    // `tmplOnly` is consumed only inside a `{tmplOnly()}` markup expression.
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Page.astro"),
        r"---
import { tmplOnly } from '../lib/util.ts';
---
<p>{tmplOnly()}</p>
",
        0,
        false,
    );
    assert!(
        !info
            .unused_import_bindings
            .contains(&"tmplOnly".to_string()),
        "expression-binding import must not be unused: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn astro_frontmatter_import_used_nowhere_is_unused() {
    // `Dead` is imported but referenced in neither the frontmatter script nor the
    // markup, so it is a genuinely-unused binding (the precision the semantic pass
    // restores; previously Astro left every import referenced).
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Page.astro"),
        r"---
import Dead from '../components/Dead.astro';
---
<div>nothing here references Dead</div>
",
        0,
        false,
    );
    assert!(
        info.unused_import_bindings.contains(&"Dead".to_string()),
        "import used nowhere must be unused: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn astro_identifier_inside_script_block_does_not_credit_frontmatter_import() {
    // A frontmatter import whose name appears ONLY inside a `<script>` block (a
    // client-script local) is not a markup use and stays unused.
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Page.astro"),
        r"---
import Widget from '../components/Widget.astro';
---
<div>no markup use</div>
<script>const Widget = 1; console.log(Widget);</script>
",
        0,
        false,
    );
    assert!(
        info.unused_import_bindings.contains(&"Widget".to_string()),
        "script-block identifier must not credit a frontmatter import: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn astro_unused_prop_harvested_dead_used_prop_credited() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Card.astro"),
        r"---
interface Props { title: string; unused: string }
const { title } = Astro.props;
---
<h1>{title}</h1>
",
        0,
        false,
    );
    let title = info
        .component_props
        .iter()
        .find(|p| p.name == "title")
        .expect("title harvested");
    let unused = info
        .component_props
        .iter()
        .find(|p| p.name == "unused")
        .expect("unused harvested");
    assert!(title.used_in_template, "title used in {{title}}: {title:?}");
    assert!(
        !unused.used_in_script && !unused.used_in_template,
        "unused prop must be dead: {unused:?}"
    );
    assert!(!info.has_unharvestable_props);
    assert!(!info.has_props_attrs_fallthrough);
}

#[test]
fn astro_props_member_access_credits_prop() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Card.astro"),
        r"---
interface Props { title: string }
const heading = Astro.props.title;
---
<h1>{heading}</h1>
",
        0,
        false,
    );
    let title = info
        .component_props
        .iter()
        .find(|p| p.name == "title")
        .expect("title harvested");
    assert!(
        title.used_in_script,
        "Astro.props.title must credit title: {title:?}"
    );
}

#[test]
fn astro_props_rest_spread_abstains() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Card.astro"),
        r"---
interface Props { a: string; b: string }
const { a, ...rest } = Astro.props;
---
<div>{a}{rest.b}</div>
",
        0,
        false,
    );
    assert!(
        info.has_props_attrs_fallthrough,
        "a rest element in the Astro.props destructure must abstain"
    );
}

#[test]
fn astro_props_interface_extends_abstains() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Card.astro"),
        r"---
import type { Base } from './base';
interface Props extends Base { title: string }
const { title } = Astro.props;
---
<h1>{title}</h1>
",
        0,
        false,
    );
    assert!(
        info.has_unharvestable_props,
        "interface Props extends Base must abstain (names unresolvable)"
    );
}

#[test]
fn astro_props_whole_object_use_abstains() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Card.astro"),
        r"---
interface Props { a: string }
const all = Astro.props;
doSomething(all);
---
<div>x</div>
",
        0,
        false,
    );
    assert!(
        info.has_props_attrs_fallthrough,
        "a whole-object Astro.props binding use must abstain"
    );
}

#[test]
fn astro_props_template_spread_abstains() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Card.astro"),
        r"---
interface Props { a: string; b: string }
const { a } = Astro.props;
---
<Child {...Astro.props}>{a}</Child>
",
        0,
        false,
    );
    assert!(
        info.has_props_attrs_fallthrough,
        "a {{...Astro.props}} template spread must abstain"
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

#[test]
fn astro_frontmatter_top_level_await_import_stays_lazy() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Page.astro"),
        r#"---
const { data } = await import("./data");
---
<p>{data}</p>
"#,
        0,
        false,
    );
    assert!(
        info.dynamic_imports
            .iter()
            .any(|import| import.source == "./data")
    );
    assert!(crate::tests::eager_dynamic_import_sources(&info).is_empty());
}

fn parse_astro_with_complexity(source: &str) -> fallow_types::extract::ModuleInfo {
    parse_source_to_module(FileId(0), Path::new("Page.astro"), source, 0, true)
}

#[test]
fn astro_script_block_functions_are_scored_at_file_lines() {
    let info = parse_astro_with_complexity(
        r##"---
---
<button id="b">Run</button>
<script>
  function hard(items) {
    let n = 0;
    for (const a of items) {
      if (a.x) {
        for (const b of a.y) {
          if (b.z) {
            for (const c of b.w) {
              if (c.k && c.l) n++;
              else if (c.m || c.o) n--;
            }
          }
        }
      }
    }
    return n;
  }
  document.querySelector("#b").addEventListener("click", () => hard([]));
</script>
"##,
    );

    let hard = info
        .complexity
        .iter()
        .find(|fc| fc.name == "hard")
        .expect("the <script> function must be scored");
    assert_eq!(hard.line, 5);
    assert_eq!(hard.cyclomatic, 10);
    assert_eq!(hard.cognitive, 24);
}

#[test]
fn astro_frontmatter_and_every_script_block_are_scored() {
    let info = parse_astro_with_complexity(
        r"---
const pick = (flag: boolean) => (flag ? 1 : 0);
---
<p>Text</p>
<script is:inline>
function inlineHelper(x) { if (x) return 1; return 0; }
</script>
<script define:vars={{ limit: 3 }}>const check = (n) => { if (n > limit) return 1; return 0; };</script>
<script>
const processed = (v) => v ?? 0;
</script>
",
    );

    let find = |name: &str| {
        info.complexity
            .iter()
            .find(|fc| fc.name == name)
            .unwrap_or_else(|| panic!("{name} must be scored: {:?}", info.complexity))
    };
    assert_eq!(find("pick").line, 2);
    assert_eq!(find("inlineHelper").line, 6);
    assert_eq!(find("processed").line, 10);
    let check = find("check");
    assert_eq!(check.line, 8);
    // `<script define:vars={{ limit: 3 }}>const check = ` is 49 bytes, so the
    // arrow function starts at column 49 of the file line.
    assert_eq!(check.col, 49);
}

#[test]
fn astro_non_code_script_blocks_add_no_complexity() {
    let info = parse_astro_with_complexity(
        r#"<script src="/scripts/app.js"></script>
<script type="application/ld+json">{ "@context": "https://schema.org" }</script>
<script type="text/x-template">function notCode(a) { if (a) return 1; return 0; }</script>
<!-- <script>function commented(a) { if (a) return 1; return 0; }</script> -->
<script>   </script>
"#,
    );

    assert!(
        info.complexity.iter().all(|fc| fc.name == "<template>"),
        "{:?}",
        info.complexity
    );
}

#[test]
fn astro_module_script_block_is_scored() {
    let info = parse_astro_with_complexity(
        "<p>Text</p>\n<script type=\"module\">function f(a) { if (a) return 1; return 0; }</script>\n",
    );
    let f = info
        .complexity
        .iter()
        .find(|fc| fc.name == "f")
        .expect("a module script must be scored");
    assert_eq!(f.line, 2);
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn astro_script_block_complexity_is_skipped_without_need_complexity() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Page.astro"),
        "<script>function f(a) { if (a) return 1; return 0; }</script>\n",
        0,
        false,
    );
    assert!(info.complexity.is_empty());
}
