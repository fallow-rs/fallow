//! Mapping of the `fallow ci` subcommands to the CI command runner.

use super::*;

pub(super) fn map_ci_subcommand(sub: CiCli) -> ci::CiCommand {
    match sub {
        command @ CiCli::PlanPrComment { .. } => map_ci_plan_pr_comment(command),
        command @ CiCli::PostPrComment { .. } => map_ci_post_pr_comment(command),
        command @ CiCli::PostReview { .. } => map_ci_post_review(command),
        command @ CiCli::PostCheckRun { .. } => map_ci_post_check_run(command),
        command @ CiCli::ReconcileReview { .. } => map_ci_reconcile_review(command),
    }
}

fn map_ci_plan_pr_comment(command: CiCli) -> ci::CiCommand {
    let CiCli::PlanPrComment {
        body,
        marker_id,
        clean,
        existing_comment_id,
        existing_body,
    } = command
    else {
        unreachable!("ci plan-pr-comment mapper called with different variant");
    };

    ci::CiCommand::PlanPrComment {
        body,
        marker_id,
        clean,
        existing_comment_id,
        existing_body,
    }
}

fn map_ci_post_pr_comment(command: CiCli) -> ci::CiCommand {
    let CiCli::PostPrComment {
        provider,
        pr,
        mr,
        body,
        envelope,
        marker_id,
        clean,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("ci post-pr-comment mapper called with different variant");
    };

    ci::CiCommand::PostPrComment {
        provider: map_ci_provider(provider),
        target: pr.or(mr),
        body,
        envelope,
        marker_id,
        clean,
        repo,
        project_id,
        api_url,
        dry_run,
    }
}

fn map_ci_post_review(command: CiCli) -> ci::CiCommand {
    let CiCli::PostReview {
        provider,
        pr,
        mr,
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("ci post-review mapper called with different variant");
    };

    ci::CiCommand::PostReview {
        provider: map_ci_provider(provider),
        target: pr.or(mr),
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    }
}

fn map_ci_post_check_run(command: CiCli) -> ci::CiCommand {
    let CiCli::PostCheckRun {
        provider,
        decision,
        repo,
        head_sha,
        api_url,
        split_gates,
        dry_run,
    } = command
    else {
        unreachable!("ci post-check-run mapper called with different variant");
    };

    ci::CiCommand::PostCheckRun {
        provider: map_ci_provider(provider),
        decision,
        repo,
        head_sha,
        api_url,
        split_gates,
        dry_run,
    }
}

fn map_ci_reconcile_review(command: CiCli) -> ci::CiCommand {
    let CiCli::ReconcileReview {
        provider,
        pr,
        mr,
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("ci reconcile-review mapper called with different variant");
    };

    ci::CiCommand::ReconcileReview {
        provider: map_ci_provider(provider),
        target: pr.or(mr),
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    }
}

fn map_ci_provider(provider: CiProviderArg) -> ci::CiProvider {
    match provider {
        CiProviderArg::Github => ci::CiProvider::Github,
        CiProviderArg::Gitlab => ci::CiProvider::Gitlab,
    }
}
