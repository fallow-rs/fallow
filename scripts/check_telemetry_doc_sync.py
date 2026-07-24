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

Companion repos are located as siblings of the fallow repo root by default
(`../fallow-docs`, `../fallow-skills`); override with FALLOW_DOCS_DIR /
FALLOW_SKILLS_DIR. A missing companion repository or expected document is a
failure because parity cannot be proven.

`SKILL.md` is intentionally excluded: its agent rule lists a representative
subset ("for example claude_code, codex, ..."), not the full allowlist.

Exit codes: 0 = in sync, 1 = a companion or value is missing, 2 = the canonical
block could not be parsed.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CANONICAL = REPO_ROOT / "docs" / "telemetry.md"


def parse_canonical_allowlist(text: str) -> list[str]:
    """Extract the agent-source values from the `## Agent Source` text block."""
    after_heading = text.split("## Agent Source", 1)
    if len(after_heading) < 2:
        return []
    fence = re.search(r"```text\n(.*?)\n```", after_heading[1], re.DOTALL)
    if not fence:
        return []
    return fence.group(1).split()


def companion_files() -> list[Path]:
    docs = Path(os.environ.get("FALLOW_DOCS_DIR", REPO_ROOT.parent / "fallow-docs"))
    skills = Path(os.environ.get("FALLOW_SKILLS_DIR", REPO_ROOT.parent / "fallow-skills"))
    files = [
        docs / "cli" / "telemetry.mdx",
        docs / "explanations" / "telemetry.mdx",
        docs / "configuration" / "environment.mdx",
    ]
    skills_ref = skills / "fallow" / "skills" / "fallow" / "references" / "cli-reference.md"
    files.append(skills_ref)
    return files


def missing_values(text: str, allowlist: list[str]) -> list[str]:
    return [v for v in allowlist if not re.search(rf"\b{re.escape(v)}\b", text)]


def main() -> int:
    allowlist = parse_canonical_allowlist(CANONICAL.read_text(encoding="utf-8"))
    if not allowlist:
        print(f"error: could not parse the agent-source allowlist from {CANONICAL}", file=sys.stderr)
        return 2
    print(f"canonical allowlist ({len(allowlist)}): {' '.join(allowlist)}")

    ok = True
    for path in companion_files():
        if not path.is_file():
            ok = False
            print(f"DRIFT: expected companion doc not found: {path}", file=sys.stderr)
            continue
        missing = missing_values(path.read_text(encoding="utf-8"), allowlist)
        if missing:
            ok = False
            print(f"DRIFT: {path} is missing {missing}", file=sys.stderr)
        else:
            print(f"ok: {path}")

    if not ok:
        print(
            "\nA companion doc is missing or omits a canonical agent-source value. "
            f"Update it to match {CANONICAL.relative_to(REPO_ROOT)}.",
            file=sys.stderr,
        )
        return 1
    print("\nall companion docs list the full canonical allowlist")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
