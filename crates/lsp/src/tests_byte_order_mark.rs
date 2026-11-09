//! A source file that starts with a UTF-8 byte order mark, analyzed by the
//! server and open in an editor buffer that does not hold the mark.

use super::*;

use serde_json::json;
use tower::{Service, ServiceExt};
use tower_lsp_server::jsonrpc::Request;

const LIB_SOURCE: &str = "export const helper = 1;\nexport const used = 2;\n";

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

fn write_project(root: &Path) -> PathBuf {
    let source_dir = root.join("src");
    std::fs::create_dir_all(&source_dir).expect("create source dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"byte-order-mark","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write package");
    std::fs::write(
        source_dir.join("index.ts"),
        "import { used } from \"./lib\";\nconsole.log(used);\n",
    )
    .expect("write entry point");
    let lib = source_dir.join("lib.ts");
    std::fs::write(&lib, format!("\u{feff}{LIB_SOURCE}")).expect("write lib");
    lib
}

#[tokio::test(flavor = "current_thread")]
async fn unused_export_on_the_first_line_of_a_file_with_a_byte_order_mark() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonical root");
    let lib = write_project(&root);
    let lib_uri = Uri::from_file_path(&lib).expect("lib URI");

    let service = analyzed_server(&root).await;
    let backend = service.inner();
    backend.documents.write().await.insert(
        lib_uri.clone(),
        DocumentState::new(1, LIB_SOURCE.to_string()),
    );
    backend.run_analysis().await;

    let diagnostic = backend
        .cached_diagnostics
        .read()
        .await
        .get(&lib_uri)
        .and_then(|diagnostics| {
            diagnostics.iter().find(|diagnostic| {
                diagnostic.code == Some(NumberOrString::String("unused-export".to_string()))
            })
        })
        .cloned()
        .expect("an unused-export diagnostic for the open file");
    assert_eq!(diagnostic.range.start, Position::new(0, 13));
    assert_eq!(diagnostic.range.end, Position::new(0, 19));

    let cursor = Range::new(Position::new(0, 13), Position::new(0, 13));
    let response = backend
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier::new(lib_uri.clone()),
            range: cursor,
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code action handled")
        .unwrap_or_default();
    let edit_range = response
        .iter()
        .find_map(|action| match action {
            CodeActionOrCommand::CodeAction(action)
                if action.title == "Remove unused export `helper`" =>
            {
                action
                    .edit
                    .as_ref()
                    .and_then(|edit| edit.changes.as_ref())
                    .and_then(|changes| changes.get(&lib_uri))
                    .map(|edits| edits[0].range)
            }
            _ => None,
        })
        .expect("a remove-export quick fix for `helper`");
    assert_eq!(
        edit_range,
        Range::new(Position::new(0, 0), Position::new(0, 7))
    );
}
