use super::common::create_config;

fn write(path: std::path::PathBuf, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create dir");
    }
    std::fs::write(path, contents).expect("write file");
}

#[test]
fn vscode_provider_interface_methods_are_framework_used() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        root.join("package.json"),
        r#"{
            "name": "vscode-provider-fixture",
            "private": true,
            "main": "src/extension.ts",
            "devDependencies": {
                "@types/vscode": "1.101.0"
            }
        }"#,
    );
    write(
        root.join("src/extension.ts"),
        r#"
            import * as vscode from "vscode";
            import { SessionDiffViewer } from "./sessionDiffViewer";

            export const activate = (context: vscode.ExtensionContext): void => {
                const viewer = new SessionDiffViewer();
                context.subscriptions.push(
                    vscode.workspace.registerTextDocumentContentProvider("session-diff", viewer),
                );
            };
        "#,
    );
    write(
        root.join("src/sessionDiffViewer.ts"),
        r#"
            import * as vscode from "vscode";

            export class SessionDiffViewer implements vscode.TextDocumentContentProvider {
                provideTextDocumentContent(uri: vscode.Uri): string {
                    return uri.toString();
                }

                unusedHelper(): string {
                    return "unused";
                }
            }
        "#,
    );

    let mut config = create_config(root.to_path_buf());
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect();

    assert!(
        !unused.contains(&"SessionDiffViewer.provideTextDocumentContent".to_string()),
        "VS Code provider method should be credited, found: {unused:?}"
    );
    assert!(
        unused.contains(&"SessionDiffViewer.unusedHelper".to_string()),
        "unrelated helpers must still be reported, found: {unused:?}"
    );
}
