use super::*;

fn config_with_every_toggle_on() -> DuplicatesConfig {
    DuplicatesConfig {
        near: true,
        skip_local: true,
        cross_language: true,
        ignore_imports: true,
        ignore_symlinks: true,
        ..DuplicatesConfig::default()
    }
}

/// An explicit `false` turns off a config `true` for each bool key.
#[test]
fn option_wins_for_every_key() {
    let off = DupesInputs {
        near: Some(false),
        skip_local: Some(false),
        cross_language: Some(false),
        ignore_imports: Some(false),
        ignore_symlinks: Some(false),
        ..DupesInputs::default()
    };
    let merged = config_with_every_toggle_on().merged(&off);
    assert!(!merged.near);
    assert!(!merged.skip_local);
    assert!(!merged.cross_language);
    assert!(!merged.ignore_imports);
    assert!(!merged.ignore_symlinks);

    let numbers = DupesInputs {
        mode: Some(DetectionMode::Weak),
        min_tokens: Some(7),
        min_lines: Some(3),
        min_occurrences: Some(4),
        threshold: Some(12.5),
        ..DupesInputs::default()
    };
    let merged = DuplicatesConfig::default().merged(&numbers);
    assert_eq!(merged.mode, DetectionMode::Weak);
    assert_eq!(merged.min_tokens, 7);
    assert_eq!(merged.min_lines, 3);
    assert_eq!(merged.min_occurrences, 4);
    assert!((merged.threshold - 12.5).abs() < f64::EPSILON);
}

#[test]
fn no_option_keeps_the_config() {
    let config = config_with_every_toggle_on();
    let merged = config.merged(&DupesInputs::default());
    assert!(merged.near && merged.skip_local && merged.cross_language);
    assert!(merged.ignore_imports && merged.ignore_symlinks);
    assert_eq!(merged.min_tokens, config.min_tokens);
}

/// `merged` keeps the config switch of the analysis, and
/// `merged_for_selected_run` turns it on.
#[test]
fn only_the_selected_run_turns_the_analysis_on() {
    let off = DuplicatesConfig {
        enabled: false,
        ..DuplicatesConfig::default()
    };
    assert!(!off.merged(&DupesInputs::default()).enabled);
    assert!(off.merged_for_selected_run(&DupesInputs::default()).enabled);
}
