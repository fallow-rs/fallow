use ls_types::{
    ClientCapabilities, CodeActionKind, CodeActionOptions, CodeActionProviderCapability,
    CodeLensOptions, DiagnosticOptions, DiagnosticServerCapabilities, HoverProviderCapability,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, WorkDoneProgressOptions,
};

pub fn build_server_capabilities(advertise_pull_diagnostics: bool) -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            ..Default::default()
        })),
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(false),
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        diagnostic_provider: advertise_pull_diagnostics.then(|| {
            DiagnosticServerCapabilities::Options(DiagnosticOptions {
                identifier: Some("fallow".to_string()),
                inter_file_dependencies: true,
                workspace_diagnostics: false,
                work_done_progress_options: WorkDoneProgressOptions::default(),
            })
        }),
        ..Default::default()
    }
}

pub fn client_supports_workspace_diagnostic_refresh(capabilities: &ClientCapabilities) -> bool {
    capabilities
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.diagnostics.as_ref())
        .and_then(|diagnostics| diagnostics.refresh_support)
        .unwrap_or(false)
}
