//! Tests for the confidence of generic SDK names: the flag imports that
//! confirm a file, and the project functions that are not flag sites.

use super::*;

fn extract_with_sdk_patterns(source: &str, names: &[&str]) -> Vec<FlagUse> {
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, source, SourceType::tsx()).parse();
    let line_offsets = fallow_types::extract::compute_line_offsets(source);
    let patterns = FlagPatterns {
        sdk_patterns: names
            .iter()
            .map(|name| ((*name).to_string(), 0, "InHouse".to_string()))
            .collect(),
        ..FlagPatterns::default()
    };
    extract_flags(&parser_return.program, &line_offsets, &patterns).flag_uses
}

fn unconfirmed(flags: &[FlagUse]) -> Vec<bool> {
    flags
        .iter()
        .map(|flag| flag.facts.unconfirmed_sdk())
        .collect()
}

#[test]
fn generic_sdk_names_without_an_sdk_import_are_unconfirmed() {
    let flags = extract_from_source(
        "client.isEnabled('a');\n\
         getValue('b');\n\
         useFeature('c');",
    );
    assert_eq!(unconfirmed(&flags), vec![true, true, true]);
}

#[test]
fn specific_sdk_names_without_an_import_stay_confirmed() {
    let flags = extract_from_source(
        "useFlag('a');\n\
         checkGate('b');\n\
         getFeatureValue('c');",
    );
    assert_eq!(unconfirmed(&flags), vec![false, false, false]);
}

#[test]
fn a_vendor_sdk_import_confirms_generic_sdk_names() {
    let flags = extract_from_source(
        "import { useUnleashClient } from '@unleash/proxy-client-react';\n\
         const client = useUnleashClient();\n\
         client.isEnabled('a');",
    );
    assert_eq!(unconfirmed(&flags), vec![false]);
}

#[test]
fn a_flag_module_import_confirms_generic_sdk_names() {
    for source in [
        "import { getValue } from './featureFlags';\ngetValue('a');",
        "import flags from '@/lib/flags';\nflags.getValue('a');",
        "import { useFeature } from '../toggles/client';\nuseFeature('a');",
    ] {
        let flags = extract_from_source(source);
        assert_eq!(unconfirmed(&flags), vec![false], "{source}");
    }
}

#[test]
fn a_top_level_require_of_an_sdk_confirms_generic_sdk_names() {
    let flags = extract_from_source(
        "const { initialize } = require('unleash-client');\n\
         const client = initialize({});\n\
         client.isEnabled('a');",
    );
    assert_eq!(unconfirmed(&flags), vec![false]);
}

#[test]
fn an_unrelated_import_does_not_confirm_generic_sdk_names() {
    let flags = extract_from_source(
        "import { form } from './form';\n\
         import type { Flags } from './flags';\n\
         form.getValue('email');",
    );
    assert_eq!(unconfirmed(&flags), vec![true]);
}

#[test]
fn an_import_equals_require_of_an_sdk_confirms_generic_sdk_names() {
    let flags = extract_from_source(
        "import unleash = require('unleash-client');\n\
         const client = unleash.initialize({});\n\
         client.isEnabled('a');",
    );
    assert_eq!(unconfirmed(&flags), vec![false]);
}

#[test]
fn an_import_of_only_inline_types_does_not_confirm_generic_sdk_names() {
    let flags = extract_from_source(
        "import { type Flags, type Keys } from './flags';\n\
         form.getValue('email');",
    );
    assert_eq!(unconfirmed(&flags), vec![true]);
}

#[test]
fn a_side_effect_or_mixed_import_of_a_flag_module_confirms_generic_sdk_names() {
    for source in [
        "import './flags';\ngetValue('a');",
        "import { type Flags, getValue } from './flags';\ngetValue('a');",
    ] {
        let flags = extract_from_source(source);
        assert_eq!(unconfirmed(&flags), vec![false], "{source}");
    }
}

#[test]
fn guarded_and_bound_generic_sites_stay_unconfirmed() {
    let flags = extract_from_source(
        "if (client.isEnabled('a')) { run(); }\n\
         function view() {\n\
           const on = getValue('b');\n\
           if (on) { run(); }\n\
         }",
    );
    assert_eq!(flags.len(), 2);
    assert!(flags.iter().all(|flag| flag.guard_span_start.is_some()));
    assert_eq!(unconfirmed(&flags), vec![true, true]);
}

#[test]
fn a_generic_name_in_sdk_patterns_is_confirmed() {
    let flags = extract_with_sdk_patterns("client.isEnabled('a');", &["isEnabled"]);
    assert_eq!(unconfirmed(&flags), vec![false]);
}

#[test]
fn generic_registry_reads_without_an_sdk_import_are_unconfirmed() {
    let facts = extract_facts(
        "import { FLAGS } from './keys';\n\
         const LOCAL = { A: 'a' } as const;\n\
         isEnabled(LOCAL.A);\n\
         isEnabled(FLAGS.B);",
    );
    assert_eq!(unconfirmed(&facts.flag_uses), vec![true]);
    let reads = &facts.registry_facts.expect("registry facts").reads;
    assert_eq!(reads.len(), 1);
    assert!(reads[0].flag_use.facts.unconfirmed_sdk());
}

#[test]
fn a_lookalike_import_does_not_confirm_generic_sdk_names() {
    for import in [
        "import { hasFlag } from 'country-flag-icons';",
        "import { checkout } from '@/features/checkout/api';",
        "import { ThemeToggle } from './ThemeToggle';",
        "import Flags from 'react-world-flags';",
    ] {
        let source = format!("{import}\nform.getValue('email');");
        let flags = extract_from_source(&source);
        assert_eq!(unconfirmed(&flags), vec![true], "{source}");
    }
}

#[test]
fn known_flag_sdk_and_flag_module_imports_confirm_generic_sdk_names() {
    for module in [
        "@unleash/proxy-client-react",
        "unleash-client",
        "launchdarkly-react-client-sdk",
        "@launchdarkly/node-server-sdk",
        "statsig-react",
        "@statsig/react-bindings",
        "@growthbook/growthbook-react",
        "@splitsoftware/splitio-react",
        "posthog-js/react",
        "configcat-js",
        "@optimizely/react-sdk",
        "@eppo/js-client-sdk",
        "flagsmith/react",
        "@openfeature/web-sdk",
        "flags/next",
        "@vercel/flags",
        "@flags-sdk/launchdarkly",
        "react-feature-toggles",
        "./featureFlags",
        "./feature-flags.ts",
        "@/lib/flags",
        "@/lib/feature-flags/client",
        "../toggles/client",
        "./useFlags",
        "./launchDarklyClient",
    ] {
        let source = format!("import {{ client }} from '{module}';\nclient.isEnabled('a');");
        let flags = extract_from_source(&source);
        assert_eq!(unconfirmed(&flags), vec![false], "{source}");
    }
}

#[test]
fn generic_vendor_names_without_an_sdk_import_are_unconfirmed() {
    let flags = extract_from_source(
        "gb.isOn('a');\n\
         gb.isOff('b');\n\
         useConfig('c');\n\
         client.variation('d', false);\n\
         client.getVariant('e');\n\
         flagsmith.hasFeature('f');",
    );
    assert_eq!(unconfirmed(&flags), vec![true; 6]);
}

#[test]
fn generic_vendor_names_with_an_sdk_import_are_confirmed() {
    let flags = extract_from_source(
        "import { useGrowthBook } from '@growthbook/growthbook-react';\n\
         gb.isOn('a');\n\
         useConfig('c');\n\
         client.variation('d', false);\n\
         client.getVariant('e');\n\
         flagsmith.hasFeature('f');",
    );
    assert_eq!(unconfirmed(&flags), vec![false; 5]);
}

#[test]
fn use_experiment_without_an_import_stays_confirmed() {
    let flags = extract_from_source("useExperiment('checkout');");
    assert_eq!(unconfirmed(&flags), vec![false]);
}

#[test]
fn a_file_local_generic_function_is_not_a_flag_site() {
    for source in [
        "function isEnabled(name: string) { return name.length > 0; }\nisEnabled('a');",
        "export const getValue = (key: string) => key;\ngetValue('b');",
        "import { getVariant } from './product';\ngetVariant('oak-30x40');",
        "import { isOn } from '@/lib/switches';\nisOn('lamp');",
    ] {
        let flags = extract_from_source(source);
        assert!(flags.is_empty(), "{source}: {flags:?}");
    }
}

#[test]
fn a_file_local_generic_function_with_a_flag_import_still_reports() {
    let flags = extract_from_source(
        "import { initialize } from 'unleash-client';\n\
         const client = initialize({});\n\
         function isEnabled(name: string) { return client.isEnabled(name); }\n\
         isEnabled('a');",
    );
    assert_eq!(unconfirmed(&flags), vec![false]);
}

#[test]
fn a_generic_name_from_a_local_flag_module_still_reports() {
    let flags = extract_from_source("import { isEnabled } from './featureFlags';\nisEnabled('a');");
    assert_eq!(unconfirmed(&flags), vec![false]);
}

#[test]
fn a_file_local_name_in_sdk_patterns_still_reports() {
    let flags = extract_with_sdk_patterns(
        "function isEnabled(name: string) { return name.length > 0; }\nisEnabled('a');",
        &["isEnabled"],
    );
    assert_eq!(unconfirmed(&flags), vec![false]);
}

#[test]
fn a_file_local_vendor_specific_name_still_reports() {
    let flags =
        extract_from_source("function useFlag(name: string) { return name; }\nuseFlag('a');");
    assert_eq!(unconfirmed(&flags), vec![false]);
}

#[test]
fn a_generic_name_from_a_loosely_named_local_flag_module_reports() {
    for module in [
        "./flags.server",
        "./flagUtils",
        "./flag-keys",
        "./getFlag",
        "./flags.constants",
        "@/lib/flag-helpers",
    ] {
        let source = format!("import {{ isEnabled }} from '{module}';\nisEnabled('a');");
        let flags = extract_from_source(&source);
        assert_eq!(unconfirmed(&flags), vec![false], "{module}");
    }
}

#[test]
fn a_generic_name_from_a_local_feature_module_reports_unconfirmed() {
    for module in [
        "./featureGate",
        "@/lib/features",
        "./experiments",
        "./CountryFlag",
        "./ThemeToggle",
    ] {
        let source = format!("import {{ isEnabled }} from '{module}';\nisEnabled('a');");
        let flags = extract_from_source(&source);
        assert_eq!(unconfirmed(&flags), vec![true], "{module}");
    }
}
