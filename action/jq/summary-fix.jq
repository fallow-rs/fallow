# This file serves every fallow version, because the fix command has no native
# report kind.

# A withheld entry keeps its `type`, so the skip flag is the only thing that
# separates a removal that landed from one this run declined. Counting a
# withheld entry here would report a write that never happened.
(.fixes | map(select(.type == "remove_export" and ((.skipped // false) == false)))) as $export_fixes |
(.fixes | map(select(.type == "remove_dependency" and ((.skipped // false) == false)))) as $dep_fixes |
($export_fixes | length) as $exports |
($dep_fixes | length) as $deps |
# Hash-mismatch skips and other `skipped: true` entries inflate the raw
# fixes-array length; count only entries that represent a fix attempt
# (NOT a skip record) for the headline.
(.fixes | map(select((.skipped // false) == false)) | length) as $fix_attempts |
((.skipped_content_changed // 0) | tonumber) as $content_changed |
((.skipped_mixed_line_endings // 0) | tonumber) as $mixed_eol |
((.skipped_low_confidence_exports // 0) | tonumber) as $low_confidence |
# Every withholding counter has to be read here. A run that withheld only a
# dependency or an enum member has `fixes` entries the gate counts, so leaving
# a counter out makes this summary say "No fixable issues found" under a job
# that reports the same run as having fixable issues.
((.skipped_low_confidence_dependencies // 0) | tonumber) as $low_confidence_deps |
((.skipped_low_confidence_members // 0) | tonumber) as $low_confidence_members |

if $fix_attempts == 0 and $content_changed == 0 and $mixed_eol == 0 and $low_confidence == 0
  and $low_confidence_deps == 0 and $low_confidence_members == 0 then
  "## Fallow - Auto-fix\n\nNo fixable issues found."
else
  "## Fallow - Auto-fix\n\n" +
  (if .dry_run then "**Dry run**: would apply" else "Applied" end) +
  " **\($fix_attempts) fixes**" +
  (if .dry_run then "" else " (\(.total_fixed) succeeded)" end) +
  (if $content_changed > 0 then
    ", skipped \($content_changed) file(s) that changed since analysis"
  else "" end) +
  (if $mixed_eol > 0 then
    ", skipped \($mixed_eol) file(s) with mixed line endings"
  else "" end) +
  (if $low_confidence > 0 then
    ", kept exports in \($low_confidence) file(s) where consumers may be hidden from static analysis"
  else "" end) +
  (if $low_confidence_deps > 0 then
    ", kept \($low_confidence_deps) declared package(s) whose only import may sit in a file this run did not fully read"
  else "" end) +
  (if $low_confidence_members > 0 then
    ", kept \($low_confidence_members) unused enum member(s) whose only reference may sit in a file this run did not fully analyze"
  else "" end) + "\n\n" +
  "| Type | Count |\n|------|-------|\n" +
  (if $exports > 0 then "| Export removals | \($exports) |\n" else "" end) +
  (if $deps > 0 then "| Dependency removals | \($deps) |\n" else "" end) +
  "\n<details>\n<summary>View details</summary>\n\n" +
  (if $exports > 0 then
    "**Export removals (\($exports))**\n" +
    ([$export_fixes[:25][] |
      "- `\(.path):\(.line)` - `\(.name)`"] | join("\n")) +
    (if $exports > 25 then "\n- *... and \($exports - 25) more*" else "" end) +
    "\n\n"
  else "" end) +
  (if $deps > 0 then
    "**Dependency removals (\($deps))**\n" +
    ([$dep_fixes[:25][] |
      "- `\(.package)` from \(.location) in `\(.file)`"] | join("\n")) +
    (if $deps > 25 then "\n- *... and \($deps - 25) more*" else "" end) +
    "\n"
  else "" end) +
  "\n\n</details>"
end
