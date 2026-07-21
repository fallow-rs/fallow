def san: tostring | gsub("%"; "%25") | gsub("\r"; "%0D") | gsub("\n"; "%0A");
def prop: select(type == "string" and length > 0) | san | gsub(","; "%2C") | gsub(":"; "%3A");
def pval: san | gsub(","; "%2C") | gsub(":"; "%3A");
def n(default): if type == "number" then . else default end;
def nl: "%0A";
(.summary.max_cyclomatic_threshold // 20) as $cyc_t |
(.summary.max_cognitive_threshold // 15) as $cog_t |
(.summary.max_crap_threshold // 30) as $crap_t |
[
  (.findings[]? |
    (.severity // "moderate") as $sev |
    (if $sev == "critical" then "error" else "warning" end) as $level |
    (if .crap != null then "  \u2022 CRAP: \(.crap | san) (threshold: \($crap_t | san))\(nl)" else "" end) as $crap_line |
    if .exceeded == "crap" or .exceeded == "cyclomatic_crap" or .exceeded == "cognitive_crap" or .exceeded == "all" then
      "::\($level) file=\(.path | prop),line=\(.line | n(1)),col=\((.col | n(0)) + 1),title=High CRAP score (\($sev | pval))::Function '\(.name | san)' has a CRAP score of \(.crap | san) (threshold: \($crap_t | san)).\(nl)\(nl)  \u2022 Severity: \($sev | san)\(nl)  \u2022 Cyclomatic: \(.cyclomatic | san)\(nl)  \u2022 Cognitive: \(.cognitive | san)\(nl)\($crap_line)  \u2022 Lines: \(.line_count | san)\(nl)\(nl)CRAP combines complexity with coverage: high CRAP means changes here carry high risk.\(nl)Consider adding tests, simplifying the function, or both."
    elif .exceeded == "both" then
      "::\($level) file=\(.path | prop),line=\(.line | n(1)),col=\((.col | n(0)) + 1),title=High complexity (\($sev | pval))::Function '\(.name | san)' exceeds both complexity thresholds:\(nl)\(nl)  \u2022 Severity: \($sev | san)\(nl)  \u2022 Cyclomatic: \(.cyclomatic | san) (threshold: \($cyc_t | san))\(nl)  \u2022 Cognitive: \(.cognitive | san) (threshold: \($cog_t | san))\(nl)\($crap_line)  \u2022 Lines: \(.line_count | san)\(nl)\(nl)Consider splitting this function into smaller, focused functions."
    elif .exceeded == "cyclomatic" then
      "::\($level) file=\(.path | prop),line=\(.line | n(1)),col=\((.col | n(0)) + 1),title=High cyclomatic complexity (\($sev | pval))::Function '\(.name | san)' has \(.cyclomatic | san) code paths (threshold: \($cyc_t | san)).\(nl)\(nl)  \u2022 Severity: \($sev | san)\(nl)  \u2022 Cyclomatic: \(.cyclomatic | san)\(nl)  \u2022 Cognitive: \(.cognitive | san)\(nl)\($crap_line)  \u2022 Lines: \(.line_count | san)\(nl)\(nl)High cyclomatic complexity means many branches to test.\(nl)Consider extracting conditionals or using early returns."
    else
      "::\($level) file=\(.path | prop),line=\(.line | n(1)),col=\((.col | n(0)) + 1),title=High cognitive complexity (\($sev | pval))::Function '\(.name | san)' is hard to understand (cognitive: \(.cognitive | san), threshold: \($cog_t | san)).\(nl)\(nl)  \u2022 Severity: \($sev | san)\(nl)  \u2022 Cyclomatic: \(.cyclomatic | san)\(nl)  \u2022 Cognitive: \(.cognitive | san)\(nl)\($crap_line)  \u2022 Lines: \(.line_count | san)\(nl)\(nl)High cognitive complexity means deeply nested or interleaved logic.\(nl)Consider flattening control flow or extracting helper functions."
    end),
  (.runtime_coverage.findings[]? |
    (if .verdict == "coverage_unavailable" then "notice" else "warning" end) as $level |
    (if .invocations == null then "\u2014" else (.invocations | tostring) end) as $invocations |
    (if .evidence.untracked_reason then (.evidence.v8_tracking + " (" + .evidence.untracked_reason + ")") else .evidence.v8_tracking end) as $tracking |
    "::\($level) file=\(.path | prop),line=\(.line | n(1)),title=Runtime coverage (\(.verdict | pval))::Function '\(.function | san)' is flagged by runtime coverage.\(nl)\(nl)  \u2022 Verdict: \(.verdict | san)\(nl)  \u2022 Invocations: \($invocations | san)\(nl)  \u2022 Confidence: \(.confidence | san)\(nl)  \u2022 Static: \(.evidence.static_status | san)\(nl)  \u2022 Tests: \(.evidence.test_coverage | san)\(nl)  \u2022 V8: \($tracking | san)\(nl)\(nl)\(if .actions | length > 0 then .actions[0].description | san else "Review the runtime evidence before changing this path." end)"),
  ((.targets // .refactoring_targets // [])[:5][]? |
    "::notice file=\(.path | prop),title=Refactoring target (\(.effort | pval) effort)::Priority: \(.priority | san) | Confidence: \(.confidence | san)\(nl)\(nl)\(.recommendation | san)\(nl)\(nl)\(if .factors then (.factors | map("  \u2022 \(.metric | san): \((.detail // .value) | san)") | join(nl)) else "" end)")
] | .[]
