//! VS Code extension plugin.
//!
//! Activates on the `vscode` package and credits provider interface methods
//! that VS Code invokes through registration APIs.

use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};

use super::Plugin;

const ENABLERS: &[&str] = &["vscode", "@types/vscode", "@vscode/test-electron"];

const TOOLING_DEPENDENCIES: &[&str] = &["@types/vscode", "@vscode/test-electron"];

const PROVIDER_RULES: &[(&str, &[&str])] = &[
    (
        "TextDocumentContentProvider",
        &["provideTextDocumentContent"],
    ),
    (
        "TreeDataProvider",
        &["getTreeItem", "getChildren", "getParent"],
    ),
    ("WebviewViewProvider", &["resolveWebviewView"]),
    ("FileDecorationProvider", &["provideFileDecoration"]),
    (
        "CodeLensProvider",
        &["provideCodeLenses", "resolveCodeLens"],
    ),
    (
        "CompletionItemProvider",
        &["provideCompletionItems", "resolveCompletionItem"],
    ),
    ("DefinitionProvider", &["provideDefinition"]),
    ("DocumentSymbolProvider", &["provideDocumentSymbols"]),
    ("HoverProvider", &["provideHover"]),
    ("ReferenceProvider", &["provideReferences"]),
    ("RenameProvider", &["provideRenameEdits", "prepareRename"]),
    (
        "DocumentFormattingEditProvider",
        &["provideDocumentFormattingEdits"],
    ),
    (
        "DocumentRangeFormattingEditProvider",
        &["provideDocumentRangeFormattingEdits"],
    ),
    (
        "DocumentLinkProvider",
        &["provideDocumentLinks", "resolveDocumentLink"],
    ),
    (
        "InlayHintsProvider",
        &["provideInlayHints", "resolveInlayHint"],
    ),
];

fn implements_rule(interface_name: &str, members: &[&str]) -> UsedClassMemberRule {
    UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: None,
        implements: Some(interface_name.to_string()),
        members: members.iter().map(|member| (*member).to_string()).collect(),
    })
}

pub struct VscodePlugin;

impl Plugin for VscodePlugin {
    fn name(&self) -> &'static str {
        "vscode"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        PROVIDER_RULES
            .iter()
            .flat_map(|(interface, members)| {
                [
                    implements_rule(interface, members),
                    implements_rule(&format!("vscode.{interface}"), members),
                ]
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enablers_cover_vscode_package() {
        let enablers = VscodePlugin.enablers();
        assert!(enablers.contains(&"vscode"));
        assert!(enablers.contains(&"@types/vscode"));
    }

    #[test]
    fn provider_rules_are_scoped_to_interfaces() {
        let rules = VscodePlugin.used_class_member_rules();

        assert!(rules.iter().any(|rule| {
            matches!(
                rule,
                UsedClassMemberRule::Scoped(rule)
                    if rule.implements.as_deref() == Some("vscode.TextDocumentContentProvider")
                        && rule.members.contains(&"provideTextDocumentContent".to_string())
            )
        }));
        assert!(
            rules
                .iter()
                .all(|rule| matches!(rule, UsedClassMemberRule::Scoped(_))),
            "VS Code provider methods must stay heritage-scoped"
        );
    }
}
