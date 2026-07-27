//! Shared client for Fallow's optional TypeScript semantic companion.
//!
//! This module is the single Rust owner of semantic requests, response
//! validation, reconciliation, binary discovery, and subprocess lifecycle.
//! Protocol adapters may map options and render results, but must not duplicate
//! these responsibilities.

mod client;
mod manifest;
mod transport;

use fallow_engine::session::AnalysisSession;
use fallow_types::envelope::TypeAwareMeta;
use fallow_types::results::AnalysisResults;
use fallow_types::semantic::SemanticCompleteness;

use crate::{DeadCodeFilters, ProgrammaticError, TypeAwareOptions};

pub use client::{
    SemanticCouplingOutcome, SemanticDeadCodeOutcome, SemanticInspectOutcome,
    discard_unverified_semantic_candidates, inspect_symbol, merge_type_aware_meta,
    refine_dead_code_results, refine_dead_code_results_in_session, symbol_impact, trace_symbol,
    type_coupling,
};
pub use transport::{
    TypeAwareError, TypeAwareFileChanges, TypeAwareOutcome, TypeAwareSession, TypeAwareStatus,
    shutdown_type_aware_sidecars, status, terminate_active_type_aware_sidecars,
};

/// Refine a programmatic dead-code result through the shared semantic client.
///
/// The shared client always produces a conservative typed result. This
/// compatibility adapter stages that result on a clone so complete-mode errors
/// preserve the caller's pre-existing mutation contract. CLI adapters call the
/// shared result function directly, render the conservative result, and apply
/// their exit policy afterwards.
pub fn refine_programmatic_dead_code(
    options: &TypeAwareOptions,
    filters: &DeadCodeFilters,
    session: &AnalysisSession,
    results: &mut AnalysisResults,
) -> Result<Option<TypeAwareMeta>, ProgrammaticError> {
    if !options.enabled {
        return Ok(None);
    }

    let include_all = !filters.any_active();
    let include_symbol_use = include_all
        || filters.unused_exports
        || filters.unused_types
        || filters.unused_class_members;
    let include_api_surface = include_all || filters.private_type_leaks;
    let entry_points = fallow_engine::list_inventory::collect_entry_points(
        session.config(),
        session.files(),
        session.workspaces(),
        None,
    )
    .into_iter()
    .map(|entry| entry.path)
    .collect::<Vec<_>>();

    if options.require != fallow_config::TypeAwareRequire::Complete {
        let outcome = refine_configured_dead_code_results(
            session.config(),
            results,
            &options.projects,
            &entry_points,
            include_symbol_use,
            include_api_surface,
            false,
        )
        .map_err(|error| programmatic_semantic_error(&error))?;
        return Ok(outcome.map(|outcome| outcome.type_aware.meta));
    }

    let mut refined_results = results.clone();
    let outcome = match refine_configured_dead_code_results(
        session.config(),
        &mut refined_results,
        &options.projects,
        &entry_points,
        include_symbol_use,
        include_api_surface,
        false,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            client::discard_unverified_semantic_candidates(results);
            return Err(programmatic_semantic_error(&error));
        }
    };
    let Some(outcome) = outcome else {
        client::discard_unverified_semantic_candidates(results);
        return Ok(None);
    };
    let meta = outcome.type_aware.meta;
    if meta
        .identity
        .as_ref()
        .is_some_and(|identity| identity.completeness != SemanticCompleteness::Complete)
    {
        client::discard_unverified_semantic_candidates(results);
        return Err(programmatic_semantic_error(&TypeAwareError::from(
            "type-aware completeness is required, but the semantic result was incomplete"
                .to_string(),
        )));
    }
    *results = refined_results;
    Ok(Some(meta))
}

/// Refine a programmatic result through an explicitly owned semantic session.
pub fn refine_programmatic_dead_code_in_session(
    semantic_session: &mut TypeAwareSession,
    changes: Option<&TypeAwareFileChanges>,
    options: &TypeAwareOptions,
    filters: &DeadCodeFilters,
    session: &AnalysisSession,
    results: &mut AnalysisResults,
) -> Result<Option<TypeAwareMeta>, ProgrammaticError> {
    if !options.enabled {
        return Ok(None);
    }
    let include_all = !filters.any_active();
    let include_symbol_use = include_all
        || filters.unused_exports
        || filters.unused_types
        || filters.unused_class_members;
    let include_api_surface = include_all || filters.private_type_leaks;
    let entry_points = fallow_engine::list_inventory::collect_entry_points(
        session.config(),
        session.files(),
        session.workspaces(),
        None,
    )
    .into_iter()
    .map(|entry| entry.path)
    .collect::<Vec<_>>();
    let mut refined_results = results.clone();
    let outcome = match refine_configured_dead_code_results_in_session(
        semantic_session,
        changes,
        session.config(),
        &mut refined_results,
        &options.projects,
        &entry_points,
        include_symbol_use,
        include_api_surface,
        false,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            client::discard_unverified_semantic_candidates(results);
            return Err(programmatic_semantic_error(&error));
        }
    };
    let Some(outcome) = outcome else {
        client::discard_unverified_semantic_candidates(results);
        return Ok(None);
    };
    let meta = outcome.type_aware.meta;
    if options.require == fallow_config::TypeAwareRequire::Complete
        && meta
            .identity
            .as_ref()
            .is_some_and(|identity| identity.completeness != SemanticCompleteness::Complete)
    {
        client::discard_unverified_semantic_candidates(results);
        return Err(programmatic_semantic_error(&TypeAwareError::from(
            "type-aware completeness is required, but the semantic result was incomplete"
                .to_string(),
        )));
    }
    *results = refined_results;
    Ok(Some(meta))
}

/// Refine dead-code results and reapply project finding exclusions after the
/// semantic stage has finished adding or updating findings.
#[expect(
    clippy::too_many_arguments,
    reason = "semantic refinement keeps requested capability families explicit"
)]
pub fn refine_configured_dead_code_results(
    config: &fallow_config::ResolvedConfig,
    results: &mut AnalysisResults,
    projects: &[std::path::PathBuf],
    entry_points: &[std::path::PathBuf],
    include_symbol_use: bool,
    include_private_type_leaks: bool,
    include_type_coupling: bool,
) -> Result<Option<SemanticDeadCodeOutcome>, TypeAwareError> {
    let outcome = refine_dead_code_results(
        &config.root,
        results,
        projects,
        entry_points,
        include_symbol_use,
        include_private_type_leaks,
        include_type_coupling,
    )?;
    fallow_engine::dead_code::filter_configured_ignored_findings(results, config);
    Ok(outcome)
}

/// Refine dead-code results through a persistent semantic session and reapply
/// project finding exclusions after reconciliation.
#[expect(
    clippy::too_many_arguments,
    reason = "semantic refinement keeps requested capability families explicit"
)]
pub fn refine_configured_dead_code_results_in_session(
    semantic_session: &mut TypeAwareSession,
    changes: Option<&TypeAwareFileChanges>,
    config: &fallow_config::ResolvedConfig,
    results: &mut AnalysisResults,
    projects: &[std::path::PathBuf],
    entry_points: &[std::path::PathBuf],
    include_symbol_use: bool,
    include_private_type_leaks: bool,
    include_type_coupling: bool,
) -> Result<Option<SemanticDeadCodeOutcome>, TypeAwareError> {
    let outcome = refine_dead_code_results_in_session(
        semantic_session,
        changes,
        &config.root,
        results,
        projects,
        entry_points,
        include_symbol_use,
        include_private_type_leaks,
        include_type_coupling,
    )?;
    fallow_engine::dead_code::filter_configured_ignored_findings(results, config);
    Ok(outcome)
}

fn programmatic_semantic_error(error: &TypeAwareError) -> ProgrammaticError {
    ProgrammaticError::new(error.to_string(), 2)
        .with_code("FALLOW_TYPE_AWARE_FAILED")
        .with_context("analysis.typeAware")
}
