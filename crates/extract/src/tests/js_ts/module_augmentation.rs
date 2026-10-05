//! A top-level `declare module 'pkg' { ... }` in a module file augments the
//! package, so it records the package as a type-only reference. In a script
//! file the same syntax declares an ambient module and records nothing.

use crate::tests::parse_ts as parse_source;

fn type_package_references(source: &str) -> Vec<String> {
    parse_source(source).type_package_references.to_vec()
}

#[test]
fn augmentation_in_a_module_file_references_the_package() {
    let cases: &[(&str, &[&str])] = &[
        (
            "declare module '@x/slots' { interface Map { a: string } }\nexport const a = 1;\n",
            &["@x/slots"],
        ),
        (
            "import type {} from './local';\ndeclare module 'pkg/sub/path' { interface A {} }\n",
            &["pkg"],
        ),
        (
            "declare module 'pkg' { interface A {} }\nexport = {};\n",
            &["pkg"],
        ),
        (
            "import fs = require('fs');\ndeclare module 'pkg' { interface A {} }\n",
            &["pkg"],
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(
            type_package_references(source),
            *expected,
            "source: {source}"
        );
    }
}

#[test]
fn ambient_declaration_or_non_package_name_references_nothing() {
    let cases: &[&str] = &[
        // Script file: an ambient module shim.
        "declare module 'untyped-lib' { export const x: number }\n",
        // `export as namespace` alone does not make a module file.
        "export as namespace Lib;\ndeclare module 'untyped-lib' { export const x: number }\n",
        // Wildcard pattern, relative path and shorthand form.
        "declare module '*.svg' { const s: string; export default s }\nexport {};\n",
        "declare module './local' { interface A {} }\nexport {};\n",
        "declare module 'pkg';\nexport {};\n",
    ];
    for source in cases {
        assert_eq!(
            type_package_references(source),
            Vec::<String>::new(),
            "source: {source}"
        );
    }
}
