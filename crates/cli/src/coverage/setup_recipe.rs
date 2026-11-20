//! Step 3 of `fallow coverage setup`: write `docs/collect-coverage.md`
//! without overwriting a recipe that the user wrote or edited.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::{
    CoverageSetupContext, CoverageSetupState, INVALID_INPUT, display_relative, hash_bytes,
    hash_file, recipe_contents, recipe_path, recipe_state_is_current, record_recipe_state,
    refuse_outside_link, save_setup_state, setup_error_exit, write_recipe,
};

/// The recipe file that setup points the user to, and what setup did with it.
#[derive(Debug)]
pub(super) struct RecipeOutcome {
    pub(super) path: PathBuf,
    pub(super) status: RecipeStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecipeStatus {
    /// Setup wrote the recipe in this run.
    Written,
    /// The file already held the expected recipe, so setup wrote nothing.
    Present,
    /// The file differs from the expected recipe and setup did not overwrite it.
    Kept,
}

/// What setup does with an existing `docs/collect-coverage.md`.
enum RecipeAction {
    /// No file exists, or the file still holds the recipe that setup wrote.
    Write,
    /// The file already holds the expected recipe.
    Adopt,
    /// Somebody wrote or edited the file. Setup must not overwrite it.
    Keep,
}

/// Setup replaces a recipe only while its bytes match the checksum that setup
/// recorded when it wrote it. Any other existing file belongs to the user. The
/// checksum alone decides, so a moved project or another spelling of the root
/// does not turn an untouched recipe into a foreign one.
fn recipe_action(state: &CoverageSetupState, path: &Path, expected_contents: &str) -> RecipeAction {
    if matches!(std::fs::symlink_metadata(path), Err(err) if err.kind() == std::io::ErrorKind::NotFound)
    {
        return RecipeAction::Write;
    }
    let Some(current) = hash_file(path) else {
        return RecipeAction::Keep;
    };
    if current == hash_bytes(expected_contents.as_bytes()) {
        return RecipeAction::Adopt;
    }
    let written_by_setup = state
        .recipe
        .as_ref()
        .is_some_and(|recipe| recipe.checksum == current);
    if written_by_setup {
        RecipeAction::Write
    } else {
        RecipeAction::Keep
    }
}

pub(super) fn run_setup_recipe_step(
    root: &Path,
    context: &CoverageSetupContext,
    setup_state: &mut CoverageSetupState,
) -> Result<RecipeOutcome, ExitCode> {
    let recipe = recipe_contents(context);
    if recipe_state_is_current(setup_state, root, context, &recipe) {
        return Ok(RecipeOutcome {
            path: resumed_recipe_path(root, setup_state),
            status: RecipeStatus::Present,
        });
    }

    let path = recipe_path(root);
    refuse_outside_link(root, &path)
        .map_err(|message| setup_error_exit(&message, INVALID_INPUT))?;
    let status = match recipe_action(setup_state, &path, &recipe) {
        RecipeAction::Keep => {
            println!("{}", kept_step_line(root, &path, context.framework.label()));
            return Ok(RecipeOutcome {
                path,
                status: RecipeStatus::Kept,
            });
        }
        RecipeAction::Adopt => RecipeStatus::Present,
        RecipeAction::Write => {
            write_recipe(root, &recipe)
                .map_err(|message| setup_error_exit(&message, INVALID_INPUT))?;
            RecipeStatus::Written
        }
    };
    record_recipe_state(setup_state, path.clone(), context, &recipe);
    save_setup_state(root, setup_state)
        .map_err(|message| setup_error_exit(&message, INVALID_INPUT))?;
    Ok(RecipeOutcome { path, status })
}

/// Step 3 for a kept file. A kept file can be an authored recipe, a hand
/// edit, or a recipe that an earlier setup wrote for another framework before
/// its setup state was lost, so the line names only the fact that setup checks.
pub(super) fn kept_step_line(root: &Path, path: &Path, framework: &str) -> String {
    format!(
        "Step 3/4: Coverage recipe... kept {} (it differs from the {framework} recipe).",
        display_relative(root, path)
    )
}

/// The recipe line of the final step.
pub(super) fn final_recipe_line(root: &Path, recipe: &RecipeOutcome, framework: &str) -> String {
    let path = display_relative(root, &recipe.path);
    match recipe.status {
        RecipeStatus::Written => format!("  -> Wrote {path} with the {framework} recipe."),
        RecipeStatus::Present => format!("  -> {path} has the {framework} recipe."),
        RecipeStatus::Kept => {
            format!("  -> Move {path} away and re-run to get the {framework} recipe.")
        }
    }
}

fn resumed_recipe_path(root: &Path, setup_state: &CoverageSetupState) -> PathBuf {
    let path = setup_state
        .recipe
        .as_ref()
        .map_or_else(|| recipe_path(root), |recipe| recipe.path.clone());
    println!(
        "Step 3/4: Coverage recipe... ok ({}) (resumed).",
        display_relative(root, &path)
    );
    path
}
