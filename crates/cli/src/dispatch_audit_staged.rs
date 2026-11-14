//! Dispatch of `fallow audit --staged`.

use super::*;

/// Audit the staged index. The audit runs in a temporary worktree that holds
/// the staged files, with HEAD as the base. The worktree is removed when the
/// run ends.
pub(super) fn dispatch_staged_audit(
    dispatch: &DispatchContext<'_>,
    mut args: AuditDispatchArgs,
) -> ExitCode {
    let output = dispatch.output;
    if dispatch.cli.changed_since.is_some() {
        return emit_input_error(
            "--staged compares the staged index with HEAD and does not take --base or --changed-since",
            output,
        );
    }
    let tree = match audit::staged::StagedTree::create(dispatch.root) {
        Ok(tree) => tree,
        Err(message) => return emit_input_error(&message, output),
    };
    let host = dispatch.root;
    let staged_root = tree.root();
    // Paths that the user gave are paths of the checkout. A scope moves into
    // the temporary worktree. A coverage file is usually not in the index, so
    // it stays in the checkout.
    args.scope = args
        .scope
        .take()
        .map(|scope| move_into(&scope, host, staged_root));
    args.coverage = args.coverage.take().map(|path| host.join(path));
    args.runtime_coverage = args.runtime_coverage.take().map(|path| host.join(path));
    args.staged_host = Some(host.to_path_buf());
    let staged_dispatch = DispatchContext {
        root: staged_root,
        ..*dispatch
    };
    dispatch_audit(&staged_dispatch, &args)
}

fn move_into(path: &Path, from: &Path, to: &Path) -> PathBuf {
    path.strip_prefix(from)
        .map_or_else(|_| path.to_path_buf(), |relative| to.join(relative))
}
