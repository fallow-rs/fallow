//! Kysely's `FileMigrationProvider` loads each script file of its
//! `migrationFolder` with a dynamic `import()` (issue #5294). A folder below
//! the module directory becomes a glob pattern over that folder, without
//! recursion, that credits the whole module.

use fallow_types::extract::{ImportedName, ModuleLoadMechanism};

use crate::tests::parse_ts as parse_source;

const SUFFIX: &str = ".{js,ts,mjs,mts,cjs,cts}";

const KYSELY_IMPORT: &str = "import { FileMigrationProvider } from 'kysely';\n";

fn patterns(source: &str) -> Vec<(String, Option<String>)> {
    parse_source(source)
        .dynamic_import_patterns
        .iter()
        .map(|pattern| (pattern.prefix.clone(), pattern.suffix.clone()))
        .collect()
}

fn with_kysely_import(body: &str) -> String {
    format!("{KYSELY_IMPORT}{body}")
}

#[test]
fn a_join_on_import_meta_dirname_loads_the_folder() {
    let source = "import { join } from 'node:path';\n\
         import { FileMigrationProvider } from 'kysely/migration';\n\
         export const provider = new FileMigrationProvider({\n\
           fs, path,\n\
           migrationFolder: join(import.meta.dirname, '..', 'schema/migrations'),\n\
         });\n";
    assert_eq!(
        patterns(source),
        [(
            "../schema/migrations/".to_string(),
            Some(SUFFIX.to_string())
        )]
    );
}

#[test]
fn the_pattern_excludes_the_sub_folders_and_declaration_files() {
    let source = with_kysely_import(
        "export const provider = new FileMigrationProvider({\n\
           fs, path, migrationFolder: path.join(__dirname, 'migrations'),\n\
         });\n",
    );
    let info = parse_source(&source);
    assert_eq!(
        info.dynamic_import_patterns[0].excludes,
        ["./migrations/*/**", "./migrations/*.d.{ts,mts,cts}"]
    );
}

#[test]
fn the_pattern_credits_the_whole_module_with_esm_loading() {
    let source = with_kysely_import(
        "export const provider = new FileMigrationProvider({\n\
           fs, path, migrationFolder: path.join(__dirname, 'migrations'),\n\
         });\n",
    );
    let info = parse_source(&source);
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(
        info.dynamic_import_patterns[0].imported_name,
        ImportedName::Namespace
    );
    assert_eq!(
        info.dynamic_import_patterns[0].mechanism,
        ModuleLoadMechanism::EsModule
    );
}

#[test]
fn a_const_folder_in_shorthand_form_loads_the_folder() {
    let source = "import * as path from 'node:path';\n\
         import { FileMigrationProvider } from 'kysely';\n\
         const migrationFolder = path.join(__dirname, 'db', 'migrations');\n\
         export const provider = new FileMigrationProvider({ fs, path, migrationFolder });\n";
    assert_eq!(
        patterns(source),
        [("./db/migrations/".to_string(), Some(SUFFIX.to_string()))]
    );
}

#[test]
fn a_member_of_a_namespace_import_loads_the_folder() {
    let source = "import * as kysely from 'kysely';\n\
         export const provider = new kysely.FileMigrationProvider({\n\
           fs, path, migrationFolder: path.join(__dirname, 'migrations'),\n\
         });\n";
    assert_eq!(
        patterns(source),
        [("./migrations/".to_string(), Some(SUFFIX.to_string()))]
    );
}

#[test]
fn a_folder_with_glob_characters_is_escaped() {
    let source = with_kysely_import(
        "export const provider = new FileMigrationProvider({\n\
           fs, path, migrationFolder: path.join(__dirname, 'migrations[v2]'),\n\
         });\n",
    );
    assert_eq!(
        patterns(&source),
        [(
            "./migrations[[]v2[]]/".to_string(),
            Some(SUFFIX.to_string())
        )]
    );
}

#[test]
fn a_folder_that_is_not_module_relative_adds_no_pattern() {
    let source = with_kysely_import(
        "export const provider = new FileMigrationProvider({\n\
           fs, path, migrationFolder: process.env.MIGRATIONS,\n\
         });\n\
         export const other = new FileMigrationProvider({ fs, path, migrationFolder: 'migrations' });\n",
    );
    assert!(patterns(&source).is_empty());
}

#[test]
fn another_constructor_adds_no_pattern() {
    let source = with_kysely_import(
        "export const provider = new OtherProvider({\n\
           migrationFolder: path.join(__dirname, 'migrations'),\n\
         });\n",
    );
    assert!(patterns(&source).is_empty());
}

#[test]
fn a_provider_that_kysely_does_not_export_adds_no_pattern() {
    let source = "import { FileMigrationProvider } from './local-provider';\n\
         export const provider = new FileMigrationProvider({\n\
           fs, path, migrationFolder: path.join(__dirname, 'migrations'),\n\
         });\n";
    assert!(patterns(source).is_empty());
}

#[test]
fn a_provider_without_an_import_adds_no_pattern() {
    let source = "class FileMigrationProvider { constructor(_options) {} }\n\
         export const provider = new FileMigrationProvider({\n\
           migrationFolder: path.join(__dirname, 'migrations'),\n\
         });\n";
    assert!(patterns(source).is_empty());
}
