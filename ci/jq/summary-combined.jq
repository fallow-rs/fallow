# GitLab variant of summary-combined.jq
# Differences from GitHub: no > [!NOTE] / > [!TIP] callouts

def count(obj; key): obj | if . then .[key] // 0 else 0 end;
def pct(n): n | . * 10 | round / 10;
def signed(n): if n > 0 then "+\(pct(n))" elif n < 0 then "\(pct(n))" else "0.0" end;
def rel_path: split("/") | if length > 3 then .[-3:] | join("/") else join("/") end;
def prefix: $ENV.PREFIX // "";
def project_url: $ENV.CI_PROJECT_URL // "";
def sha: $ENV.CI_COMMIT_SHA // "";
def file_link(path; start; end_line):
  (path | rel_path) as $display |
  if (project_url | length) > 0 and (sha | length) > 0 then
    "[`\($display):\(start)-\(end_line)`](\(project_url)/-/blob/\(sha)/\(prefix)\(path)#L\(start)-\(end_line))"
  else "`\($display):\(start)-\(end_line)`" end;
def dead_code_docs: "https://docs.fallow.tools/explanations/dead-code";
def docs(anchor): dead_code_docs + "#" + anchor;
def health_docs: "https://docs.fallow.tools/explanations/health";
def dupes_docs: "https://docs.fallow.tools/explanations/duplication";
def suppression_docs: "https://docs.fallow.tools/configuration/suppression";
def metric_delta(name):
  (.health.health_trend.metrics // []) | map(select(.name == name)) | first // null;
def exceeded_priority:
  (.exceeded // "") as $e |
  if $e == "all" then 5
  elif $e == "cyclomatic_crap" or $e == "cognitive_crap" then 4
  elif $e == "crap" then 3
  elif $e == "both" then 2
  elif $e == "cyclomatic" or $e == "cognitive" then 1
  else 0 end;
def severity_priority:
  (.severity // "") as $s |
  if $s == "critical" then 3 elif $s == "high" then 2 elif $s == "moderate" then 1 else 0 end;
def ranked_health_findings:
  (.health.findings // [])
  | sort_by([exceeded_priority, severity_priority, (.crap != null), (.cyclomatic // 0), (.cognitive // 0), (.line_count // 0)])
  | reverse;
def prod_failing_findings:
  (.health.runtime_coverage.findings // [])
  | map(select(.verdict == "safe_to_delete" or .verdict == "review_required" or .verdict == "low_traffic"));
def prod_advisory_findings:
  (.health.runtime_coverage.findings // [])
  | map(select(.verdict != "safe_to_delete" and .verdict != "review_required" and .verdict != "low_traffic"));
def prod_hot_paths: (.health.runtime_coverage.hot_paths // []);
# When runtime-coverage post-processing flipped the verdict to
# `hot-path-touched`, the hot_paths array is the MR-scoped subset
# (line-overlap match against the supplied --diff-file). Render the
# header text so reviewers see "touched" framing rather than the
# project-wide "top hot paths" framing.
def prod_hot_paths_touched: (.health.runtime_coverage.verdict // "") == "hot-path-touched";
def prod_hot_path_label($n):
  (if prod_hot_paths_touched then "hot path\(if $n == 1 then "" else "s" end) touched" else "hot path\(if $n == 1 then "" else "s" end)" end);

(count(.check; "total_issues")) as $check |
(count(.dupes.stats; "clone_groups")) as $dupes |
(count(.health.summary; "functions_above_threshold")) as $complex |
(($complex) + (prod_failing_findings | length)) as $health |
(prod_failing_findings | length) as $prod_failing |
(prod_advisory_findings | length) as $prod_advisory |
(prod_hot_paths | length) as $hot_paths |
($check + $dupes + $health) as $total |
(.health.vital_signs // {}) as $vitals |
(.health.summary // {}) as $summary |
(.dupes.stats // {}) as $dupes_stats |

# Health delta header (only when --score is present)
(if .health.health_score then
  (metric_delta("score")) as $score_delta |
  (metric_delta("dead_export_pct")) as $dead_delta |
  (metric_delta("avg_cyclomatic")) as $cx_delta |
  "> :chart_with_upwards_trend: **Health: \(.health.health_score.grade) (\(pct(.health.health_score.score)))**" +
  (if $score_delta then
    " \u00b7 \(signed($score_delta.delta)) pts vs previous (\(.health.health_trend.compared_to.grade) \(pct(.health.health_trend.compared_to.score)))" +
    (if $dead_delta and $dead_delta.delta != 0 then
      " \u00b7 \($dead_delta.label | ascii_downcase) \(pct($dead_delta.current))% (\(signed($dead_delta.delta))%)" +
      (if $dead_delta.delta > 0 then " [suppress?](\(suppression_docs))" else "" end)
    else "" end) +
    (if $cx_delta and $cx_delta.delta != 0 then
      " \u00b7 \($cx_delta.label | ascii_downcase) \(pct($cx_delta.current)) (\(signed($cx_delta.delta)))"
    else "" end)
  else
    "\n> _Set `FALLOW_SAVE_SNAPSHOT: \"true\"` to track score trends over time._"
  end) +
  "\n\n"
else "" end) +

if $total == 0 then
  "# :seedling: Fallow\n\n" +
  (if $prod_advisory > 0 or $hot_paths > 0 then
    "> **No blocking issues found**\n\n" +
    ":white_check_mark: No code issues \u00b7 :white_check_mark: No duplication \u00b7 :white_check_mark: No blocking health findings" +
    (if $prod_advisory > 0 then " \u00b7 :information_source: **\($prod_advisory)** runtime coverage advisory finding\(if $prod_advisory == 1 then "" else "s" end)" else "" end) +
    (if $hot_paths > 0 then " \u00b7 :eyes: **\($hot_paths)** \(prod_hot_path_label($hot_paths))" else "" end)
  else
    "> **No issues found**\n\n" +
    ":white_check_mark: No code issues \u00b7 :white_check_mark: No duplication \u00b7 :white_check_mark: No complex functions"
  end) +
  (if $vitals.maintainability_avg then
    "\n\n| Metric | Value |\n|:-------|------:|\n" +
    "| [Maintainability](\(health_docs)#maintainability-index-mi) | **\(pct($vitals.maintainability_avg))** / 100 |\n"
  else "" end)
else
  "# :seedling: Fallow\n\n" +

  # One-line status
  (if $check > 0 then ":warning: **\($check)** code \(if $check == 1 then "issue" else "issues" end)" else ":white_check_mark: No code issues" end) +
  " \u00b7 " +
  (if $dupes > 0 then ":warning: **\($dupes)** clone \(if $dupes == 1 then "group" else "groups" end)" else ":white_check_mark: No duplication" end) +
  " \u00b7 " +
  (if $health > 0 then ":warning: **\($health)** health \(if $health == 1 then "finding" else "findings" end)" else ":white_check_mark: No blocking health findings" end) +
  (if $prod_advisory > 0 then " \u00b7 :information_source: **\($prod_advisory)** coverage advisory finding\(if $prod_advisory == 1 then "" else "s" end)" else "" end) +
  (if $hot_paths > 0 then " \u00b7 :eyes: **\($hot_paths)** \(prod_hot_path_label($hot_paths))" else "" end) +
  "\n\n" +

  # Pointer to inline comments
  (if $check > 0 or $dupes > 0 or $health > 0 then
    "See inline review comments for per-finding details.\n\n"
  else "" end) +

  # Code issues breakdown
  (if $check > 0 then
    "<details>\n<summary><strong><a href=\"\(dead_code_docs)\">Code issues</a> (\($check))</strong></summary>\n\n" +
    "| Category | Count |\n|:---------|------:|\n" +
    ([
      (if (.check.unused_files | length) > 0 then "| [Unused files](\(docs("unused-files"))) | \(.check.unused_files | length) |" else null end),
      (if (.check.unused_exports | length) > 0 then "| [Unused exports](\(docs("unused-exports"))) | \(.check.unused_exports | length) |" else null end),
      (if (.check.unused_types | length) > 0 then "| [Unused types](\(docs("unused-types"))) | \(.check.unused_types | length) |" else null end),
      (if (.check.private_type_leaks | length) > 0 then "| [Private type leaks](\(docs("private-type-leaks"))) | \(.check.private_type_leaks | length) |" else null end),
      (if (.check.unused_dependencies | length) > 0 then "| [Unused dependencies](\(docs("unused-dependencies"))) | \(.check.unused_dependencies | length) |" else null end),
      (if (.check.unused_dev_dependencies | length) > 0 then "| [Unused devDependencies](\(docs("unused-dependencies"))) | \(.check.unused_dev_dependencies | length) |" else null end),
      (if (.check.unused_optional_dependencies | length) > 0 then "| [Unused optionalDependencies](\(docs("unused-dependencies"))) | \(.check.unused_optional_dependencies | length) |" else null end),
      (if (.check.unused_enum_members | length) > 0 then "| [Unused enum members](\(docs("unused-enum-members"))) | \(.check.unused_enum_members | length) |" else null end),
      (if (.check.unused_class_members | length) > 0 then "| [Unused class members](\(docs("unused-class-members"))) | \(.check.unused_class_members | length) |" else null end),
      (if (.check.unresolved_imports | length) > 0 then "| [Unresolved imports](\(docs("unresolved-imports"))) | \(.check.unresolved_imports | length) |" else null end),
      (if (.check.unlisted_dependencies | length) > 0 then "| [Unlisted dependencies](\(docs("unlisted-dependencies"))) | \(.check.unlisted_dependencies | length) |" else null end),
      (if (.check.duplicate_exports | length) > 0 then "| [Duplicate exports](\(docs("duplicate-exports"))) | \(.check.duplicate_exports | length) |" else null end),
      (if (.check.circular_dependencies | length) > 0 then "| [Circular dependencies](\(docs("circular-dependencies"))) | \(.check.circular_dependencies | length) |" else null end),
      (if (.check.boundary_violations | length) > 0 then "| [Boundary violations](\(docs("boundary-violations"))) | \(.check.boundary_violations | length) |" else null end),
      (if (.check.type_only_dependencies | length) > 0 then "| [Type-only dependencies](\(docs("type-only-dependencies"))) | \(.check.type_only_dependencies | length) |" else null end),
      (if (.check.test_only_dependencies | length) > 0 then "| [Test-only dependencies](\(docs("test-only-dependencies"))) | \(.check.test_only_dependencies | length) |" else null end),
      (if (.check.stale_suppressions | length) > 0 then "| [Stale suppressions](\(docs("stale-suppressions"))) | \(.check.stale_suppressions | length) |" else null end)
    ] | map(select(. != null)) | join("\n")) +
    "\n\n</details>\n\n"
  else "" end) +

  # Duplication breakdown
  (if $dupes > 0 then
    ((.dupes.clone_groups // []) | sort_by([(.line_count // 0), (.token_count // 0)]) | reverse) as $groups |
    ($dupes_stats.files_with_clones // 0) as $files_with_clones |
    "<details>\n<summary><strong><a href=\"\(dupes_docs)\">Duplication</a> (\($dupes) \(if $dupes == 1 then "group" else "groups" end) · \($dupes_stats.duplicated_lines // 0) lines · \(pct($dupes_stats.duplication_percentage // 0))%)</strong></summary>\n\n" +
    "| Locations | Lines | Tokens |\n|:----------|------:|-------:|\n" +
    ([$groups[:5][] |
      ([(.instances // [])[] | file_link(.file; .start_line; .end_line)] | join("<br>")) as $locs |
      "| \($locs) | \(.line_count) | \(.token_count) |"
    ] | join("\n")) +
    (if $dupes > 5 then "\n\n*… and \($dupes - 5) more groups.*" else "" end) +
    "\n\nAcross \($files_with_clones) \(if $files_with_clones == 1 then "file" else "files" end).\n\n</details>\n\n"
  else "" end) +

  # Complexity breakdown
  (if $complex > 0 then
    (ranked_health_findings) as $findings |
    (($summary.max_crap_threshold != null) or ($findings | map(.crap) | any(. != null))) as $show_crap |
    ($summary.max_cyclomatic_threshold // "default") as $cyc_t |
    ($summary.max_cognitive_threshold // "default") as $cog_t |
    ($summary.max_crap_threshold // "default") as $crap_t |
    "<details>\n<summary><strong><a href=\"\(health_docs)#complexity-metrics\">Complexity</a> (\($complex) \(if $complex == 1 then "function" else "functions" end) above threshold)</strong></summary>\n\n" +
    "| File | Function | Severity | [Cyclomatic](\(health_docs)#cyclomatic-complexity) | [Cognitive](\(health_docs)#cognitive-complexity)\(if $show_crap then " | [CRAP](\(health_docs)#crap-score)" else "" end) | Lines |\n|:-----|:---------|:---------|----------:|---------:\(if $show_crap then "|-----:" else "" end)|------:|\n" +
    ([$findings[:5][] |
      "| `\(.path | rel_path):\(.line)` | `\(.name)` | \(.severity // "moderate") | \(.cyclomatic)\(if (.exceeded // "") | test("cyclomatic|both|all") then " **!**" else "" end) | \(.cognitive)\(if (.exceeded // "") | test("cognitive|both|all") then " **!**" else "" end)\(if $show_crap then " | \(if .crap == null then "-" else (.crap | tostring) + (if (.exceeded // "") | test("crap|all") then " **!**" else "" end) end)" else "" end) | \(.line_count) |"
    ] | join("\n")) +
    "\n\n**\($summary.files_analyzed // "unknown")** files, **\($summary.functions_analyzed // "unknown")** functions analyzed (thresholds: cyclomatic > \($cyc_t), cognitive > \($cog_t)\(if $show_crap then ", CRAP >= \($crap_t)" else "" end))\n\n</details>\n\n"
  else "" end) +

  (if $prod_failing > 0 or $prod_advisory > 0 or $hot_paths > 0 then
    "<details>\n<summary><strong><a href=\"\(health_docs)#runtime-coverage\">Runtime coverage</a> (\($prod_failing + $prod_advisory) finding\(if ($prod_failing + $prod_advisory) == 1 then "" else "s" end)\(if $hot_paths > 0 then ", \($hot_paths) \(prod_hot_path_label($hot_paths))" else "" end))</strong></summary>\n\n" +
    (if $prod_failing + $prod_advisory > 0 then
      "| File | Function | Verdict | Invocations | Confidence |\n|:-----|:---------|:--------|------------:|:-----------|\n" +
      ([(.health.runtime_coverage.findings // [])[:5][] |
        "| `\(.path | rel_path):\(.line)` | `\(.function)` | `\(.verdict)` | \(if .invocations == null then "\u2014" else (.invocations | tostring) end) | \(.confidence) |"
      ] | join("\n")) +
      (if $hot_paths > 0 then "\n\n" else "" end)
    else "" end) +
    (if $hot_paths > 0 then
      "| File | Function | Invocations | Percentile |\n|:-----|:---------|------------:|-----------:|\n" +
      ([prod_hot_paths[:5][] |
        "| `\(.path | rel_path):\(.line)` | `\(.function)` | \(.invocations) | \(.percentile) |"
      ] | join("\n")) +
      "\n\n"
    else "" end) +
    "</details>\n\n"
  else "" end) +

  # Vital signs
  (if $vitals | length > 0 then
    # Compute scoped maintainability from filtered file_scores (differs from codebase avg when --changed-since is active)
    ((.health.file_scores // []) | if length > 0 then (map(.maintainability_index) | add / length | . * 10 | round / 10) else null end) as $scoped_maint |
    "#### [Codebase health](\(health_docs))\n\n" +
    "| Metric | Value |\n|:-------|------:|\n" +
    (if $vitals.maintainability_avg then "| [Maintainability](\(health_docs)#maintainability-index-mi) | **\(pct($vitals.maintainability_avg))** / 100 |\n" else "" end) +
    (if $scoped_maint != null and $scoped_maint != pct($vitals.maintainability_avg // 0) then
      "| [Maintainability](\(health_docs)#maintainability-index-mi) (changed files) | **\($scoped_maint)** / 100 |\n"
    else "" end) +
    (if $vitals.avg_cyclomatic then "| [Avg complexity](\(health_docs)#cyclomatic-complexity) | \(pct($vitals.avg_cyclomatic)) |\n" else "" end) +
    "\n"
  else "" end) +

  # Conditional tips based on which categories were found
  (if ((.check.unused_exports // []) + (.check.unused_dependencies // []) + (.check.unused_enum_members // [])) | length > 0 then
    "> :bulb: Run `fallow fix --dry-run` to preview auto-fixes." +
    (if (.check.unused_exports // []) | length > 0 then
      " Add [`/** @public */`](https://docs.fallow.tools/configuration/suppression) above exports to preserve them."
    else "" end)
  else "" end)
end
