#!/usr/bin/env python3
"""trigger-tree statusline: live doc-discovery stats for the current session.

Portable (macOS/Linux), stdlib only. The dot pulses with the age of the last read or scan:
● bright green < 90s, ◐ amber < 10min, ○ dim otherwise.
Register in project or user settings under "statusLine" with a refreshInterval.
"""

import hashlib
import json
import os
import re
import sys
import time
import unicodedata
from datetime import datetime, timezone

ROOT = os.environ.get("TT_PROJECT_DIR") or os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
HIST = os.path.join(ROOT, ".trigger-tree", "history.jsonl")
BADGE = os.path.join(ROOT, ".trigger-tree", "badge.json")

RESET = "\033[0m"
FRESH = "\033[1;38;5;114m"  # bright green
WARM = "\033[38;5;178m"  # amber
COLD = "\033[38;5;245m"  # dim


def terminal_safe(value):
    """Strip terminal controls and invisible direction overrides from untrusted text."""
    bidi_controls = "\u061c\u200e\u200f\u202a\u202b\u202c\u202d\u202e\u2066\u2067\u2068\u2069"
    text = re.sub(r"(?:\x1b\]|\x9d).*?(?:\x07|\x1b\\|\x9c)", "", str(value), flags=re.S)
    text = re.sub(r"(?:\x1b\[|\x9b)[0-?]*[ -/]*[@-~]", "", text)
    text = re.sub(r"\x1b[@-_]", "", text)
    return "".join(
        ch
        for ch in text
        if unicodedata.category(ch) not in ("Cc", "Cs") and ch not in bidi_controls
    )


try:
    sys.stdout.reconfigure(encoding="utf-8")  # emoji-safe on Windows consoles
except AttributeError:  # pragma: no cover, exotic stdout replacement
    pass


def main():
    try:
        data = json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        data = {}
    session = data.get("session_id")
    if not session:
        print("🌳 tt: no data")
        return

    files, scans, last, last_time = {}, 0, None, None
    state_name = hashlib.sha256(str(session).encode("utf-8")).hexdigest()[:32] + ".json"
    state_path = os.path.join(ROOT, ".trigger-tree", "sessions", state_name)
    try:
        state = json.loads(open(state_path, encoding="utf-8").read())
    except (OSError, ValueError):
        state = None
    if state is not None:
        files = {path: True for path in state.get("files", [])}
        scans = int(state.get("scans", 0))
        last = state.get("last")
        try:
            last_time = datetime.strptime(last["ts"], "%Y-%m-%dT%H:%M:%SZ").replace(
                tzinfo=timezone.utc
            )
        except (KeyError, TypeError, ValueError):
            last_time = None
    elif not os.path.isfile(HIST):
        print("🌳 tt: no data")
        return
    else:
        fh = open(HIST, encoding="utf-8")
        try:
            scans, last, last_time = _collect_history(fh, session, files)
        finally:
            fh.close()

    if not files and not scans:
        print("🌳 tt: 0 docs consulted")
        return

    dirs = {os.path.dirname(p) for p in files}
    depth = max((p.count("/") for p in files), default=0)
    grade = mature_grade()
    stats = f"{grade} · " if grade else ""
    stats += f"{len(files)} files · {scans} searches · {len(dirs)} folders · depth {depth}"

    age = 10**9
    if last_time is not None:
        age = time.time() - last_time.timestamp()

    if age < 90:
        dot, color = "●", FRESH
    elif age < 600:
        dot, color = "◐", WARM
    else:
        dot, color = "○", COLD

    path = terminal_safe(last["path"]) + ("/" if last.get("t") == "scan" else "")
    print(f"🌳 {stats} {color}{dot} {path}{RESET}")


def mature_grade():
    """Read the optional public-safe badge cache; measuring badges have no grade."""
    try:
        payload = json.loads(open(BADGE, encoding="utf-8").read())
    except (OSError, ValueError):
        return None
    message = payload.get("message")
    match = re.fullmatch(r"([A-F]) \(\d{1,3}\)", message) if isinstance(message, str) else None
    return match.group(1) if match else None


def _collect_history(fh, session, files):
    scans, last, last_time = 0, None, None
    for line in fh:
        if f'"session":"{session}"' not in line and f'"session": "{session}"' not in line:
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        typ = event.get("t")
        if typ == "read":
            files[event["path"]] = True
        elif typ == "scan":
            scans += 1
        else:
            continue
        try:
            event_time = datetime.strptime(event["ts"], "%Y-%m-%dT%H:%M:%SZ").replace(
                tzinfo=timezone.utc
            )
        except (KeyError, ValueError):
            event_time = None
        if last is None or (
            event_time is not None and (last_time is None or event_time >= last_time)
        ):
            last, last_time = event, event_time
    return scans, last, last_time


if __name__ == "__main__":
    try:
        main()
    except Exception:
        print("🌳 tt")
    sys.exit(0)
