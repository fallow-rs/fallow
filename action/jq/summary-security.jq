def plural(n; word): "\(n) \(word)\(if n == 1 then "" else "s" end)";
def rel_path:
  if startswith("/") then
    (split("/") | if length > 3 then .[-3:] | join("/") else join("/") end)
  else . end;
def path_line:
  "`\(.path | rel_path)\(if .line then ":\(.line)" else "" end)`";
def finding_count:
  if .gate then (.gate.new_count // 0)
  else (.summary.security_findings // ((.security_findings // []) | length))
  end;
def gate_line:
  if .gate then
    "\n\nSecurity gate: `\(.gate.mode)`, verdict: `\(.gate.verdict)`, matching candidates: **\(.gate.new_count // 0)**."
  else "" end;
def finding_rows:
  [(.security_findings // [])[:15][] |
    "| \(path_line) | \(.kind) | \(.severity // "unknown") | \(.candidate.sink.callee // "-") |"
  ];

finding_count as $count |
(.elapsed_ms // 0) as $elapsed |
"## Fallow Security\n\n" +
(if $count == 0 then
  "> [!NOTE]\n> **No security candidates matched** · \($elapsed)ms"
else
  "> [!WARNING]\n> **\(plural($count; "security candidate")) matched** · \($elapsed)ms"
end) +
gate_line +
(if ((.security_findings // []) | length) > 0 then
  "\n\n| Location | Kind | Severity | Sink |\n|:---------|:-----|:---------|:-----|\n" +
  (finding_rows | join("\n")) +
  (if ((.security_findings // []) | length) > 15 then "\n\n> \(((.security_findings // []) | length) - 15) more candidates in the full report" else "" end)
else "" end) +
"\n\nTreat these as candidates for verification, not confirmed vulnerabilities."
