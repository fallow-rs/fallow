def pct(n): n | . * 10 | round / 10;
def signed(n): if n > 0 then "+\(pct(n))" elif n < 0 then "\(pct(n))" else "0.0" end;
def metric_delta(name):
  (.health_trend.metrics // []) | map(select(.name == name)) | first // null;
def suppression_docs: "https://docs.fallow.tools/configuration/suppression";
def complexity_findings: (.findings // []);
def production_findings: (.runtime_coverage.findings // []);
def production_hot_paths: (.runtime_coverage.hot_paths // []);
def production_summary: (.runtime_coverage.summary // {});
def prod_phrase(complex; prod):
  if complex > 0 and prod > 0 then
    "\(complex) complexity finding\(if complex == 1 then "" else "s" end) and \(prod) runtime coverage finding\(if prod == 1 then "" else "s" end)"
  elif complex > 0 then
    "\(complex) complexity finding\(if complex == 1 then "" else "s" end)"
  else
    "\(prod) runtime coverage finding\(if prod == 1 then "" else "s" end)"
  end;

(complexity_findings | length) as $complex |
(production_findings | length) as $prod |
(production_hot_paths | length) as $hot |
((if .health_score then
  (metric_delta("score")) as $score_delta |
  (metric_delta("dead_export_pct")) as $dead_delta |
  (metric_delta("avg_cyclomatic")) as $cx_delta |
  "> **Health: \(.health_score.grade) (\(pct(.health_score.score)))**" +
  (if $score_delta then
    " \u00b7 \(signed($score_delta.delta)) pts vs previous (\(.health_trend.compared_to.grade) \(pct(.health_trend.compared_to.score)))" +
    (if $dead_delta and $dead_delta.delta != 0 then
      " \u00b7 \($dead_delta.label | ascii_downcase) \(pct($dead_delta.current))% (\(signed($dead_delta.delta))%)" +
      (if $dead_delta.delta > 0 then " [suppress?](\(suppression_docs))" else "" end)
    else "" end) +
    (if $cx_delta and $cx_delta.delta != 0 then
      " \u00b7 \($cx_delta.label | ascii_downcase) \(pct($cx_delta.current)) (\(signed($cx_delta.delta)))"
    else "" end)
  else
    "\n> _Enable `save-snapshot: true` to track score trends over time._"
  end) +
  "\n\n"
else "" end)) +
if $prod == 0 and $hot == 0 then
  if $complex == 0 then
    "## Fallow \u2014 Code Complexity\n\n" +
    "> [!NOTE]\n> **No functions exceed complexity thresholds** \u00b7 \(.elapsed_ms)ms\n\n" +
    "\(.summary.functions_analyzed) functions analyzed (max cyclomatic: \(.summary.max_cyclomatic_threshold), max cognitive: \(.summary.max_cognitive_threshold), max CRAP: \(.summary.max_crap_threshold // 30))"
  else
    "## Fallow \u2014 Code Complexity\n\n" +
    "> [!WARNING]\n> **\(.summary.functions_above_threshold) function\(if .summary.functions_above_threshold == 1 then "" else "s" end) exceed\(if .summary.functions_above_threshold == 1 then "s" else "" end) thresholds** \u00b7 \(.elapsed_ms)ms\n\n" +
    "| File | Function | Severity | Cyclomatic | Cognitive | CRAP | Lines |\n|:-----|:---------|:---------|:-----------|:----------|:-----|:------|\n" +
    ([complexity_findings[:25][] |
      "| `\(.path):\(.line)` | `\(.name)` | \(.severity // "moderate") | \(.cyclomatic)\(if (.exceeded // "") | test("cyclomatic|both|all") then " **!**" else "" end) | \(.cognitive)\(if (.exceeded // "") | test("cognitive|both|all") then " **!**" else "" end) | \(if .crap == null then "-" else (.crap | tostring) + (if (.exceeded // "") | test("crap|all") then " **!**" else "" end) end) | \(.line_count) |"
    ] | join("\n")) +
    (if $complex > 25 then "\n\n> \($complex - 25) more \u2014 run `fallow health` locally for the full list" else "" end) +
    "\n\n**!** marks the dimension that breached.\n\n**\(.summary.files_analyzed)** files, **\(.summary.functions_analyzed)** functions analyzed (thresholds: cyclomatic > \(.summary.max_cyclomatic_threshold), cognitive > \(.summary.max_cognitive_threshold), CRAP >= \(.summary.max_crap_threshold // 30))"
  end
else
  "## Fallow \u2014 Health\n\n" +
  (if $complex == 0 and $prod == 0 then
    "> [!NOTE]\n> **No failing health findings** \u00b7 \(.elapsed_ms)ms\n\n"
  else
    "> [!WARNING]\n> **\(prod_phrase($complex; $prod))** \u00b7 \(.elapsed_ms)ms\n\n"
  end) +
  (if $complex > 0 then
    "### Complexity\n\n" +
    "| File | Function | Severity | Cyclomatic | Cognitive | CRAP | Lines |\n|:-----|:---------|:---------|:-----------|:----------|:-----|:------|\n" +
    ([complexity_findings[:25][] |
      "| `\(.path):\(.line)` | `\(.name)` | \(.severity // "moderate") | \(.cyclomatic)\(if (.exceeded // "") | test("cyclomatic|both|all") then " **!**" else "" end) | \(.cognitive)\(if (.exceeded // "") | test("cognitive|both|all") then " **!**" else "" end) | \(if .crap == null then "-" else (.crap | tostring) + (if (.exceeded // "") | test("crap|all") then " **!**" else "" end) end) | \(.line_count) |"
    ] | join("\n")) +
    (if $complex > 25 then "\n\n> \($complex - 25) more complexity findings \u2014 run `fallow health` locally for the full list" else "" end)
  else "" end) +
  (if $prod > 0 then
    (if $complex > 0 then "\n\n" else "" end) +
    "### Runtime Coverage\n\n" +
    "| File | Function | Verdict | Invocations | Confidence |\n|:-----|:---------|:--------|------------:|:-----------|\n" +
    ([production_findings[:25][] |
      "| `\(.path):\(.line)` | `\(.function)` | `\(.verdict)` | \(if .invocations == null then "\u2014" else (.invocations | tostring) end) | \(.confidence) |"
    ] | join("\n")) +
    (if $prod > 25 then "\n\n> \($prod - 25) more runtime coverage findings \u2014 run `fallow health` locally for the full list" else "" end)
  else "" end) +
  (if $hot > 0 then
    (if $complex > 0 or $prod > 0 then "\n\n" else "" end) +
    "### Hot Paths\n\n" +
    "| File | Function | Invocations | Percentile |\n|:-----|:---------|------------:|-----------:|\n" +
    ([production_hot_paths[:10][] |
      "| `\(.path):\(.line)` | `\(.function)` | \(.invocations) | \(.percentile) |"
    ] | join("\n")) +
    (if $hot > 10 then "\n\n> \($hot - 10) more hot paths in the full report" else "" end)
  else "" end) +
  (if $complex > 0 then
    "\n\n**!** marks the dimension that breached.\n\n**\(.summary.files_analyzed)** files, **\(.summary.functions_analyzed)** functions analyzed (thresholds: cyclomatic > \(.summary.max_cyclomatic_threshold), cognitive > \(.summary.max_cognitive_threshold), CRAP >= \(.summary.max_crap_threshold // 30))"
  elif $prod > 0 then
    "\n\n**\(production_summary.functions_tracked // 0)** tracked functions, **\(production_summary.functions_hit // 0)** hit, **\(production_summary.functions_unhit // 0)** unhit, **\(production_summary.functions_untracked // 0)** untracked"
  else
    "\n\nObserved **\($hot)** hot path\(if $hot == 1 then "" else "s" end) in runtime coverage."
  end)
end
