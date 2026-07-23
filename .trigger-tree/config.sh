# trigger-tree project configuration.
# Override per project: create $PROJECT/.trigger-tree/config.sh with the same variables.
# Paths are relative to the project root.

# A Read of a file matching this regex counts as a documentation read.
TT_WATCH_REGEX='^docs/.*\.(?:md|markdown)$|^\.claude/(?:rules|agents|skills)/.*\.md$|^\.codex/(?:references|agents|skills)/.*\.(?:md|toml)$|^\.agents/(?:rules|agents|skills)/.*\.md$|^\.agents/README\.md$|(^|/)(?:CLAUDE|AGENTS|GEMINI)\.md$|^(?:CONTEXT|CONTRIBUTING|README|BENCHMARKS|SECURITY|ROADMAP)\.md$'

# A Glob/Grep whose explicit target dir matches this regex counts as search activity.
# The event records that a search happened, not why it happened or whether routing failed.
TT_SCAN_REGEX='^(?:docs|\.claude/(?:rules|agents|skills)|\.codex/(?:references|agents|skills)|\.agents/(?:rules|agents|skills))(/|$)'

# Files matching this regex are loaded automatically (system-prompt injection, nested
# CLAUDE.md on-demand loading, Skill tool) and therefore cannot be judged through
# Read telemetry; they are excluded from untouched review-candidate analysis.
TT_ALWAYS_LOADED_REGEX='(^|/)(?:CLAUDE|AGENTS|GEMINI)\.md$|(^|/)CLAUDE\.local\.md$|^\.claude/(?:rules|agents|skills)/|^\.codex/(?:agents|skills)/|^\.agents/(?:agents|rules|skills)/'
# Example project override additions for other instruction systems:
# TT_ALWAYS_LOADED_REGEX='...|^\.github/copilot-instructions\.md$|^\.cursor/rules/'

# Markdown that is intentionally human-facing or generated per platform.
TT_SCOPE_IGNORE='.github/*,CHANGELOG.md,CODE_OF_CONDUCT.md,npm/darwin-*/README.md,npm/linux-*/README.md,npm/win32-*/README.md'

# Comma-separated globs for rare-but-critical documentation that must be reviewed,
# never treated as an archive candidate. Safety paths are protected regardless.
TT_CRITICAL_GLOB='AGENTS.md,CLAUDE.md,.claude/rules/workflow.md,.claude/rules/release-workflow.md,.claude/rules/detection.md,.codex/references/quality-gates.md,.codex/references/review-routing.md'

# Store only a stable prompt fingerprint. Use `off` for markers without linkability.
TT_LOG_PROMPTS='hash'

# Rotate history.jsonl to history-<timestamp>.jsonl when it exceeds this many bytes.
TT_ROTATE_BYTES='5242880'

# Experimental, correlational view joining reads with local session outcomes.
# Values: off (default) or on. This never makes causal claims or sends data anywhere.
TT_EXPERIMENTAL_OUTCOMES='off'
