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
