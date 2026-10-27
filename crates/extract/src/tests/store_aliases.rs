//! Store `ReturnType` aliases declared in a function body or a block.
//!
//! `type Local = ReturnType<typeof useCounterStore>` names the store type. A
//! parameter typed with the alias in the same scope reads store members. A
//! nearer type of the same name shadows the alias, the same as in TypeScript.

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::parse_source_to_module;

const STORE_IMPORTS: &str = "import { useCounterStore, useOtherStore } from './store'\n";

fn parse(body: &str) -> ModuleInfo {
    let source = format!("{STORE_IMPORTS}{body}");
    parse_source_to_module(FileId(0), Path::new("src/read.ts"), &source, 0, true)
}

fn reads(info: &ModuleInfo, store: &str, member: &str) -> bool {
    info.member_accesses
        .iter()
        .any(|access| access.object == store && access.member == member)
}

#[test]
fn function_body_alias_credits_the_store() {
    let info = parse(
        "export function setup() {\n\
           type Local = ReturnType<typeof useCounterStore>\n\
           const read = (store: Local) => store.doubled\n\
           return read(useCounterStore())\n\
         }\n",
    );
    assert!(reads(&info, "useCounterStore", "doubled"));
}

#[test]
fn block_alias_credits_the_store() {
    let info = parse(
        "if (ready) {\n\
           type Local = ReturnType<typeof useCounterStore>\n\
           const read = (store: Local) => store.doubled\n\
           read(useCounterStore())\n\
         }\n",
    );
    assert!(reads(&info, "useCounterStore", "doubled"));
}

#[test]
fn nested_function_uses_the_alias_of_an_outer_function() {
    let info = parse(
        "export function setup() {\n\
           type Local = ReturnType<typeof useCounterStore>\n\
           function inner() {\n\
             const read = (store: Local) => store.doubled\n\
             return read(useCounterStore())\n\
           }\n\
           return inner()\n\
         }\n",
    );
    assert!(reads(&info, "useCounterStore", "doubled"));
}

#[test]
fn inner_type_of_the_same_name_shadows_the_alias() {
    let info = parse(
        "export function setup() {\n\
           type Local = ReturnType<typeof useCounterStore>\n\
           function inner() {\n\
             type Local = { doubled: number }\n\
             const read = (value: Local) => value.doubled\n\
             return read({ doubled: 1 })\n\
           }\n\
           return inner()\n\
         }\n",
    );
    assert!(!reads(&info, "useCounterStore", "doubled"));
}

#[test]
fn local_non_store_type_shadows_a_module_store_alias() {
    let info = parse(
        "type Local = ReturnType<typeof useCounterStore>\n\
         export function setup() {\n\
           type Local = { doubled: number }\n\
           const read = (value: Local) => value.doubled\n\
           return read({ doubled: 1 })\n\
         }\n",
    );
    assert!(!reads(&info, "useCounterStore", "doubled"));
}

#[test]
fn generic_alias_is_not_a_store_alias() {
    let info = parse(
        "export function setup() {\n\
           type Local<T> = T extends string ? ReturnType<typeof useCounterStore> : never\n\
           const read = (store: Local<string>) => store.doubled\n\
           return read(useCounterStore())\n\
         }\n",
    );
    assert!(!reads(&info, "useCounterStore", "doubled"));
}

#[test]
fn generic_store_alias_credits_the_store() {
    let info = parse(
        "export function setup() {\n\
           type Local<T = unknown> = ReturnType<typeof useCounterStore>\n\
           const read = (store: Local) => store.doubled\n\
           return read(useCounterStore())\n\
         }\n",
    );
    assert!(reads(&info, "useCounterStore", "doubled"));
}

#[test]
fn parameter_type_resolves_in_the_scope_outside_the_body() {
    // In TypeScript, the parameter type `S` names the module alias, not the
    // alias that the function body declares.
    let info = parse(
        "type S = ReturnType<typeof useOtherStore>\n\
         export function read(store: S) {\n\
           type S = ReturnType<typeof useCounterStore>\n\
           return store.doubled\n\
         }\n",
    );
    assert!(reads(&info, "useOtherStore", "doubled"));
    assert!(!reads(&info, "useCounterStore", "doubled"));
}
