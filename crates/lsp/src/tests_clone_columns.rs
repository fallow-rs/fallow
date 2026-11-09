//! Clone diagnostics on a line with non-ASCII text before the clone. The
//! duplication columns are byte columns and the LSP converts them to UTF-16
//! units (issue #4378).

use super::*;

use serde_json::json;
use tower::{Service, ServiceExt};
use tower_lsp_server::jsonrpc::Request;

const PREFIX: &str = "/* é \u{1F600} */ ";
const BODY: &str = "export function shared(value: number): number {\n  const doubled = value * 2;\n  const shifted = doubled + 3;\n  const tripled = shifted * 3;\n  return tripled * 4;\n}\n";

async fn analyzed_server(root: &Path) -> LspService<FallowLspServer> {
    let (mut service, _) = LspService::build(FallowLspServer::new).finish();
    let initialize = Request::build("initialize")
        .params(json!({
            "capabilities": {},
            "rootUri": Uri::from_file_path(root).expect("root URI"),
        }))
        .id(1)
        .finish();
    let response = service
        .ready()
        .await
        .expect("service ready")
        .call(initialize)
        .await
        .expect("initialize handled")
        .expect("initialize response");
    assert!(response.is_ok(), "initialize must succeed");
    service
}

fn write_project(root: &Path) -> (PathBuf, PathBuf) {
    let source_dir = root.join("src");
    std::fs::create_dir_all(&source_dir).expect("create source dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"clone-columns","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write package");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"duplicates":{"minTokens":5,"minLines":2}}"#,
    )
    .expect("write config");
    std::fs::write(
        source_dir.join("index.ts"),
        "import { shared } from \"./a\";\nimport { shared as other } from \"./b\";\nconsole.log(shared(1), other(2));\n",
    )
    .expect("write entry point");
    let a = source_dir.join("a.ts");
    std::fs::write(&a, format!("{PREFIX}{BODY}")).expect("write a");
    let b = source_dir.join("b.ts");
    std::fs::write(&b, BODY).expect("write b");
    (a, b)
}

#[tokio::test(flavor = "current_thread")]
async fn clone_diagnostic_starts_at_the_clone_after_non_ascii_text() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonical root");
    let (a, b) = write_project(&root);
    let a_uri = Uri::from_file_path(&a).expect("a URI");
    let b_uri = Uri::from_file_path(&b).expect("b URI");
    let expected_character = u32::try_from(PREFIX.encode_utf16().count()).expect("small prefix");

    let service = analyzed_server(&root).await;
    let backend = service.inner();
    backend.run_analysis().await;

    let diagnostics = backend.cached_diagnostics.read().await;
    let clone_diagnostic = |uri: &Uri| -> Diagnostic {
        diagnostics
            .get(uri)
            .and_then(|diagnostics| {
                diagnostics.iter().find(|diagnostic| {
                    diagnostic.code == Some(NumberOrString::String("code-duplication".to_string()))
                })
            })
            .cloned()
            .expect("a code-duplication diagnostic")
    };

    let in_a = clone_diagnostic(&a_uri);
    assert_eq!(in_a.range.start, Position::new(0, expected_character));

    let in_b = clone_diagnostic(&b_uri);
    let related = in_b
        .related_information
        .as_ref()
        .and_then(|related| related.iter().find(|info| info.location.uri == a_uri))
        .expect("related information for the clone in a.ts");
    assert_eq!(
        related.location.range.start,
        Position::new(0, expected_character)
    );
}
