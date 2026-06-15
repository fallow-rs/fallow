mod astro;
mod css;
mod graphql;
mod js_ts;
mod mdx;
mod regex_compile;
mod sfc;

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::parse::parse_source_to_module;

/// Shared test helper: parse TypeScript source and return `ModuleInfo`.
pub fn parse_ts(source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new("test.ts"), source, 0, false)
}

/// Shared test helper: parse TypeScript source with complexity metrics.
pub fn parse_ts_with_complexity(source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new("test.ts"), source, 0, true)
}

/// Shared test helper: parse TSX source and return `ModuleInfo`.
pub fn parse_tsx(source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new("test.tsx"), source, 0, false)
}

/// Shared test helper: parse source at a specific path (basename-gated
/// extraction such as the SvelteKit `load()` harvest needs the real filename).
pub fn parse_at_path(path: &str, source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new(path), source, 0, false)
}

#[test]
fn parses_glimmer_typescript_as_typescript() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("component.gts"),
        "import type Service from './service';\nexport type ServiceRef = Service;\n",
        0,
        false,
    );

    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./service");
    assert!(info.imports[0].is_type_only);
    assert!(
        info.exports
            .iter()
            .any(|export| export.name.matches_str("ServiceRef"))
    );
}

/// Regression test for issue #375: a `.gts` file containing both a
/// module-level template expression (assigned to const) and a class-body
/// template must still parse all imports and the default export.
///
/// Before the context-aware stripping fix, the module-level template was
/// blanked to spaces, leaving `const Wrapper: TOC<...> = ;` which is a
/// TypeScript syntax error. oxc bailed and returned zero imports, causing
/// every referenced component to be reported as unused.
#[test]
fn parses_gts_with_multi_template_blocks() {
    let source = "import type {TOC} from '@ember/component/template-only';\n\
                  import Component from '@glimmer/component';\n\
                  import BillingInfo from 'my-app/components/billing-info';\n\
                  \n\
                  const Wrapper: TOC<{ Blocks: { default: [] } }> = <template>\n  <div class=\"wrapper\">{{yield}}</div>\n</template>;\n\
                  \n\
                  export default class InvoiceDetails extends Component {\n  <template>\n    <Wrapper>\n      <BillingInfo />\n    </Wrapper>\n  </template>\n}\n";

    let info = parse_source_to_module(
        FileId(0),
        Path::new("invoice-details.gts"),
        source,
        0,
        false,
    );

    assert_eq!(
        info.imports.len(),
        3,
        "all three import statements should be extracted; got {:?}",
        info.imports.iter().map(|i| &i.source).collect::<Vec<_>>()
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "@ember/component/template-only"),
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "@glimmer/component")
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "my-app/components/billing-info"),
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(e.name, fallow_types::extract::ExportName::Default)),
        "default export should be extracted",
    );
}

/// Regression test for issue #379: a `.gts` file that uses the canonical
/// template-only-component shape (`export default <template>...</template>`
/// with no `const` wrapper) must still parse the import statement and the
/// default export.
///
/// Before the keyword-aware `is_expression_position` fix, the previous
/// non-whitespace byte before `<template>` was `t` (end of `default`),
/// which fell through to blank-out and left `export default ;`, a
/// TypeScript syntax error that made oxc bail and drop every import.
#[test]
fn parses_gts_with_standalone_default_template() {
    let source = "import Icon from 'my-app/components/icon';\n\
                  \n\
                  export default <template>\n  <span class=\"badge\"><Icon /> badge</span>\n</template>\n";

    let info = parse_source_to_module(FileId(0), Path::new("badge.gts"), source, 0, false);

    assert_eq!(
        info.imports.len(),
        1,
        "import statement should be extracted; got {:?}",
        info.imports.iter().map(|i| &i.source).collect::<Vec<_>>()
    );
    assert_eq!(info.imports[0].source, "my-app/components/icon");
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(e.name, fallow_types::extract::ExportName::Default)),
        "default export should be extracted",
    );
}

/// Issue #475: the same source bytes with and without a leading UTF-8 BOM
/// must produce identical extraction results. `parse_source_to_module` strips
/// the BOM as a defense-in-depth step before any line-offset computation or
/// oxc parse, so the byte spans on every export entry must match.
#[test]
fn parse_source_to_module_strips_bom_defense_in_depth() {
    let body = "import { foo } from './foo';\nexport const bar = 1;\n";
    let with_bom = format!("\u{FEFF}{body}");
    let info_plain = parse_ts(body);
    let info_bom = parse_ts(&with_bom);

    assert_eq!(info_plain.imports.len(), info_bom.imports.len());
    assert_eq!(info_plain.exports.len(), info_bom.exports.len());
    let plain_spans: Vec<(u32, u32)> = info_plain
        .exports
        .iter()
        .map(|e| (e.span.start, e.span.end))
        .collect();
    let bom_spans: Vec<(u32, u32)> = info_bom
        .exports
        .iter()
        .map(|e| (e.span.start, e.span.end))
        .collect();
    assert_eq!(
        plain_spans, bom_spans,
        "BOM-bearing source must produce identical export byte spans (no shift by the BOM codepoint)",
    );
}

/// Issue #475: confirm `strip_bom` + hash invariant: the post-strip bytes
/// of a BOM-bearing source equal the post-strip bytes of the same source
/// without BOM, so the cache `content_hash` (xxh3 over post-strip bytes)
/// matches and the cache hits on both shapes.
#[test]
fn bom_stripped_before_hash_so_with_and_without_bom_yield_same_hash() {
    let body = "export const x = 1;\n";
    let plain = body;
    let bom = format!("\u{FEFF}{body}");

    let plain_hash = xxhash_rust::xxh3::xxh3_64(crate::strip_bom(plain).as_bytes());
    let bom_hash = xxhash_rust::xxh3::xxh3_64(crate::strip_bom(&bom).as_bytes());
    assert_eq!(
        plain_hash, bom_hash,
        "post-strip hashes must match so the extraction cache hits regardless of BOM presence",
    );

    let plain_info = parse_ts(plain);
    let bom_info = parse_ts(&bom);
    assert_eq!(
        plain_info.exports.len(),
        bom_info.exports.len(),
        "BOM-bearing and BOM-free source must yield the same number of exports",
    );
}

/// Issue #475: `compute_line_offsets` runs against the post-BOM source, so
/// line numbers for symbols on line 1 are not shifted by the BOM codepoint.
/// This is the user-visible fix: the first reported export of a BOM-bearing
/// file lands on line 1 col 0 (not line 1 col 3).
#[test]
fn bom_stripped_before_line_offsets_so_line_numbers_align() {
    use fallow_types::extract::{byte_offset_to_line_col, compute_line_offsets};

    let body = "export const first = 1;\nexport const second = 2;\n";
    let with_bom = format!("\u{FEFF}{body}");
    let info_plain = parse_ts(body);
    let info_bom = parse_ts(&with_bom);

    let plain_first = info_plain
        .exports
        .iter()
        .find(|e| e.name.matches_str("first"))
        .expect("plain source exports `first`");
    let bom_first = info_bom
        .exports
        .iter()
        .find(|e| e.name.matches_str("first"))
        .expect("BOM-bearing source exports `first`");

    let plain_offsets = compute_line_offsets(body);
    let bom_offsets = compute_line_offsets(crate::strip_bom(&with_bom));
    let plain_pos = byte_offset_to_line_col(&plain_offsets, plain_first.span.start);
    let bom_pos = byte_offset_to_line_col(&bom_offsets, bom_first.span.start);
    assert_eq!(
        plain_pos, bom_pos,
        "line/col must align across BOM presence"
    );
    assert_eq!(
        plain_pos.0, 1,
        "the first export sits on line 1 in both views",
    );
}

#[test]
fn glimmer_template_only_pascal_tag_credits_import() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("app.gts"),
        "import HelloWorld from './hello-world';\nimport { greeting } from './lib';\n\
         <template><HelloWorld @msg={{greeting}} /></template>\n",
        0,
        false,
    );

    assert!(
        info.unused_import_bindings.is_empty(),
        "expected HelloWorld and greeting to be credited via the <template> block, \
         but unused_import_bindings = {:?}",
        info.unused_import_bindings,
    );
}

#[test]
fn glimmer_dotted_template_reference_emits_member_access() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("app.gts"),
        "import * as utils from './utils';\n<template>{{utils.formatDate value}}</template>\n",
        0,
        false,
    );

    assert!(info.unused_import_bindings.is_empty());
    assert!(
        info.member_accesses
            .iter()
            .any(|access| access.object == "utils" && access.member == "formatDate")
    );
}

#[test]
fn glimmer_import_used_only_inside_template_is_not_flagged() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("counter.gts"),
        "import { capitalize } from './helpers';\n\
         <template>{{capitalize name}}</template>\n",
        0,
        false,
    );

    assert!(info.unused_import_bindings.is_empty());
}

fn assert_unused(info: &ModuleInfo, expected: &[&str]) {
    let mut actual: Vec<&str> = info
        .unused_import_bindings
        .iter()
        .map(String::as_str)
        .collect();
    actual.sort_unstable();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    assert_eq!(
        actual, expected,
        "unused_import_bindings did not match expected set"
    );
}

#[test]
fn glimmer_import_referenced_nowhere_is_flagged_unused() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("app.gts"),
        "import { unused } from './lib';\n\
         <template>hello world</template>\n",
        0,
        false,
    );
    assert_unused(&info, &["unused"]);
}

#[test]
fn glimmer_import_referenced_only_via_this_dot_in_template_is_flagged() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("app.gts"),
        "import { greeting } from './lib';\n\
         <template>{{this.greeting}}</template>\n",
        0,
        false,
    );
    assert_unused(&info, &["greeting"]);
}

#[test]
fn glimmer_import_referenced_only_via_arg_in_template_is_flagged() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("app.gts"),
        "import { name } from './lib';\n\
         <template>{{@name}}</template>\n",
        0,
        false,
    );
    assert_unused(&info, &["name"]);
}

#[test]
fn glimmer_import_shadowing_builtin_helper_is_flagged() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("app.gts"),
        "import { each } from './lib';\n\
         <template>{{#each items as |x|}}{{x}}{{/each}}</template>\n",
        0,
        false,
    );
    assert_unused(&info, &["each"]);
}

#[test]
fn glimmer_import_shadowed_by_block_param_is_flagged() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("app.gts"),
        "import { item } from './lib';\n\
         <template>{{#each items as |item|}}{{item}}{{/each}}</template>\n",
        0,
        false,
    );
    assert_unused(&info, &["item"]);
}

#[test]
fn glimmer_mix_of_used_and_unused_imports_flags_only_the_unused() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("app.gts"),
        "import HelloWorld from './hello-world';\n\
         import { greeting } from './lib';\n\
         import { stale } from './lib';\n\
         <template><HelloWorld @msg={{greeting}} /></template>\n",
        0,
        false,
    );
    assert_unused(&info, &["stale"]);
}

#[test]
fn glimmer_strict_mode_helper_imports_from_ember_helper_are_credited() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("form.gts"),
        "import { hash, array, concat, fn, get } from '@ember/helper';\n\
         <template>\n  \
           {{#let (hash a=(array 1 2) label=(concat \"x\" \"y\")) as |opts|}}\n    \
             <button {{on \"click\" (fn this.save opts)}}>{{get opts \"label\"}}</button>\n  \
           {{/let}}\n\
         </template>\n",
        0,
        false,
    );
    assert_unused(&info, &[]);
}

#[test]
fn glimmer_template_this_dot_member_emits_member_access() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("toolbar.gts"),
        "import Component from '@glimmer/component';\n\
         export class Toolbar extends Component {\n  \
           handleSelect = (x) => { void x; };\n  \
           changeTab = (t) => { void t; };\n  \
           <template>\n    \
             <Child @onSelect={{this.handleSelect}} \
                    @changeTab={{this.changeTab}} />\n  \
           </template>\n\
         }\n",
        0,
        false,
    );
    let access_keys: Vec<(&str, &str)> = info
        .member_accesses
        .iter()
        .map(|a| (a.object.as_str(), a.member.as_str()))
        .collect();
    assert!(
        access_keys.contains(&("this", "handleSelect")),
        "expected this.handleSelect member-access from <template>; \
         got {access_keys:?}"
    );
    assert!(
        access_keys.contains(&("this", "changeTab")),
        "expected this.changeTab member-access from <template>; got {access_keys:?}"
    );
}

#[test]
fn glimmer_template_this_dot_member_records_access_with_zero_imports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("no-imports.gts"),
        "export class Widget {\n  \
           handleClick = () => {};\n  \
           <template>\n    \
             <button {{on \"click\" this.handleClick}}>x</button>\n  \
           </template>\n\
         }\n",
        0,
        false,
    );
    let access_keys: Vec<(&str, &str)> = info
        .member_accesses
        .iter()
        .map(|a| (a.object.as_str(), a.member.as_str()))
        .collect();
    assert!(
        access_keys.contains(&("this", "handleClick")),
        "this.handleClick must still be recorded as a member access when \
         the file has zero module-scope imports; got {access_keys:?}",
    );
}

#[test]
fn glimmer_file_with_two_class_components_credits_all_template_imports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("layout.gts"),
        "import Component from '@glimmer/component';\n\
         import { on } from '@ember/modifier';\n\
         import HelloWorld from './hello-world';\n\
         \n\
         export class Header extends Component {\n  \
           greet = (e) => { void e; };\n  \
           <template>\n    \
             <h1>Header</h1>\n    \
             <button {{on \"click\" this.greet}}>greet</button>\n  \
           </template>\n\
         }\n\
         \n\
         export class Footer extends Component {\n  \
           <template>\n    \
             <HelloWorld @msg=\"bye\" />\n  \
           </template>\n\
         }\n",
        0,
        false,
    );
    assert_unused(&info, &[]);
    let access_keys: Vec<(&str, &str)> = info
        .member_accesses
        .iter()
        .map(|a| (a.object.as_str(), a.member.as_str()))
        .collect();
    assert!(
        access_keys.contains(&("this", "greet")),
        "Header.greet referenced via `{{{{on \"click\" this.greet}}}}` must \
         emit a `this.greet` member access; got {access_keys:?}",
    );
}

#[test]
fn glimmer_file_with_two_template_only_components_credits_all_imports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("greetings.gts"),
        "import HelloWorld from './hello-world';\n\
         import { formatDate } from './utils';\n\
         \n\
         export const Greeting = <template>\n  \
           <HelloWorld @msg=\"hi\" />\n\
         </template>;\n\
         \n\
         export const Stamp = <template>\n  \
           {{formatDate this}}\n\
         </template>;\n",
        0,
        false,
    );
    assert_unused(&info, &[]);
}

#[test]
fn glimmer_file_mixing_class_and_template_only_components_credits_all_imports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("mixed.gts"),
        "import Component from '@glimmer/component';\n\
         import { capitalize } from './utils';\n\
         \n\
         export class Heading extends Component {\n  \
           <template><h1>{{capitalize \"hello\"}}</h1></template>\n\
         }\n\
         \n\
         export const Spacer = <template><hr /></template>;\n",
        0,
        false,
    );
    assert_unused(&info, &[]);
}

#[test]
fn glimmer_file_with_two_components_flags_only_genuinely_unused_imports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("layout.gts"),
        "import Component from '@glimmer/component';\n\
         import HelloWorld from './hello-world';\n\
         import { stale } from './lib';\n\
         \n\
         export class Header extends Component {\n  \
           <template><h1>Header</h1></template>\n\
         }\n\
         \n\
         export class Footer extends Component {\n  \
           <template><HelloWorld @msg=\"bye\" /></template>\n\
         }\n",
        0,
        false,
    );
    assert_unused(&info, &["stale"]);
}

#[test]
fn glimmer_file_without_template_still_flags_unused_imports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("plain.gts"),
        "import { unused } from './lib';\nexport const x = 1;\n",
        0,
        false,
    );
    assert_unused(&info, &["unused"]);
}
