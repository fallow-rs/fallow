#!/usr/bin/env python3
"""Build a small public Fallow config corpus report."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None


CONFIG_FILENAMES = (".fallowrc.json", ".fallowrc.jsonc", "fallow.toml")
KEY_LABELS = {
    "entry": "entry",
    "dynamicallyLoaded": "dynamicallyLoaded",
    "ignorePatterns": "ignorePatterns",
    "ignoreDependencies": "ignoreDependencies",
    "ignoreExports": "ignoreExports",
    "usedClassMembers": "usedClassMembers",
    "rules": "rules",
    "audit": "audit / baseline",
}
COMMENT_PHRASES = (
    "false positive",
    "workaround",
    "fallow misses",
    "loaded by",
    "framework",
    "generated",
    "not imported",
    "runtime",
    "plugin",
    "dynamic",
    "entrypoint",
    "entry point",
    "keep",
    "manual",
)


@dataclass(frozen=True)
class SearchItem:
    query: str
    repo: str
    repo_url: str
    path: str
    blob_sha: str
    blob_url: str
    ref: str


@dataclass(frozen=True)
class CommentHit:
    line: int
    phrase: str
    text: str


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Fetch public Fallow config files and summarize workaround signals."
    )
    parser.add_argument("--limit", type=int, default=40, help="Maximum search results per filename")
    parser.add_argument(
        "--cache-dir",
        type=Path,
        default=Path(".fallow/public-config-corpus"),
        help="Directory for cached config snapshots and manifest.json",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("docs/public-config-corpus.md"),
        help="Markdown report path, use '-' for stdout",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        help="Manifest path, defaults to <cache-dir>/manifest.json",
    )
    parser.add_argument(
        "--from-search-fixture",
        type=Path,
        help="Read fake gh search results from this JSON file instead of calling GitHub",
    )
    parser.add_argument(
        "--offline",
        action="store_true",
        help="Do not fetch missing cache entries. Implied by --from-search-fixture.",
    )
    parser.add_argument("--timeout", type=float, default=15.0, help="Raw fetch timeout in seconds")
    parser.add_argument("--retries", type=int, default=2, help="Raw fetch retry count")
    parser.add_argument("--search-timeout", type=float, default=30.0, help="GitHub code search timeout in seconds")
    parser.add_argument(
        "--fetched-at",
        help="Override fetch timestamp for deterministic fixture tests",
    )
    parser.add_argument(
        "--gh-version",
        help="Override gh version metadata for deterministic fixture tests",
    )
    args = parser.parse_args()

    if args.limit < 1:
        parser.error("--limit must be >= 1")
    if args.retries < 0:
        parser.error("--retries must be >= 0")
    if args.search_timeout <= 0:
        parser.error("--search-timeout must be > 0")

    args.cache_dir.mkdir(parents=True, exist_ok=True)
    manifest_path = args.manifest or args.cache_dir / "manifest.json"
    offline = args.offline or args.from_search_fixture is not None
    fetched_at = args.fetched_at or now_utc()
    gh_version = args.gh_version or detect_gh_version(offline)

    try:
        search_groups = load_search_groups(args.from_search_fixture, args.limit)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"error: failed to load search fixture: {error}", file=sys.stderr)
        return 2

    if search_groups is None:
        try:
            search_groups = run_live_searches(args.limit, args.search_timeout)
        except (OSError, subprocess.SubprocessError, json.JSONDecodeError) as error:
            print(f"error: failed to run GitHub code search: {error}", file=sys.stderr)
            return 2

    entries: list[dict[str, Any]] = []
    seen_items: set[tuple[str, str, str]] = set()
    for query, raw_items in search_groups:
        for raw in raw_items:
            item = normalize_search_item(query, raw)
            if item is None:
                entries.append(
                    {
                        "query": query,
                        "parse_status": "search-result-error",
                        "fetch_error": "search result is missing repo/path/url metadata",
                    }
                )
                continue
            identity = (item.repo, item.path, item.blob_sha or item.ref)
            if identity in seen_items:
                continue
            seen_items.add(identity)
            entries.append(
                analyze_item(
                    item=item,
                    cache_dir=args.cache_dir,
                    fetched_at=fetched_at,
                    gh_version=gh_version,
                    offline=offline,
                    timeout=args.timeout,
                    retries=args.retries,
                    limit=args.limit,
                )
            )

    entries.sort(key=manifest_sort_key)
    manifest = {
        "generated_at": fetched_at,
        "gh_version": gh_version,
        "limit_per_filename": args.limit,
        "filenames": list(CONFIG_FILENAMES),
        "entries": entries,
    }

    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    report = render_markdown(manifest, manifest_path)
    if str(args.output) == "-":
        sys.stdout.write(report)
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(report, encoding="utf-8")

    return 0


def now_utc() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def detect_gh_version(offline: bool) -> str:
    if offline:
        return "fixture"
    try:
        result = subprocess.run(
            ["gh", "--version"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        return "unknown"
    return result.stdout.splitlines()[0] if result.stdout else "unknown"


def load_search_groups(path: Path | None, limit: int) -> list[tuple[str, list[dict[str, Any]]]] | None:
    if path is None:
        return None
    data = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(data, list):
        return [("fixture", data[:limit])]
    if not isinstance(data, dict):
        raise ValueError("fixture root must be an object or array")
    groups: list[tuple[str, list[dict[str, Any]]]] = []
    if "queries" in data:
        for group in data["queries"]:
            query = str(group.get("query", "fixture"))
            results = group.get("results", [])
            if not isinstance(results, list):
                raise ValueError(f"results for {query} must be an array")
            groups.append((query, results[:limit]))
        return groups
    for query, results in data.items():
        if not isinstance(results, list):
            raise ValueError(f"results for {query} must be an array")
        groups.append((str(query), results[:limit]))
    return groups


def run_live_searches(limit: int, search_timeout: float) -> list[tuple[str, list[dict[str, Any]]]]:
    groups: list[tuple[str, list[dict[str, Any]]]] = []
    for filename in CONFIG_FILENAMES:
        query = f"filename:{filename}"
        command = [
            "gh",
            "search",
            "code",
            "--filename",
            filename,
            "--json",
            "path,repository,sha,url",
            "--limit",
            str(limit),
        ]
        result = subprocess.run(
            command,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=search_timeout,
        )
        groups.append((query, json.loads(result.stdout)))
    return groups


def normalize_search_item(query: str, raw: dict[str, Any]) -> SearchItem | None:
    repo = raw.get("repository")
    if not isinstance(repo, dict):
        return None
    name = repo.get("nameWithOwner")
    path = raw.get("path")
    url = raw.get("url")
    if not isinstance(name, str) or not isinstance(path, str) or not isinstance(url, str):
        return None
    ref = parse_blob_ref(url, path)
    if not ref:
        return None
    repo_url = repo.get("url") if isinstance(repo.get("url"), str) else f"https://github.com/{name}"
    sha = raw.get("sha") if isinstance(raw.get("sha"), str) else ""
    return SearchItem(
        query=query,
        repo=name,
        repo_url=repo_url,
        path=path,
        blob_sha=sha,
        blob_url=url,
        ref=ref,
    )


def parse_blob_ref(url: str, path: str) -> str:
    marker = "/blob/"
    start = url.find(marker)
    if start < 0:
        return ""
    rest = url[start + len(marker) :]
    suffix = "/" + path
    if not rest.endswith(suffix):
        return ""
    return rest[: -len(suffix)]


def analyze_item(
    *,
    item: SearchItem,
    cache_dir: Path,
    fetched_at: str,
    gh_version: str,
    offline: bool,
    timeout: float,
    retries: int,
    limit: int,
) -> dict[str, Any]:
    cache_path = cache_dir / cache_filename(item)
    raw_url = build_raw_url(item)
    content = ""
    fetch_error = ""

    if cache_path.exists():
        content = cache_path.read_text(encoding="utf-8", errors="replace")
    elif offline:
        fetch_error = f"cache miss in offline mode: {cache_path}"
    else:
        try:
            content = fetch_raw(raw_url, timeout=timeout, retries=retries)
            cache_path.parent.mkdir(parents=True, exist_ok=True)
            cache_path.write_text(content, encoding="utf-8")
        except (OSError, urllib.error.URLError, TimeoutError) as error:
            fetch_error = str(error)

    entry: dict[str, Any] = {
        "repo": item.repo,
        "path": item.path,
        "blob_url": item.blob_url,
        "raw_url": raw_url,
        "ref": item.ref,
        "blob_sha": item.blob_sha,
        "cache_path": str(cache_path),
        "query": item.query,
        "limit_per_filename": limit,
        "gh_version": gh_version,
        "fetched_at": fetched_at,
    }

    if fetch_error:
        entry.update(
            {
                "fetch_error": fetch_error,
                "bytes": 0,
                "sha256": "",
                "detected_format": detect_format(item.path),
                "parse_status": "not-fetched",
                "keys": [],
                "comment_hits": [],
            }
        )
        return entry

    parsed, parse_status, comments = parse_config(item.path, content)
    key_hits = detect_key_hits(parsed)
    comment_hits = detect_comment_hits(comments)
    encoded = content.encode("utf-8")
    entry.update(
        {
            "bytes": len(encoded),
            "sha256": hashlib.sha256(encoded).hexdigest(),
            "detected_format": detect_format(item.path),
            "parse_status": parse_status,
            "keys": key_hits,
            "comment_hits": [hit.__dict__ for hit in comment_hits],
        }
    )
    return entry


def cache_filename(item: SearchItem) -> str:
    identity = item.blob_sha or item.ref
    return f"{safe_name(item.repo)}__{safe_name(identity[:16])}__{safe_name(item.path)}"


def safe_name(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]+", "__", value).strip("_") or "unknown"


def build_raw_url(item: SearchItem) -> str:
    owner_repo = item.repo
    ref = item.ref
    path = item.path
    return f"https://raw.githubusercontent.com/{owner_repo}/{ref}/{path}"


def fetch_raw(raw_url: str, *, timeout: float, retries: int) -> str:
    last_error: Exception | None = None
    for attempt in range(retries + 1):
        try:
            request = urllib.request.Request(raw_url, headers={"User-Agent": "fallow-public-config-corpus"})
            with urllib.request.urlopen(request, timeout=timeout) as response:
                return response.read().decode("utf-8", errors="replace")
        except (urllib.error.URLError, TimeoutError, OSError) as error:
            last_error = error
            if attempt < retries:
                time.sleep(min(2**attempt, 5))
    assert last_error is not None
    raise last_error


def detect_format(path: str) -> str:
    if path.endswith(".toml"):
        return "toml"
    if path.endswith(".jsonc"):
        return "jsonc"
    if path.endswith(".json"):
        return "json"
    return "unknown"


def parse_config(path: str, content: str) -> tuple[Any, str, list[tuple[int, str]]]:
    fmt = detect_format(path)
    if fmt in {"json", "jsonc"}:
        stripped, comments = strip_jsonc_comments(content)
        stripped = strip_jsonc_trailing_commas(stripped)
        try:
            return json.loads(stripped), "ok", comments
        except json.JSONDecodeError as error:
            return None, f"parse-error: {error.msg}", comments
    if fmt == "toml":
        comments = extract_toml_comments(content)
        if tomllib is not None:
            try:
                return tomllib.loads(content), "ok", comments
            except tomllib.TOMLDecodeError as error:
                return None, f"parse-error: {error}", comments
        try:
            return parse_toml_fallback(content), "ok", comments
        except ValueError as error:
            return None, f"parse-error: {error}", comments
    return None, "unsupported-format", []


def parse_toml_fallback(content: str) -> dict[str, Any]:
    parsed: dict[str, Any] = {}
    current = parsed
    for logical_line in toml_logical_lines(content):
        line = logical_line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            current = parsed
            for part in line[1:-1].split("."):
                part = part.strip()
                if not part:
                    raise ValueError("empty table name")
                nested = current.setdefault(part, {})
                if not isinstance(nested, dict):
                    raise ValueError(f"table conflicts with scalar: {part}")
                current = nested
            continue
        if "=" not in line:
            raise ValueError(f"unsupported TOML line: {line}")
        key, value = line.split("=", 1)
        current[key.strip()] = parse_toml_value_fallback(value.strip())
    return parsed


def toml_logical_lines(content: str) -> list[str]:
    lines: list[str] = []
    pending: list[str] = []
    bracket_depth = 0
    for raw_line in content.splitlines():
        stripped = strip_toml_inline_comment(raw_line).strip()
        if not stripped:
            continue
        pending.append(stripped)
        bracket_depth += toml_bracket_delta(stripped)
        if bracket_depth <= 0:
            lines.append(" ".join(pending))
            pending = []
            bracket_depth = 0
    if pending:
        raise ValueError(f"unterminated TOML array: {' '.join(pending)}")
    return lines


def toml_bracket_delta(line: str) -> int:
    delta = 0
    in_string = False
    escape = False
    for ch in line:
        if in_string:
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif ch == '"':
                in_string = False
            continue
        if ch == '"':
            in_string = True
            continue
        if ch == "[":
            delta += 1
        elif ch == "]":
            delta -= 1
    return delta


def strip_toml_inline_comment(line: str) -> str:
    in_string = False
    escape = False
    for index, ch in enumerate(line):
        if in_string:
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif ch == '"':
                in_string = False
            continue
        if ch == '"':
            in_string = True
            continue
        if ch == "#":
            return line[:index]
    return line


def parse_toml_value_fallback(value: str) -> Any:
    if value.startswith("[") and value.endswith("]"):
        inner = value[1:-1].strip()
        if not inner:
            return []
        return [parse_toml_value_fallback(part.strip()) for part in split_toml_array(inner)]
    if value.startswith('"') and value.endswith('"'):
        return value[1:-1]
    if value in {"true", "false"}:
        return value == "true"
    return value


def split_toml_array(value: str) -> list[str]:
    parts: list[str] = []
    start = 0
    in_string = False
    escape = False
    for index, ch in enumerate(value):
        if in_string:
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif ch == '"':
                in_string = False
            continue
        if ch == '"':
            in_string = True
            continue
        if ch == ",":
            parts.append(value[start:index])
            start = index + 1
    parts.append(value[start:])
    return parts


def strip_jsonc_comments(content: str) -> tuple[str, list[tuple[int, str]]]:
    out: list[str] = []
    comments: list[tuple[int, str]] = []
    i = 0
    line = 1
    in_string = False
    escape = False
    while i < len(content):
        ch = content[i]
        nxt = content[i + 1] if i + 1 < len(content) else ""
        if in_string:
            out.append(ch)
            if ch == "\n":
                line += 1
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif ch == '"':
                in_string = False
            i += 1
            continue
        if ch == '"':
            in_string = True
            out.append(ch)
            i += 1
            continue
        if ch == "/" and nxt == "/":
            start_line = line
            j = i + 2
            while j < len(content) and content[j] != "\n":
                j += 1
            comments.append((start_line, content[i + 2 : j].strip()))
            out.extend(" " * (j - i))
            i = j
            continue
        if ch == "/" and nxt == "*":
            start_line = line
            j = i + 2
            comment_chars: list[str] = []
            while j < len(content) - 1 and not (content[j] == "*" and content[j + 1] == "/"):
                comment_chars.append(content[j])
                if content[j] == "\n":
                    line += 1
                j += 1
            j = min(j + 2, len(content))
            comments.append((start_line, " ".join("".join(comment_chars).split())))
            out.extend("\n" if c == "\n" else " " for c in content[i:j])
            i = j
            continue
        out.append(ch)
        if ch == "\n":
            line += 1
        i += 1
    return "".join(out), comments


def strip_jsonc_trailing_commas(content: str) -> str:
    out: list[str] = []
    i = 0
    in_string = False
    escape = False
    while i < len(content):
        ch = content[i]
        if in_string:
            out.append(ch)
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif ch == '"':
                in_string = False
            i += 1
            continue
        if ch == '"':
            in_string = True
            out.append(ch)
            i += 1
            continue
        if ch == ",":
            j = i + 1
            while j < len(content) and content[j].isspace():
                j += 1
            if j < len(content) and content[j] in "]}":
                out.append(" ")
                i += 1
                continue
        out.append(ch)
        i += 1
    return "".join(out)


def extract_toml_comments(content: str) -> list[tuple[int, str]]:
    comments: list[tuple[int, str]] = []
    for line_no, line in enumerate(content.splitlines(), start=1):
        in_string = False
        escape = False
        for index, ch in enumerate(line):
            if in_string:
                if escape:
                    escape = False
                elif ch == "\\":
                    escape = True
                elif ch == '"':
                    in_string = False
                continue
            if ch == '"':
                in_string = True
                continue
            if ch == "#":
                comments.append((line_no, line[index + 1 :].strip()))
                break
    return comments


def detect_key_hits(parsed: Any) -> list[str]:
    if not isinstance(parsed, dict):
        return []
    hits: list[str] = []
    for key in ("entry", "dynamicallyLoaded", "ignorePatterns", "ignoreDependencies", "ignoreExports", "usedClassMembers"):
        if has_non_empty_key(parsed, key):
            hits.append(key)
    rules = parsed.get("rules")
    if isinstance(rules, dict) and rules:
        hits.append("rules")
    if has_non_empty_key(parsed, "audit") or has_baseline_key(parsed):
        hits.append("audit")
    return hits


def has_non_empty_key(parsed: dict[str, Any], key: str) -> bool:
    if key not in parsed:
        return False
    value = parsed[key]
    if value is None:
        return False
    if isinstance(value, (list, dict, str)):
        return bool(value)
    return True


def has_baseline_key(value: Any) -> bool:
    if isinstance(value, dict):
        for key, nested in value.items():
            if "baseline" in str(key).lower():
                return True
            if has_baseline_key(nested):
                return True
    elif isinstance(value, list):
        return any(has_baseline_key(item) for item in value)
    return False


def detect_comment_hits(comments: list[tuple[int, str]]) -> list[CommentHit]:
    hits: list[CommentHit] = []
    for line, text in comments:
        lowered = text.lower()
        for phrase in COMMENT_PHRASES:
            if phrase in lowered:
                hits.append(CommentHit(line=line, phrase=phrase, text=compact_snippet(text)))
                break
    return hits


def compact_snippet(text: str, limit: int = 140) -> str:
    compact = " ".join(text.split())
    if len(compact) <= limit:
        return compact
    return compact[: limit - 3].rstrip() + "..."


def manifest_sort_key(entry: dict[str, Any]) -> tuple[str, str, str]:
    return (str(entry.get("repo", "")), str(entry.get("path", "")), str(entry.get("blob_sha", "")))


def render_markdown(manifest: dict[str, Any], manifest_path: Path) -> str:
    entries = manifest["entries"]
    fetched = [entry for entry in entries if not entry.get("fetch_error")]
    fetch_failures = [entry for entry in entries if entry.get("fetch_error")]
    parse_failures = [entry for entry in fetched if str(entry.get("parse_status", "")) != "ok"]
    key_counts = count_keys(fetched)
    comment_rows = collect_comment_rows(fetched)

    lines = [
        "# Public Fallow Config Corpus",
        "",
        "This maintainer report summarizes public Fallow config files that may encode workaround signals. Comment hits are candidate evidence only, not confirmed false positives.",
        "",
        "## How To Run",
        "",
        "```bash",
        "python3 scripts/public-config-corpus.py --limit 40 --output docs/public-config-corpus.md",
        "```",
        "",
        f"- Generated at: `{manifest['generated_at']}`",
        f"- `gh` version: `{manifest['gh_version']}`",
        f"- Per-filename cap: `{manifest['limit_per_filename']}`",
        f"- Manifest: `{manifest_path}`",
        "- Cache: `.fallow/public-config-corpus/` by default, intentionally untracked.",
        "- Public repositories only. Do not use private repository tokens for this corpus.",
        "- Store only the small config snapshots needed for reproducibility. When filing issues, link to source and quote only short snippets.",
        "",
        "## Current Report",
        "",
        f"- Search results considered: {len(entries)}",
        f"- Fetched configs: {len(fetched)}",
        f"- Fetch failures: {len(fetch_failures)}",
        f"- Parse failures: {len(parse_failures)}",
        f"- Candidate workaround comments: {len(comment_rows)}",
        "",
        "### Workaround Key Counts",
        "",
        "| Signal | Configs |",
        "|---|---:|",
    ]
    for key, label in KEY_LABELS.items():
        lines.append(f"| `{md_escape(label)}` | {key_counts.get(key, 0)} |")

    lines.extend(
        [
            "",
            "### Review Queue",
            "",
        ]
    )
    if key_counts:
        top = sorted(key_counts.items(), key=lambda item: (-item[1], KEY_LABELS.get(item[0], item[0])))[:5]
        for key, count in top:
            lines.append(f"- Review `{KEY_LABELS.get(key, key)}` usage across {count} config(s).")
    if comment_rows:
        lines.append(f"- Inspect {len(comment_rows)} candidate workaround comment(s).")
    if parse_failures:
        lines.append(f"- Check {len(parse_failures)} parse failure(s), these may indicate unsupported config syntax or search noise.")
    if fetch_failures:
        lines.append(f"- Re-run or inspect {len(fetch_failures)} fetch failure(s); the corpus was partial.")
    if not key_counts and not comment_rows and not parse_failures and not fetch_failures:
        lines.append("- No review items found in this run.")

    lines.extend(
        [
            "",
            "### Candidate Workaround Comments",
            "",
            "| Repo | Path | Line | Phrase | Snippet |",
            "|---|---|---:|---|---|",
        ]
    )
    if comment_rows:
        for row in comment_rows[:50]:
            lines.append(
                f"| {md_link(row['repo'], row['blob_url'])} | `{md_escape(row['path'])}` | {row['line']} | `{md_escape(row['phrase'])}` | {md_escape(row['text'])} |"
            )
    else:
        lines.append("| _none_ |  |  |  |  |")

    lines.extend(
        [
            "",
            "### Parse Failures",
            "",
            "| Repo | Path | Status |",
            "|---|---|---|",
        ]
    )
    if parse_failures:
        for entry in parse_failures:
            lines.append(
                f"| {md_link(str(entry.get('repo', 'unknown')), str(entry.get('blob_url', '')))} | `{md_escape(str(entry.get('path', '')))}` | `{md_escape(str(entry.get('parse_status', '')))}` |"
            )
    else:
        lines.append("| _none_ |  |  |")

    lines.extend(
        [
            "",
            "### Fetch Failures",
            "",
            "| Repo | Path | Error |",
            "|---|---|---|",
        ]
    )
    if fetch_failures:
        for entry in fetch_failures:
            lines.append(
                f"| {md_escape(str(entry.get('repo', 'unknown')))} | `{md_escape(str(entry.get('path', '')))}` | `{md_escape(str(entry.get('fetch_error', '')))}` |"
            )
    else:
        lines.append("| _none_ |  |  |")

    lines.extend(
        [
            "",
            "## Seed Report From 2026-05-22 Research",
            "",
            "The first manual pass over 100 public configs found:",
            "",
            "- 72 / 100 configs define custom `entry` values.",
            "- 59 / 100 configs define `ignorePatterns`.",
            "- 57 / 100 configs define `ignoreDependencies`.",
            "- 48 / 100 configs customize `rules`.",
            "- 18 / 100 configs include `audit` or baseline config.",
            "",
            "Known issue family from that pass:",
            "",
            "- [#546](https://github.com/fallow-rs/fallow/issues/546): Storybook staticDirs and manager runtime resources.",
            "- [#586](https://github.com/fallow-rs/fallow/issues/586): Playwright fixture class-member propagation.",
            "- [#588](https://github.com/fallow-rs/fallow/issues/588), [#589](https://github.com/fallow-rs/fallow/issues/589), [#590](https://github.com/fallow-rs/fallow/issues/590): rwsdk, Wrangler, Node loader, and content-collections convention-loaded files.",
            "- [#600](https://github.com/fallow-rs/fallow/issues/600): Electron-Vite renderer HTML entries.",
            "- [#601](https://github.com/fallow-rs/fallow/issues/601), [#602](https://github.com/fallow-rs/fallow/issues/602): Vitest alias and mock-module consumers.",
            "",
        ]
    )
    return "\n".join(lines)


def count_keys(entries: list[dict[str, Any]]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for entry in entries:
        for key in entry.get("keys", []):
            counts[key] = counts.get(key, 0) + 1
    return counts


def collect_comment_rows(entries: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for entry in entries:
        for hit in entry.get("comment_hits", []):
            rows.append(
                {
                    "repo": entry.get("repo", "unknown"),
                    "path": entry.get("path", ""),
                    "blob_url": entry.get("blob_url", ""),
                    "line": hit.get("line", 0),
                    "phrase": hit.get("phrase", ""),
                    "text": hit.get("text", ""),
                }
            )
    rows.sort(key=lambda row: (str(row["repo"]), str(row["path"]), int(row["line"]), str(row["phrase"])))
    return rows


def md_escape(value: str) -> str:
    return value.replace("\\", "\\\\").replace("|", "\\|").replace("\n", " ")


def md_link(label: str, url: str) -> str:
    safe_label = md_escape(label)
    if not url:
        return safe_label
    return f"[{safe_label}]({url})"


if __name__ == "__main__":
    raise SystemExit(main())
