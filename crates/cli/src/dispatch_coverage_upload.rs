use crate::cli_args::CoverageCli;
use crate::coverage;

pub(super) fn map_coverage_upload_inventory(sub: &CoverageCli) -> coverage::CoverageSubcommand {
    let CoverageCli::UploadInventory {
        api_key,
        api_endpoint,
        project_id,
        git_sha,
        vercel_deployment_id,
        allow_dirty,
        exclude_paths,
        path_prefix,
        dry_run,
        with_callers,
        ignore_upload_errors,
    } = sub
    else {
        unreachable!("coverage inventory mapper called with non-inventory variant");
    };
    coverage::CoverageSubcommand::UploadInventory(coverage::UploadInventoryArgs {
        api_key: api_key.clone(),
        api_endpoint: api_endpoint.clone(),
        project_id: project_id.clone(),
        git_sha: git_sha.clone(),
        vercel_deployment_id: vercel_deployment_id.clone(),
        allow_dirty: *allow_dirty,
        exclude_paths: exclude_paths.clone(),
        path_prefix: path_prefix.clone(),
        dry_run: *dry_run,
        with_callers: *with_callers,
        ignore_upload_errors: *ignore_upload_errors,
    })
}

pub(super) fn map_coverage_upload_source_maps(sub: &CoverageCli) -> coverage::CoverageSubcommand {
    let CoverageCli::UploadSourceMaps {
        dir,
        include,
        exclude,
        repo,
        git_sha,
        vercel_deployment_id,
        endpoint,
        strip_path,
        dry_run,
        concurrency,
        fail_fast,
    } = sub
    else {
        unreachable!("coverage source-map mapper called with non-source-map variant");
    };
    coverage::CoverageSubcommand::UploadSourceMaps(coverage::UploadSourceMapsArgs {
        dir: dir.clone(),
        include: include.clone(),
        exclude: exclude.clone(),
        repo: repo.clone(),
        git_sha: git_sha.clone(),
        vercel_deployment_id: vercel_deployment_id.clone(),
        endpoint: endpoint.clone(),
        strip_path: *strip_path,
        dry_run: *dry_run,
        concurrency: *concurrency,
        fail_fast: *fail_fast,
    })
}
