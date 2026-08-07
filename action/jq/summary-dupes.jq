def clone_rank:
  (.spread // 0) as $spread |
  ([1000000000, 1047319732, 1075000000, 1094639463, 1109873014, 1122319732, 1132843281, 1141959195, 1150000000][([$spread, 8] | min)]) as $weight |
  ((.instances // []) | sort_by([(.file // ""), (.start_line // 0)]) | first // {}) as $first |
  [-((.token_count // 0) * ((.instances // []) | length) * $weight), -$spread, -(.token_count // 0), -((.instances // []) | length), -(.line_count // 0), ($first.file // ""), ($first.start_line // 0)];
def best_clone_group:
  ((.groups // []) | sort_by(clone_rank) | first // null);
def clone_family_rank:
  (best_clone_group) as $best |
  if $best == null then [1, [], (.files // [])]
  else [0, ($best | clone_rank), (.files // [])] end;

if .stats.clone_groups == 0 then
  "## Fallow — Code Duplication\n\nNo code duplication found.\n\n*Analyzed \(.stats.total_files) files in \(.elapsed_ms)ms*"
else
  "## Fallow — Code Duplication\n\nFound **\(.stats.clone_groups) clone groups** (\(.stats.clone_instances) instances) across \(.stats.files_with_clones) files in \(.elapsed_ms)ms\n\n" +
  "| Metric | Value |\n|--------|-------|\n" +
  "| Files analyzed | \(.stats.total_files) |\n" +
  "| Files with clones | \(.stats.files_with_clones) |\n" +
  "| Clone groups | \(.stats.clone_groups) |\n" +
  "| Clone instances | \(.stats.clone_instances) |\n" +
  "| Duplicated lines | \(.stats.duplicated_lines) / \(.stats.total_lines) (\((.stats.duplication_percentage // 0) | . * 10 | round / 10)%) |\n" +
  "\n<details>\n<summary>View details</summary>\n\n" +
  (if (.clone_families | length) > 0 then
    ((.clone_families // []) | sort_by(clone_family_rank)) as $families |
    "**Clone Families (\($families | length))**\n\n" +
    ([$families[:15][] |
      (best_clone_group) as $best |
      "- **\(.files[:3] | join(", "))\(if (.files | length) > 3 then " (+\((.files | length) - 3) more)" else "" end)** — \(.total_duplicated_lines) lines, \(.groups | length) groups" +
      (if $best != null and (($best.instances // []) | length) > 0 then
        "\n  - " + ([($best.instances // [])[] | "`\(.file):\(.start_line)-\(.end_line)`"] | join(", "))
      else "" end) +
      (if (.suggestions | length) > 0 then
        "\n" + ([.suggestions[] | "  - \(.description) (~\(.estimated_savings) lines)"] | join("\n"))
      else "" end)
    ] | join("\n")) +
    (if ($families | length) > 15 then "\n- *... and \(($families | length) - 15) more families*" else "" end)
  else
    ((.clone_groups // []) | sort_by(clone_rank)) as $sorted |
    ([$sorted[:20][] |
      ([(.instances // [])[] | "`\(.file):\(.start_line)-\(.end_line)`"] | join(", ")) as $locs |
      "- **\(.line_count) lines, \(.token_count) tokens**, \($locs)"
    ] | join("\n")) +
    (if (.clone_groups | length) > 20 then "\n- *... and \((.clone_groups | length) - 20) more groups*" else "" end)
  end) +
  "\n\n</details>"
end
