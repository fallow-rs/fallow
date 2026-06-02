//! Ionic Angular plugin.
//!
//! Activates on `@ionic/angular`, keeps Ionic CLI config reachable, and credits
//! page lifecycle methods that Ionic invokes through its Angular router outlet.

use super::Plugin;

const ENABLERS: &[&str] = &["@ionic/angular"];

const CONFIG_PATTERNS: &[&str] = &["ionic.config.json"];

const ALWAYS_USED: &[&str] = &["ionic.config.json"];

const TOOLING_DEPENDENCIES: &[&str] = &["@ionic/cli", "@ionic/angular-toolkit", "ionicons"];

const IONIC_LIFECYCLE_MEMBERS: &[&str] = &[
    "ionViewWillEnter",
    "ionViewDidEnter",
    "ionViewWillLeave",
    "ionViewDidLeave",
];

pub struct IonicPlugin;

impl Plugin for IonicPlugin {
    fn name(&self) -> &'static str {
        "ionic"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn used_class_members(&self) -> &'static [&'static str] {
        IONIC_LIFECYCLE_MEMBERS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enablers_cover_ionic_angular() {
        let plugin = IonicPlugin;
        assert!(plugin.enablers().contains(&"@ionic/angular"));
    }

    #[test]
    fn config_file_is_always_used() {
        let plugin = IonicPlugin;
        assert!(plugin.config_patterns().contains(&"ionic.config.json"));
        assert!(plugin.always_used().contains(&"ionic.config.json"));
    }

    #[test]
    fn tooling_dependencies_cover_ionic_cli_packages() {
        let plugin = IonicPlugin;
        let deps = plugin.tooling_dependencies();
        assert!(deps.contains(&"@ionic/cli"));
        assert!(deps.contains(&"@ionic/angular-toolkit"));
        assert!(deps.contains(&"ionicons"));
    }

    #[test]
    fn lifecycle_members_cover_ionic_angular_page_hooks() {
        let members = IonicPlugin.used_class_members();
        assert!(members.contains(&"ionViewWillEnter"));
        assert!(members.contains(&"ionViewDidEnter"));
        assert!(members.contains(&"ionViewWillLeave"));
        assert!(members.contains(&"ionViewDidLeave"));
    }

    #[test]
    fn lifecycle_members_do_not_use_broad_patterns() {
        for member in IonicPlugin.used_class_members() {
            assert!(
                !member.contains('*') && !member.contains('?'),
                "Ionic lifecycle member must be an exact method name: {member}"
            );
        }
    }
}
