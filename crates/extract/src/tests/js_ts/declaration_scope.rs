//! Global-scope and triple-slash facts that decide whether a declaration file
//! stays an entry point when nothing imports it.

use crate::tests::{parse_at_path, parse_ts as parse_source};

#[test]
fn script_files_and_global_blocks_have_global_declarations() {
    for source in [
        "declare const BUILD_ID: string;\n",
        "interface Window { flag: boolean }\n",
        "type T = typeof import('./target');\n",
        "export as namespace Lib;\n",
        "import type { A } from './a';\ndeclare global { interface Window { a: A } }\nexport {};\n",
        "declare module 'virtual:config' { export const value: number; }\nexport {};\n",
    ] {
        assert!(
            parse_source(source).has_global_declarations,
            "{source:?} must count as adding global declarations"
        );
    }
}

#[test]
fn plain_module_files_have_no_global_declarations() {
    for source in [
        "export interface A { id: string }\n",
        "import type { A } from './a';\nexport type B = A;\n",
        "export {};\n",
        "declare namespace Local { const x: number }\nexport {};\n",
        "export declare function f(): void;\n",
    ] {
        assert!(
            !parse_source(source).has_global_declarations,
            "{source:?} must not count as adding global declarations"
        );
    }
}

#[test]
fn declaration_files_record_global_declarations() {
    let ambient = parse_at_path("/project/types/env.d.ts", "declare const ENV: string;\n");
    assert!(ambient.has_global_declarations);
    let module = parse_at_path("/project/types/api.d.ts", "export interface Api {}\n");
    assert!(!module.has_global_declarations);
}

#[test]
fn reference_path_directives_are_recorded() {
    let info = parse_source(
        "/// <reference path=\"./types/a.d.ts\" />\n\
         ///<reference path='../b.d.ts'/>\n\
         /// <reference types=\"node\" />\n\
         /// <reference lib=\"dom\" />\n\
         // <reference path=\"./not-a-directive.d.ts\" />\n\
         /* <reference path=\"./block.d.ts\" /> */\n\
         export const value = 1;\n",
    );
    assert_eq!(
        info.triple_slash_reference_paths.to_vec(),
        vec!["./types/a.d.ts".to_string(), "../b.d.ts".to_string()]
    );
}

#[test]
fn reference_directive_without_a_path_records_nothing() {
    for source in [
        "/// <reference no-default-lib=\"true\" />\n",
        "/// <reference typespath=\"./x.d.ts\" />\n",
        "/// <referencepath=\"./x.d.ts\" />\n",
        "/// <reference path=\"\" />\n",
    ] {
        assert!(
            parse_source(source).triple_slash_reference_paths.is_empty(),
            "{source:?} must record no reference path"
        );
    }
}
