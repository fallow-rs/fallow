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
    let tree = match audit::staged::StagedTree::create(
        dispatch.root,
        crate::runtime_support::ConfigFlags::from_cli(dispatch.cli),
    ) {
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
    let staged_inputs = dispatch.run_inputs.map(|inputs| {
        let mut inputs = inputs.clone();
        inputs.root = Some(fallow_types::sourced::Given {
            value: staged_root.to_path_buf(),
            origin: fallow_types::sourced::Origin::Flag("--staged"),
        });
        inputs
    });
    let staged_dispatch = DispatchContext {
        root: staged_root,
        run_inputs: staged_inputs.as_ref(),
        ..*dispatch
    };
    dispatch_audit(&staged_dispatch, &args)
}

/// Point the caches of a staged audit at the checkout, and lend the parse
/// cache of the checkout to the temporary worktree. Drop of the result writes
/// the parse cache back, after the run and before the worktree is removed.
pub(super) fn lend_parse_cache(
    dispatch: &DispatchContext<'_>,
    args: &AuditDispatchArgs,
    inputs: &mut ResolvedAuditInputs,
) -> Option<audit::worktree_cache::WorktreeParseCache> {
    let host = args.staged_host.as_deref()?;
    // The temporary worktree is removed after the run, so the caches of a
    // staged audit stay in the checkout.
    inputs.cache_dir = fallow_engine::repo_refs::remap_cache_dir_for_base_worktree(
        dispatch.root,
        host,
        &inputs.cache_dir,
    );
    (!dispatch.cli.no_cache).then(|| {
        audit::worktree_cache::WorktreeParseCache::lend(
            host,
            &inputs.cache_dir,
            dispatch.root,
            audit::worktree_cache::WriteBack::Yes,
        )
    })
}

fn move_into(path: &Path, from: &Path, to: &Path) -> PathBuf {
    path.strip_prefix(from)
        .map_or_else(|_| path.to_path_buf(), |relative| to.join(relative))
}
