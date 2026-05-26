(.fixes | map(select(.type == "remove_export")) | length) as $exports |
(.fixes | map(select(.type == "remove_dependency")) | length) as $deps |
# Hash-mismatch skips and other `skipped: true` entries inflate the raw
# fixes-array length; count only entries that represent a fix attempt
# (NOT a skip record) for the headline.
(.fixes | map(select((.skipped // false) == false)) | length) as $fix_attempts |
((.skipped_content_changed // 0) | tonumber) as $content_changed |
((.skipped_mixed_line_endings // 0) | tonumber) as $mixed_eol |
((.skipped_low_confidence_exports // 0) | tonumber) as $low_confidence |

if $fix_attempts == 0 and $content_changed == 0 and $mixed_eol == 0 and $low_confidence == 0 then
  "## Fallow — Auto-fix\n\nNo fixable issues found."
else
  "## Fallow — Auto-fix\n\n" +
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
  else "" end) + "\n\n" +
  "| Type | Count |\n|------|-------|\n" +
  (if $exports > 0 then "| Export removals | \($exports) |\n" else "" end) +
  (if $deps > 0 then "| Dependency removals | \($deps) |\n" else "" end) +
  "\n<details>\n<summary>View details</summary>\n\n" +
  (if $exports > 0 then
    "**Export removals (\($exports))**\n" +
    ([.fixes | map(select(.type == "remove_export"))[:25][] |
      "- `\(.path):\(.line)` — `\(.name)`"] | join("\n")) +
    (if $exports > 25 then "\n- *... and \($exports - 25) more*" else "" end) +
    "\n\n"
  else "" end) +
  (if $deps > 0 then
    "**Dependency removals (\($deps))**\n" +
    ([.fixes | map(select(.type == "remove_dependency"))[:25][] |
      "- `\(.package)` from \(.location) in `\(.file)`"] | join("\n")) +
    (if $deps > 25 then "\n- *... and \($deps - 25) more*" else "" end) +
    "\n"
  else "" end) +
  "\n\n</details>"
end
