//! unplugin-icons plugin.
//!
//! unplugin-icons serves each icon as a virtual module, such as
//! `~icons/lucide/x` or `virtual:icons/mdi/arrow-left`. No package has these
//! names, so the plugin marks the prefixes as virtual modules. The analysis
//! credits the icon set packages that the imports name (see
//! `analyze::iconify`).

use super::Plugin;

const ENABLERS: &[&str] = &["unplugin-icons"];

/// The module ID prefixes that the unplugin-icons resolver accepts.
pub const ICON_IMPORT_PREFIXES: &[&str] = &["~icons/", "virtual:icons/", "virtual/icons/"];

define_plugin! {
    struct UnpluginIconsPlugin => "unplugin-icons",
    enablers: ENABLERS,
    virtual_module_prefixes: ICON_IMPORT_PREFIXES,
}
