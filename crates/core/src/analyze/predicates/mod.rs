mod file;
mod import;
mod lifecycle;

pub(super) use file::{
    is_barrel_with_reachable_sources, is_config_file, is_declaration_file, is_html_file,
    is_test_or_spec_file,
};
pub use import::{is_builtin_module, is_virtual_module, project_uses_rsc_directives};
pub(super) use import::{is_implicit_dependency, is_path_alias};
pub(super) use lifecycle::{is_angular_lifecycle_method, is_react_lifecycle_method};
