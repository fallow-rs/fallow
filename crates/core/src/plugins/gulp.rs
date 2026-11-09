//! gulp plugin.
//!
//! The gulp CLI loads the `gulpfile` of the working directory, so no source
//! file imports it. A `gulpfile.js` can also be a directory, and then gulp
//! loads its `index` file. The plugin makes these files entry points, so the
//! task files that they import are reachable.

use super::Plugin;

const ENABLERS: &[&str] = &["gulp", "gulp-cli"];

/// The gulpfile names that gulp finds through its module loaders, plus the
/// `index` file of a gulpfile directory.
const ENTRY_PATTERNS: &[&str] = &[
    "{gulpfile,Gulpfile}.{js,mjs,cjs,ts,mts,cts,babel.js,esm.js}",
    "{gulpfile,Gulpfile}.{js,ts}/index.{js,mjs,cjs,ts,mts,cts}",
];

define_plugin! {
    struct GulpPlugin => "gulp",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
}
