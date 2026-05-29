#!/usr/bin/env python3
"""Scan for hidden/bidi unicode and (agent mode) shell-exec injection shapes.

Two defense-in-depth guards share this scanner (see SECURITY.md, "Agent-instruction
surface"):

  --mode committed   The COMMITTED text surface. Codepoint hits BLOCK (wired into
                     .githooks/pre-commit on staged files, and a CI step over the
                     tracked surface). Keeps zero-width / bidi characters out of
                     source. No keyword scan here (too FP-prone for Rust/CI code).

  --mode agent       The local agent-instruction allowlist, INCLUDING untracked /
                     gitignored files (AGENTS.md, .codex/**, .claude/**,
                     .cursorrules, .cursor/**, *.mcp.json). This is the gap repo
                     review cannot see: those files never reach a PR. Run from a
                     Claude Code SessionStart hook. Codepoint hits are reported as
                     errors (exit 1); keyword-soup shapes (curl|sh, base64 -d, eval,
                     node -e) WARN only, because they are trivially bypassed by plain
                     ASCII so blocking on them is theater; and a sha256 manifest
                     drift check WARNS when a tracked agent file changed since it was
                     last blessed.

  --mode check-manifest
                     The CI drift gate. BLOCKS (exit 1) when any tracked agent file
                     differs from its blessed sha256 in scripts/agent-files.sha256,
                     or a blessed file is missing. This is the enforcing counterpart
                     to agent mode's warn-only drift check: it makes a PR that edits a
                     tracked agent file re-bless the manifest in the same change,
                     rather than leaving stale drift that warns on every later session.
                     No codepoint or keyword scan here. Wired into ci.yml.

  --update-manifest  Regenerate scripts/agent-files.sha256 (the blessed baseline for
                     the drift checks). Run after a legitimate edit to a tracked agent
                     file, then commit the manifest.

Exit code is nonzero on a codepoint hit (committed / agent modes) or on manifest
drift (check-manifest mode). In agent mode the manifest-drift and keyword-soup
findings stay warn-only (exit 0) so a session is never hard-blocked by a heuristic
or an in-progress edit. Stdlib only.
"""

from __future__ import annotations

import hashlib
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = REPO_ROOT / "scripts" / "agent-files.sha256"

# Zero-width and bidirectional-override code points that have no legitimate place
# in source or agent-instruction text. A leading U+FEFF (BOM) is tolerated; a
# mid-file one is not. U+200D (zero-width joiner) is handled separately because it
# is load-bearing inside emoji sequences (family/profession emoji).
BLOCK_CODEPOINTS = {
    0x200B,  # zero-width space
    0x200C,  # zero-width non-joiner
    0x2060,  # word joiner
    0x202A,  # left-to-right embedding
    0x202B,  # right-to-left embedding
    0x202C,  # pop directional formatting
    0x202D,  # left-to-right override
    0x202E,  # right-to-left override
    0x2066,  # left-to-right isolate
    0x2067,  # right-to-left isolate
    0x2068,  # first strong isolate
    0x2069,  # pop directional isolate
}
ZWJ = 0x200D
BOM = 0xFEFF

# Text file extensions scanned in committed mode.
COMMITTED_EXTS = {
    ".md", ".mdx", ".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs",
    ".json", ".jsonc", ".yml", ".yaml", ".toml",
}

# Agent-instruction file allowlist for agent mode. These are the poisoning targets
# that never go through PR review (most are gitignored). Globs are repo-root-relative.
AGENT_GLOBS = [
    "AGENTS.md",
    "CLAUDE.md",
    ".cursorrules",
    ".claude/**/*",
    ".codex/**/*",
    ".cursor/**/*",
    "**/*.mcp.json",
]
# Never walk these even when reachable via a glob; pure noise / huge.
AGENT_SKIP_DIRS = {"node_modules", "target", ".git", "dist", ".next"}

# Keyword-soup shapes (agent mode, WARN-only). Each is (label, compiled regex).
KEYWORD_PATTERNS = [
    ("network piped to shell", re.compile(r"(curl|wget|Invoke-WebRequest|iwr)\b[^\n]*\|\s*(ba)?sh\b", re.IGNORECASE)),
    ("base64-decode piped to shell", re.compile(r"base64\s+(-d|-D|--decode)\b[^\n]*\|\s*(ba)?sh\b", re.IGNORECASE)),
    ("inline eval", re.compile(r"\beval\s*[\(\"']")),
    ("inline code execution flag", re.compile(r"\b(node|deno|bun)\s+-e\b|\bpython3?\s+-c\b|\b(ruby|perl)\s+-e\b")),
    ("credential term near outbound call", re.compile(r"(AWS_SECRET|API[_-]?KEY|PRIVATE[_ ]KEY|SECRET[_ ]?KEY|seed phrase|mnemonic|wallet)[^\n]*(curl|wget|fetch|https?://)", re.IGNORECASE)),
]


def is_emoji_ish(cp: int) -> bool:
    """Code points that can legitimately neighbor a ZWJ inside an emoji sequence."""
    return (
        0x1F000 <= cp <= 0x1FAFF  # supplemental symbols, emoji, pictographs
        or 0x2600 <= cp <= 0x27BF  # misc symbols + dingbats
        or 0x2300 <= cp <= 0x23FF  # misc technical (some emoji)
        or cp == 0xFE0F  # variation selector-16 (emoji presentation)
        or 0x1F3FB <= cp <= 0x1F3FF  # skin-tone modifiers
        or 0x1F1E6 <= cp <= 0x1F1FF  # regional indicators
        or cp in (0x2640, 0x2642, 0x2695, 0x2696, 0x2708, 0x2764)  # gender / profession / heart signs used in ZWJ sequences
    )


def codepoint_hits(text: str) -> list[tuple[int, int, str]]:
    """Return (line, col, label) for every disallowed code point in `text`."""
    hits: list[tuple[int, int, str]] = []
    line = 1
    col = 1
    for i, ch in enumerate(text):
        cp = ord(ch)
        if cp == 0x0A:
            line += 1
            col = 1
            continue
        if cp in BLOCK_CODEPOINTS:
            is_bidi = 0x202A <= cp <= 0x202E or 0x2066 <= cp <= 0x2069
            hits.append((line, col, f"U+{cp:04X} {'bidi-override' if is_bidi else 'zero-width'}"))
        elif cp == BOM and i != 0:
            hits.append((line, col, "U+FEFF byte-order-mark mid-file"))
        elif cp == ZWJ:
            prev_cp = ord(text[i - 1]) if i > 0 else 0
            next_cp = ord(text[i + 1]) if i + 1 < len(text) else 0
            if not (is_emoji_ish(prev_cp) and is_emoji_ish(next_cp)):
                hits.append((line, col, "U+200D zero-width-joiner outside an emoji sequence"))
        col += 1
    return hits


def read_text(path: Path) -> str | None:
    try:
        data = path.read_bytes()
    except (OSError, ValueError):
        return None
    if b"\x00" in data[:4096]:  # binary
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None


def git_tracked_files() -> list[Path]:
    out = subprocess.run(
        ["git", "ls-files"], cwd=REPO_ROOT, capture_output=True, text=True, check=True
    ).stdout
    return [REPO_ROOT / line for line in out.splitlines() if line]


def staged_files() -> list[Path]:
    out = subprocess.run(
        ["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR"],
        cwd=REPO_ROOT, capture_output=True, text=True, check=True,
    ).stdout
    return [REPO_ROOT / line for line in out.splitlines() if line]


def committed_surface(staged_only: bool) -> list[Path]:
    files = staged_files() if staged_only else git_tracked_files()
    return [p for p in files if p.suffix in COMMITTED_EXTS and p.is_file()]


def agent_surface() -> list[Path]:
    seen: set[Path] = set()
    for pattern in AGENT_GLOBS:
        for path in REPO_ROOT.glob(pattern):
            if not path.is_file():
                continue
            if any(part in AGENT_SKIP_DIRS for part in path.relative_to(REPO_ROOT).parts):
                continue
            seen.add(path)
    return sorted(seen)


def manifest_paths() -> list[Path]:
    """Tracked agent files that get a blessed sha256 in the drift manifest."""
    files = []
    for p in git_tracked_files():
        rel = p.relative_to(REPO_ROOT).as_posix()
        if (
            rel == "CLAUDE.md"
            or rel == ".claude/settings.json"
            or rel.startswith(".claude/agents/")
            or rel.startswith(".claude/rules/")
        ) and p.is_file():
            files.append(p)
    return sorted(files)


def sha256_of(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def rel(path: Path) -> str:
    return path.relative_to(REPO_ROOT).as_posix()


def update_manifest() -> int:
    lines = [f"{sha256_of(p)}  {rel(p)}" for p in manifest_paths()]
    MANIFEST_PATH.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"Wrote {len(lines)} blessed agent-file hashes to {rel(MANIFEST_PATH)}")
    return 0


def manifest_drift_warnings() -> list[str]:
    if not MANIFEST_PATH.exists():
        return []
    warnings = []
    for line in MANIFEST_PATH.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        expected, _, rel_path = line.partition("  ")
        path = REPO_ROOT / rel_path
        if not path.is_file():
            warnings.append(f"{rel_path}: blessed agent file is missing")
        elif sha256_of(path) != expected:
            warnings.append(f"{rel_path}: content changed since it was last blessed")
    return warnings


def check_manifest() -> int:
    """CI gate: nonzero when the blessed baseline is stale or incomplete.

    Drift here means a tracked agent file was edited (or removed) without
    re-blessing scripts/agent-files.sha256, so the agent-mode guard would warn
    on every later session. Also fail when a tracked manifest file has no blessed
    entry at all, so a newly added rule doc cannot silently escape the baseline.
    """
    if not MANIFEST_PATH.exists():
        print("error: scripts/agent-files.sha256 is missing; run --update-manifest", file=sys.stderr)
        return 1

    drift = manifest_drift_warnings()
    blessed = {line.partition("  ")[2] for line in MANIFEST_PATH.read_text(encoding="utf-8").splitlines() if line.strip()}
    unblessed = [rel(p) for p in manifest_paths() if rel(p) not in blessed]

    if not drift and not unblessed:
        return 0

    print("error: agent-file baseline is stale (scripts/agent-files.sha256).", file=sys.stderr)
    for w in drift:
        print(f"  drift: {w}", file=sys.stderr)
    for u in unblessed:
        print(f"  unblessed: {u}: tracked agent file has no entry in the manifest", file=sys.stderr)
    print(
        "  A PR that edits a tracked agent file (.claude/rules/**, .claude/agents/**,\n"
        "  CLAUDE.md, .claude/settings.json) must re-bless the baseline in the same change.\n"
        "  Fix: python3 scripts/scan-hidden-unicode.py --update-manifest, then commit the manifest.",
        file=sys.stderr,
    )
    return 1


def scan_committed(staged_only: bool) -> int:
    errors = 0
    for path in committed_surface(staged_only):
        text = read_text(path)
        if text is None:
            continue
        for line, col, label in codepoint_hits(text):
            errors += 1
            print(f"error: {rel(path)}:{line}:{col}: hidden code point ({label})", file=sys.stderr)
    if errors:
        print(
            f"\n{errors} hidden code point(s) found. Remove them; they have no place in source.\n"
            "If a match is a legitimate emoji, the scanner already allowlists emoji ZWJ sequences.",
            file=sys.stderr,
        )
    return 1 if errors else 0


def scan_agent() -> int:
    codepoint_errors = 0
    keyword_warnings: list[str] = []
    # Keyword-soup runs ONLY on the un-reviewed surface: untracked / gitignored
    # agent files (AGENTS.md, .codex/**, .claude/settings.local.json, .cursor/**,
    # *.mcp.json). Tracked .claude/** files went through PR review, and they
    # legitimately discuss shell commands (node -e, python -c) in reviewer prompts
    # and rule docs, so scanning them just produces false-positive fatigue. The
    # codepoint scan still covers every file regardless of tracked status.
    tracked = {p.resolve() for p in git_tracked_files()}
    for path in agent_surface():
        text = read_text(path)
        if text is None:
            continue
        for line, col, label in codepoint_hits(text):
            codepoint_errors += 1
            print(f"error: {rel(path)}:{line}:{col}: hidden code point ({label})", file=sys.stderr)
        if path.resolve() in tracked:
            continue
        for lineno, src_line in enumerate(text.splitlines(), start=1):
            for label, pattern in KEYWORD_PATTERNS:
                if pattern.search(src_line):
                    keyword_warnings.append(f"{rel(path)}:{lineno}: {label}")

    drift = manifest_drift_warnings()

    if codepoint_errors or keyword_warnings or drift:
        print("\nfallow agent-file guard: review the items below.", file=sys.stderr)
        if codepoint_errors:
            print(f"  {codepoint_errors} hidden code point(s) in agent-instruction files (above).", file=sys.stderr)
        for w in keyword_warnings:
            print(f"  warn (shell-exec shape): {w}", file=sys.stderr)
        for w in drift:
            print(f"  warn (drift): {w}", file=sys.stderr)
        print(
            "  Agent-instruction files are untrusted by default; a poisoned one can carry hidden\n"
            "  instructions for the next agent session. Inspect the flagged file(s).\n"
            "  After a legitimate change, re-bless the baseline: python3 scripts/scan-hidden-unicode.py --update-manifest",
            file=sys.stderr,
        )
    # Only a real hidden code point fails the check; keyword + drift are advisory.
    return 1 if codepoint_errors else 0


def main(argv: list[str]) -> int:
    mode = None
    staged_only = False
    for arg in argv:
        if arg == "--update-manifest":
            return update_manifest()
        if arg == "--mode=committed" or arg == "committed":
            mode = "committed"
        elif arg == "--mode=agent" or arg == "agent":
            mode = "agent"
        elif arg == "--mode=check-manifest" or arg == "check-manifest":
            mode = "check-manifest"
        elif arg == "--mode" :
            continue
        elif arg == "--staged":
            staged_only = True
    if mode == "committed":
        return scan_committed(staged_only)
    if mode == "agent":
        return scan_agent()
    if mode == "check-manifest":
        return check_manifest()
    print("usage: scan-hidden-unicode.py --mode {committed,agent,check-manifest} [--staged] | --update-manifest", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
