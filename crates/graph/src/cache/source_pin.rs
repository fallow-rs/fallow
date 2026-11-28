//! Pin of the `fallow-graph` sources that build and store the persisted graph.
//!
//! A warm graph cache is reused when `GRAPH_CACHE_VERSION`, the cache mode and
//! the file fingerprints match. The binary version is not part of the key. So
//! a change to how the graph is built, without a version bump, makes a warm
//! run replay the old graph and hide the new behavior.
//!
//! This test hashes the production code of this crate and compares it with
//! [`PIN`]. When the code changes, the test fails until the author decides:
//! bump `GRAPH_CACHE_VERSION` when the persisted graph can hold other facts,
//! or keep the version for a change that cannot alter it. In both cases, set
//! [`PIN`] to the values that the failure message prints.
//!
//! The hash covers each module that `lib.rs` reaches through `mod` items that
//! are not test-only. It hashes the parsed tokens, so comments, doc comments
//! and formatting do not count. It drops each item, field, variant, match arm
//! and statement that compiles only with `test`, and each `cfg_attr` whose
//! condition requires `test`. It does not cover the crates that feed the
//! graph, such as `fallow-extract` and the plugin config code of `fallow-core`.

use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use quote::ToTokens;
use syn::punctuated::Punctuated;
use syn::visit_mut::{self, VisitMut};
use syn::{Attribute, Meta, Token};

use super::GRAPH_CACHE_VERSION;

/// The graph cache version and the source hash that it was decided for.
const PIN: (u32, &str) = (137, "76523f8d1210619e");

#[test]
#[cfg_attr(miri, ignore = "the filesystem is blocked by Miri isolation")]
fn graph_build_sources_match_the_cache_version_pin() {
    let hash = format!("{:016x}", crate_source_hash());
    let version = GRAPH_CACHE_VERSION;
    assert!(
        PIN == (version, hash.as_str()),
        "The production code of fallow-graph changed, or GRAPH_CACHE_VERSION changed.\n\
         When the persisted graph can now hold other facts, bump GRAPH_CACHE_VERSION in \
         crates/graph/src/cache/mod.rs. A warm cache from an earlier build would otherwise \
         replay the old graph.\n\
         Then set PIN in crates/graph/src/cache/source_pin.rs to ({version}, \"{hash}\").",
    );
}

#[test]
fn normalize_drops_comments_formatting_and_test_only_code() {
    let with_test_code = r##"
        //! Module doc.
        use a::b;

        /// Doc.
        /** Block doc. */
        #[cfg_attr(test, derive(Debug))]
        fn kept() -> &'static str {
            let s = r#"{ raw
            {"#; // trailing comment
            #[cfg(test)]
            let probe = 1;
            #[cfg(test)]
            counter += 1;
            #[cfg(all(test, not(miri)))]
            if probe > 0 { record(); }
            s
        }

        #[cfg(all(test, unix))]
        mod tests { fn x() { let _ = '}'; } }

        #[cfg(test)] fn one_line() {}

        struct S {
            kept: u8,
            #[cfg(test)]
            probe: usize,
        }

        enum E { A, #[cfg(test)] Probe }

        fn arms(x: u8) -> S {
            match x {
                #[cfg(test)]
                0 => unreachable!(),
                _ => S { kept: x, #[cfg(test)] probe: 0 },
            }
        }

        #[cfg(not(test))]
        fn production_only() {}
    "##;
    let plain = r##"
        use a::b;
        fn kept() -> &'static str {
            let s = r#"{ raw
            {"#;
            s
        }
        struct S { kept: u8 }
        enum E { A }
        fn arms(x: u8) -> S { match x { _ => S { kept: x } } }
        #[cfg(not(test))]
        fn production_only() {}
    "##;
    assert_eq!(normalize(with_test_code), normalize(plain));
}

#[test]
fn normalize_keeps_production_changes() {
    assert_ne!(normalize("fn a() { 1 }"), normalize("fn a() { 2 }"));
    assert_ne!(
        normalize("#[cfg(not(test))] fn a() {}"),
        normalize("fn a() {}"),
    );
    assert_ne!(
        normalize("const S: &str = \"a\n/// one\nb\";"),
        normalize("const S: &str = \"a\n/// two\nb\";"),
    );
    assert_ne!(
        normalize("#[cfg_attr(unix, derive(Debug))] struct A;"),
        normalize("struct A;"),
    );
}

fn crate_source_hash() -> u64 {
    let root = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR for tests"),
    );
    let root = root.as_path();
    let src = root.join("src");
    let mut files = Vec::new();
    collect_file_module(&src.join("lib.rs"), &src, &mut files);
    files.sort();
    files.dedup();
    let mut hash = Fnv64::new();
    for file in &files {
        let relative = file.strip_prefix(root).unwrap_or(file);
        hash.write(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hash.write(&[0]);
        hash.write(normalize(&read(file)).as_bytes());
        hash.write(&[0]);
    }
    hash.finish()
}

fn read(file: &Path) -> String {
    std::fs::read_to_string(file)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", file.display()))
}

/// Collect `file` and the file modules below it. `child_dir` is the folder of
/// the child modules of `file`.
fn collect_file_module(file: &Path, child_dir: &Path, files: &mut Vec<PathBuf>) {
    let parsed = syn::parse_file(&read(file))
        .unwrap_or_else(|error| panic!("cannot parse {}: {error}", file.display()));
    if requires_test(&parsed.attrs) {
        return;
    }
    files.push(file.to_path_buf());
    let file_dir = file.parent().expect("source file has a parent");
    collect_child_modules(&parsed.items, file_dir, child_dir, files);
}

/// Walk the `mod` items of one module. `path_dir` is the folder that a
/// `#[path]` value is relative to.
fn collect_child_modules(
    items: &[syn::Item],
    path_dir: &Path,
    child_dir: &Path,
    files: &mut Vec<PathBuf>,
) {
    for item in items {
        let syn::Item::Mod(module) = item else {
            continue;
        };
        if requires_test(&module.attrs) {
            continue;
        }
        let name = module.ident.to_string();
        if let Some((_, inner)) = &module.content {
            let inner_dir = child_dir.join(&name);
            collect_child_modules(inner, &inner_dir, &inner_dir, files);
            continue;
        }
        if let Some(path) = path_attribute(&module.attrs) {
            let target = path_dir.join(path);
            let dir = target
                .parent()
                .expect("module file has a parent")
                .to_path_buf();
            collect_file_module(&target, &dir, files);
            continue;
        }
        let flat = child_dir.join(format!("{name}.rs"));
        if flat.is_file() {
            collect_file_module(&flat, &child_dir.join(&name), files);
        } else {
            let nested = child_dir.join(&name);
            collect_file_module(&nested.join("mod.rs"), &nested, files);
        }
    }
}

fn path_attribute(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        let Meta::NameValue(name_value) = &attr.meta else {
            return None;
        };
        if !name_value.path.is_ident("path") {
            return None;
        }
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(value),
            ..
        }) = &name_value.value
        else {
            return None;
        };
        Some(value.value())
    })
}

/// The token text of `source` without comments, doc comments, test-only code
/// and test-only `cfg_attr` attributes.
fn normalize(source: &str) -> String {
    let mut file =
        syn::parse_file(source).unwrap_or_else(|error| panic!("cannot parse source: {error}"));
    StripTestCode.visit_file_mut(&mut file);
    let mut out = String::new();
    print_tokens(file.to_token_stream(), &mut out);
    out
}

/// Print tokens in a fixed form, so the hash does not depend on how
/// `proc-macro2` renders a token stream.
fn print_tokens(stream: TokenStream, out: &mut String) {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    let mut index = 0;
    while index < tokens.len() {
        if let Some(next) = skipped_attribute_end(&tokens, index) {
            index = next;
            continue;
        }
        match &tokens[index] {
            TokenTree::Ident(ident) => {
                out.push_str(&ident.to_string());
                out.push(' ');
            }
            TokenTree::Punct(punct) => {
                out.push(punct.as_char());
                if punct.spacing() == Spacing::Alone {
                    out.push(' ');
                }
            }
            TokenTree::Literal(literal) => {
                out.push_str(&literal.to_string());
                out.push(' ');
            }
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                out.push_str(open);
                print_tokens(group.stream(), out);
                out.push_str(close);
                out.push(' ');
            }
        }
        index += 1;
    }
}

/// When `tokens[index]` starts a `doc` attribute or a `cfg_attr` whose
/// condition requires `test`, the index after that attribute.
fn skipped_attribute_end(tokens: &[TokenTree], index: usize) -> Option<usize> {
    let (meta, end) = attribute_at(tokens, index)?;
    let skip = meta.path().is_ident("doc")
        || (meta.path().is_ident("cfg_attr")
            && matches!(&meta, Meta::List(list) if list
                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .is_ok_and(|args| args.first().is_some_and(meta_requires_test))));
    skip.then_some(end)
}

/// Parse `#[...]` or `#![...]` at `index`: the attribute meta and the index
/// after it.
fn attribute_at(tokens: &[TokenTree], index: usize) -> Option<(Meta, usize)> {
    let TokenTree::Punct(hash) = tokens.get(index)? else {
        return None;
    };
    if hash.as_char() != '#' {
        return None;
    }
    let mut next = index + 1;
    if let Some(TokenTree::Punct(bang)) = tokens.get(next)
        && bang.as_char() == '!'
    {
        next += 1;
    }
    let TokenTree::Group(group) = tokens.get(next)? else {
        return None;
    };
    if group.delimiter() != Delimiter::Bracket {
        return None;
    }
    let meta = syn::parse2::<Meta>(group.stream()).ok()?;
    Some((meta, next + 1))
}

/// True when the leading outer attributes of `node` include a `cfg` that holds
/// only when `test` is set. This reads the tokens, so it works for each node
/// kind, also for expression statements.
fn leads_with_test_cfg(node: &impl ToTokens) -> bool {
    let tokens: Vec<TokenTree> = node.to_token_stream().into_iter().collect();
    let mut index = 0;
    while let Some((meta, next)) = attribute_at(&tokens, index) {
        if is_test_cfg(&meta) {
            return true;
        }
        index = next;
    }
    false
}

/// True when an attribute list has a `cfg` that holds only when `test` is set.
fn requires_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| is_test_cfg(&attr.meta))
}

fn is_test_cfg(meta: &Meta) -> bool {
    let Meta::List(list) = meta else {
        return false;
    };
    list.path.is_ident("cfg")
        && list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .is_ok_and(|metas| metas.iter().any(meta_requires_test))
}

fn meta_requires_test(meta: &Meta) -> bool {
    match meta {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) => {
            let Ok(children) =
                list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            else {
                return false;
            };
            if list.path.is_ident("all") {
                children.iter().any(meta_requires_test)
            } else if list.path.is_ident("any") {
                !children.is_empty() && children.iter().all(meta_requires_test)
            } else {
                false
            }
        }
        Meta::NameValue(_) => false,
    }
}

fn retain_production<T: Clone + ToTokens, P: Default + Clone>(items: &mut Punctuated<T, P>) {
    *items = items
        .iter()
        .filter(|item| !leads_with_test_cfg(*item))
        .cloned()
        .collect();
}

struct StripTestCode;

impl VisitMut for StripTestCode {
    fn visit_file_mut(&mut self, file: &mut syn::File) {
        file.items.retain(|item| !leads_with_test_cfg(item));
        visit_mut::visit_file_mut(self, file);
    }

    fn visit_item_mod_mut(&mut self, module: &mut syn::ItemMod) {
        if let Some((_, items)) = &mut module.content {
            items.retain(|item| !leads_with_test_cfg(item));
        }
        visit_mut::visit_item_mod_mut(self, module);
    }

    fn visit_item_impl_mut(&mut self, item: &mut syn::ItemImpl) {
        item.items.retain(|item| !leads_with_test_cfg(item));
        visit_mut::visit_item_impl_mut(self, item);
    }

    fn visit_item_trait_mut(&mut self, item: &mut syn::ItemTrait) {
        item.items.retain(|item| !leads_with_test_cfg(item));
        visit_mut::visit_item_trait_mut(self, item);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        block.stmts.retain(|stmt| !leads_with_test_cfg(stmt));
        visit_mut::visit_block_mut(self, block);
    }

    fn visit_fields_named_mut(&mut self, fields: &mut syn::FieldsNamed) {
        retain_production(&mut fields.named);
        visit_mut::visit_fields_named_mut(self, fields);
    }

    fn visit_fields_unnamed_mut(&mut self, fields: &mut syn::FieldsUnnamed) {
        retain_production(&mut fields.unnamed);
        visit_mut::visit_fields_unnamed_mut(self, fields);
    }

    fn visit_item_enum_mut(&mut self, item: &mut syn::ItemEnum) {
        retain_production(&mut item.variants);
        visit_mut::visit_item_enum_mut(self, item);
    }

    fn visit_expr_match_mut(&mut self, expr: &mut syn::ExprMatch) {
        expr.arms.retain(|arm| !leads_with_test_cfg(arm));
        // The comma after a block arm is optional, so a removed test arm must
        // not leave a comma behind in the hashed text.
        for arm in &mut expr.arms {
            arm.comma = None;
        }
        visit_mut::visit_expr_match_mut(self, expr);
    }

    fn visit_expr_struct_mut(&mut self, expr: &mut syn::ExprStruct) {
        retain_production(&mut expr.fields);
        visit_mut::visit_expr_struct_mut(self, expr);
    }
}

/// FNV-1a 64. The standard hasher is not stable between Rust versions.
struct Fnv64(u64);

impl Fnv64 {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }

    const fn finish(&self) -> u64 {
        self.0
    }
}
