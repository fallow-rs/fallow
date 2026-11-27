//! The feature flag walk and the component contract scans run only when the
//! parse needs their facts, and the parse cache serves an entry only to a run
//! whose needs the entry covers.
use std::path::Path;

use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{FlagPatterns, ModuleInfo};
use fallow_types::shared_state::ExtractionMode;
use fallow_types::source_fingerprint::SourceFingerprint;

use crate::cache::CacheStore;
use crate::{ParseNeeds, parse_all_files_with_mode};

const TSX: &str = "export function Card({ flag }: { flag?: boolean }) { return <p>{String(flag)}</p>; }\n\
export const enabled = process.env.FEATURE_CHECKOUT === \"1\";\n";

const NO_FACTS: ParseNeeds = ParseNeeds {
    complexity: false,
    call_facts: false,
    flags: false,
    component_contracts: false,
};

const SCANS: ParseNeeds = ParseNeeds {
    flags: true,
    component_contracts: true,
    ..NO_FACTS
};

fn parse_with(path: &str, source: &str, flags: bool, component_contracts: bool) -> ModuleInfo {
    crate::module_parse::parse_source_without_bom(
        FileId(0),
        Path::new(path),
        source,
        0,
        false,
        &crate::module_parse::ExtractionOptions {
            flag_patterns: &FlagPatterns::default(),
            mode: ExtractionMode::Standard,
            call_facts: false,
            flags,
            component_contracts,
        },
    )
}

const fn has_flag_facts(module: &ModuleInfo) -> bool {
    !module.flag_uses.is_empty()
}

const fn has_contracts(module: &ModuleInfo) -> bool {
    module.component_contracts.is_some()
}

#[test]
fn the_flag_walk_follows_the_extraction_option() {
    for flags in [false, true] {
        let module = parse_with("Card.tsx", TSX, flags, true);
        assert_eq!(has_flag_facts(&module), flags, "flags {flags}");
        assert!(has_contracts(&module), "the contract option stays on");
    }
}

#[test]
fn the_contract_scans_follow_the_extraction_option() {
    let sources = [
        ("Card.tsx", TSX),
        (
            "Card.vue",
            "<script setup lang=\"ts\">const props = defineProps<{ flag?: boolean }>();</script><template><p>{{ props.flag }}</p></template>",
        ),
        (
            "Card.gts",
            "import Component from '@glimmer/component'; interface Signature { Args: { flag?: boolean } }; export default class Card extends Component<Signature> { get flag() { return this.args.flag; } }",
        ),
        (
            "page.astro",
            "---\nimport Card from './Card.astro';\ninterface Props { flag?: boolean }\nconst { flag } = Astro.props;\n---\n<Card flag={flag} />\n",
        ),
        ("index.html", "<my-card flag></my-card>\n"),
    ];
    for (path, source) in sources {
        for component_contracts in [false, true] {
            let module = parse_with(path, source, true, component_contracts);
            assert_eq!(
                has_contracts(&module),
                component_contracts,
                "{path} with component_contracts {component_contracts}"
            );
        }
    }
}

fn write_source(root: &Path) -> DiscoveredFile {
    let path = root.join("Card.tsx");
    std::fs::write(&path, TSX).expect("write source");
    DiscoveredFile {
        id: FileId(0),
        path,
        size_bytes: u64::try_from(TSX.len()).expect("source length fits u64"),
    }
}

fn parse(files: &[DiscoveredFile], store: &CacheStore, needs: ParseNeeds) -> crate::ParseResult {
    parse_all_files_with_mode(
        files,
        Some(store),
        needs,
        None,
        &FlagPatterns::default(),
        ExtractionMode::Standard,
    )
}

fn record(store: &mut CacheStore, file: &DiscoveredFile, module: &ModuleInfo, needs: ParseNeeds) {
    let metadata = std::fs::metadata(&file.path).expect("read metadata");
    store.record_module(
        &file.path,
        module,
        SourceFingerprint::from_metadata(&metadata),
        needs,
    );
}

#[test]
fn an_entry_without_the_scans_is_not_served_to_a_run_that_needs_them() {
    let project = tempfile::tempdir().expect("create project");
    let files = [write_source(project.path())];
    let mut store = CacheStore::new(project.path());

    let first = parse(&files, &store, NO_FACTS);
    let module = first.modules.first().expect("first parse produces module");
    assert!(!has_flag_facts(module) && !has_contracts(module));
    record(&mut store, &files[0], module, NO_FACTS);

    for needs in [
        ParseNeeds {
            flags: true,
            ..NO_FACTS
        },
        ParseNeeds {
            component_contracts: true,
            ..NO_FACTS
        },
    ] {
        let second = parse(&files, &store, needs);
        assert_eq!(second.cache_misses, 1, "{needs:?}");
        let module = second
            .modules
            .first()
            .expect("second parse produces module");
        assert_eq!(has_flag_facts(module), needs.flags);
        assert_eq!(has_contracts(module), needs.component_contracts);
    }
}

#[test]
fn a_run_without_the_scans_keeps_the_stored_facts_of_the_entry() {
    let project = tempfile::tempdir().expect("create project");
    let files = [write_source(project.path())];
    let mut store = CacheStore::new(project.path());

    let full = parse(&files, &store, SCANS);
    let module = full.modules.first().expect("full parse produces module");
    record(&mut store, &files[0], module, SCANS);

    let lean = parse(&files, &store, NO_FACTS);
    assert_eq!(lean.cache_hits, 1);
    let lean_module = lean.modules.first().expect("lean parse produces module");
    record(&mut store, &files[0], lean_module, NO_FACTS);

    let again = parse(&files, &store, SCANS);
    assert_eq!(again.cache_hits, 1);
    let module = again.modules.first().expect("hit produces module");
    assert!(has_flag_facts(module) && has_contracts(module));
}
