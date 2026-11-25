//! Input files of the `ci` commands.

use crate::common::run_fallow_raw_in_dir;

/// A `--body` link below the working directory that resolves outside it is
/// refused, and its target text does not reach the plan.
#[test]
#[cfg(unix)]
fn plan_pr_comment_refuses_a_body_link_out_of_the_checkout() {
    let dir = tempfile::tempdir().expect("temp dir");
    let checkout = dir.path().join("checkout");
    std::fs::create_dir_all(&checkout).expect("mkdir");
    let outside = dir.path().join("other.md");
    std::fs::write(&outside, "text from outside the checkout").expect("write");
    std::os::unix::fs::symlink(&outside, checkout.join("comment.md")).expect("link");

    let output = run_fallow_raw_in_dir(
        &checkout,
        &[
            "ci",
            "plan-pr-comment",
            "--body",
            "comment.md",
            "--marker-id",
            "input-file-test",
        ],
    );
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(!output.stdout.contains("from outside"), "{}", output.stdout);
    assert!(!output.stderr.contains("from outside"), "{}", output.stderr);
}
