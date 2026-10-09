//! The `jsx_runtime_from_config` flag: a file with JSX and no JSX runtime
//! pragma takes the runtime source from a bundler or test config.

use crate::tests::{parse_ts, parse_tsx};

#[test]
fn jsx_without_pragma_takes_the_runtime_from_config() {
    assert!(parse_tsx("export const A = () => <div />;\n").jsx_runtime_from_config);
    assert!(parse_tsx("export const A = () => <></>;\n").jsx_runtime_from_config);
    assert!(
        parse_tsx("/** @jsxRuntime automatic */\nexport const A = () => <div />;\n")
            .jsx_runtime_from_config
    );
}

#[test]
fn pragma_or_missing_jsx_clears_the_flag() {
    for source in [
        "/** @jsxImportSource preact */\nexport const A = () => <div />;\n",
        "/** @jsxRuntime classic */\nexport const A = () => <div />;\n",
        "export const a = 1;\n",
    ] {
        assert!(
            !parse_tsx(source).jsx_runtime_from_config,
            "flag must be false for: {source}"
        );
    }
    assert!(!parse_ts("export const a = 1;\n").jsx_runtime_from_config);
}
