//! Built-in rule-pack templates for `fallow rule-pack init`.

pub struct Template {
    pub name: &'static str,
    pub body: &'static str,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        name: "starter",
        body: include_str!("../../data/rule-pack-templates/starter.jsonc"),
    },
    Template {
        name: "ai-safe-repo",
        body: include_str!("../../data/rule-pack-templates/ai-safe-repo.jsonc"),
    },
    Template {
        name: "side-effect-free-domain",
        body: include_str!("../../data/rule-pack-templates/side-effect-free-domain.jsonc"),
    },
    Template {
        name: "clean-architecture",
        body: include_str!("../../data/rule-pack-templates/clean-architecture.jsonc"),
    },
    Template {
        name: "next-app-router",
        body: include_str!("../../data/rule-pack-templates/next-app-router.jsonc"),
    },
];

pub fn by_name(name: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|template| template.name == name)
}

pub fn render(template: &Template, pack_name: &str) -> String {
    template.body.replace("__PACK_NAME__", pack_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_loads_through_the_real_pack_loader() {
        for template in TEMPLATES {
            let dir = tempfile::tempdir().expect("create temp dir");
            let rel = format!("{}.jsonc", template.name);
            std::fs::write(dir.path().join(&rel), render(template, template.name))
                .expect("write template");
            let loaded = fallow_config::load_rule_packs(dir.path(), std::slice::from_ref(&rel))
                .unwrap_or_else(|errs| panic!("template {} invalid: {errs:?}", template.name));

            assert_eq!(loaded.len(), 1, "template {}", template.name);
            assert!(!loaded[0].rules.is_empty(), "template {}", template.name);
        }
    }
}
