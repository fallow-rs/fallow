#!/usr/bin/env python3
"""Cross-repo drift guard for the telemetry agent-source allowlist.

The telemetry contract is documented in three repos:

  - fallow      docs/telemetry.md                              (canonical; ships in npm)
  - fallow-docs cli/telemetry.mdx, explanations/telemetry.mdx,
                configuration/environment.mdx                  (hosted)
  - fallow-skills references/cli-reference.md                  (agent guidance)

Within the fallow repo, a Rust test (crates/cli/src/telemetry.rs
`docs_agent_source_allowlist_matches_code`) already pins docs/telemetry.md to the
AgentSource enum, and the fallow-cloud server has its own agreement test against
the same enum. This script closes the remaining gap: it asserts every companion
doc lists the full canonical allowlist, so a value added to the canonical doc
(for example a new agent) cannot silently go missing from a hosted or skills copy
the way `windsurf`/`gemini` aliases once drifted out of the explanation page.

CI checks out the two public companion repositories and runs this script as a
hard gate. Local runs use sibling checkouts by default.

Companion repos are located as siblings of the main checkout by default
(`../fallow-docs`, `../fallow-skills`); override with FALLOW_DOCS_DIR /
FALLOW_SKILLS_DIR. Inside a linked git worktree the main checkout is not this
directory, so the default is resolved against the common git directory's parent.
A companion named through the environment, and any companion checkout that
exists, must hold every expected document: a missing one is a failure because
parity cannot be proven. A guessed companion checkout that is not present at all
stands down with a `skipped:` line, because that is a maintainer who has not
cloned it rather than drift.

`SKILL.md` is intentionally excluded: its agent rule lists a representative
subset ("for example claude_code, codex, ..."), not the full allowlist.

Exit codes: 0 = in sync, 1 = a companion or value is missing, 2 = the canonical
block could not be parsed.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CANONICAL = REPO_ROOT / "docs" / "telemetry.md"


def main_checkout_root() -> Path:
    """The checkout whose siblings the companion repositories are.

    `git rev-parse --git-common-dir` answers `.git` in a primary checkout and the
    main checkout's git directory in a linked worktree, so its parent is the
    right directory in both. GIT_DIR, GIT_WORK_TREE and GIT_INDEX_FILE are
    stripped so an ambient value cannot answer for another repository, and any
    failure falls back to this directory.
    """
    ambient = {"GIT_DIR", "GIT_INDEX_FILE", "GIT_WORK_TREE"}
    env = {k: v for k, v in os.environ.items() if k not in ambient}
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--git-common-dir"],
            capture_output=True,
            check=False,
            cwd=REPO_ROOT,
            env=env,
            text=True,
        )
    except OSError:
        return REPO_ROOT
    common_dir = result.stdout.strip()
    if result.returncode != 0 or not common_dir:
        return REPO_ROOT
    return (REPO_ROOT / common_dir).resolve().parent


def parse_canonical_allowlist(text: str) -> list[str]:
    """Extract the agent-source values from the `## Agent Source` text block."""
    after_heading = text.split("## Agent Source", 1)
    if len(after_heading) < 2:
        return []
    fence = re.search(r"```text\n(.*?)\n```", after_heading[1], re.DOTALL)
    if not fence:
        return []
    return fence.group(1).split()


COMPANIONS = (
    (
        "FALLOW_DOCS_DIR",
        "fallow-docs",
        (
            Path("cli") / "telemetry.mdx",
            Path("explanations") / "telemetry.mdx",
            Path("configuration") / "environment.mdx",
        ),
    ),
    (
        "FALLOW_SKILLS_DIR",
        "fallow-skills",
        (Path("fallow") / "skills" / "fallow" / "references" / "cli-reference.md",),
    ),
)


def companion_checkouts() -> list[tuple[Path, str, bool, list[Path]]]:
    """Each companion checkout, the variable that names it, and its documents."""
    sibling_of = main_checkout_root().parent
    checkouts = []
    for variable, directory, documents in COMPANIONS:
        override = os.environ.get(variable)
        root = Path(override) if override else sibling_of / directory
        checkouts.append((root, variable, bool(override), [root / doc for doc in documents]))
    return checkouts


def missing_values(text: str, allowlist: list[str]) -> list[str]:
    return [v for v in allowlist if not re.search(rf"\b{re.escape(v)}\b", text)]


def main() -> int:
    allowlist = parse_canonical_allowlist(CANONICAL.read_text(encoding="utf-8"))
    if not allowlist:
        print(f"error: could not parse the agent-source allowlist from {CANONICAL}", file=sys.stderr)
        return 2
    print(f"canonical allowlist ({len(allowlist)}): {' '.join(allowlist)}")

    ok = True
    checked = 0
    for root, variable, named, documents in companion_checkouts():
        if not named and not root.is_dir():
            print(f"skipped: no companion checkout at {root}; set {variable} to check parity")
            continue
        for path in documents:
            if not path.is_file():
                ok = False
                print(f"DRIFT: expected companion doc not found: {path}", file=sys.stderr)
                continue
            missing = missing_values(path.read_text(encoding="utf-8"), allowlist)
            if missing:
                ok = False
                print(f"DRIFT: {path} is missing {missing}", file=sys.stderr)
            else:
                checked += 1
                print(f"ok: {path}")

    if not ok:
        print(
            "\nA companion doc is missing or omits a canonical agent-source value. "
            f"Update it to match {CANONICAL.relative_to(REPO_ROOT)}.",
            file=sys.stderr,
        )
        return 1
    if checked == 0:
        print("\nno companion checkout was available, so parity was not checked")
        return 0
    print("\nall companion docs list the full canonical allowlist")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
