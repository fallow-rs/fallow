# Legacy renderer for fallow before 3.4.2. The action runs this file only when
# the binary has no `fallow report`. Do not add new issue kinds: the binaries
# that use this file do not emit them. Newer binaries render natively.

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
# Name what this listing does not show, so the corpus total in the label is
# never read as the size of the list below it. Two caps stack: `--top` before
# the envelope was built, and this renderer's own display limit after. Empty
# when the listing is complete, so an untruncated run stays byte-identical.
def omission_tail(withheld; capped_by_top; noun):
  if withheld <= 0 then ""
  elif capped_by_top == 0 then "\n- *... and \(withheld) more \(noun)*"
  else "\n- *... and \(withheld) more \(noun), \(capped_by_top) of them withheld by a display limit before this report*"
  end;

if .stats.clone_groups == 0 then
  "## Fallow - Code Duplication\n\nNo code duplication found.\n\n*Analyzed \(.stats.total_files) files in \(.elapsed_ms)ms*"
else
  "## Fallow - Code Duplication\n\nFound **\(.stats.clone_groups) clone groups** (\(.stats.clone_instances) instances) across \(.stats.files_with_clones) files in \(.elapsed_ms)ms\n\n" +
  "| Metric | Value |\n|--------|-------|\n" +
  "| Files analyzed | \(.stats.total_files) |\n" +
  "| Files with clones | \(.stats.files_with_clones) |\n" +
  "| Clone groups | \(.stats.clone_groups) |\n" +
  "| Clone instances | \(.stats.clone_instances) |\n" +
  "| Duplicated lines | \(.stats.duplicated_lines) / \(.stats.total_lines) (\((.stats.duplication_percentage // 0) | . * 10 | round / 10)%) |\n" +
  "\n<details>\n<summary>View details</summary>\n\n" +
  (if (.clone_families | length) > 0 then
    ((.clone_families // []) | sort_by(clone_family_rank)) as $families |
    # `$families` is what the envelope carries, which `--top` may already have
    # truncated; `clone_families_omitted` counts what it withheld. The corpus
    # total is the sum, so this label never reports a capped array as the whole
    # measurement while the header two lines up reports the corpus.
    ($families | length) as $families_listed |
    ((.clone_families_omitted // 0) | tonumber) as $families_omitted |
    ($families_listed + $families_omitted) as $families_total |
    ([$families_listed, 15] | min) as $families_rendered |
    "**Clone Families (\($families_total))**\n\n" +
    ([$families[:15][] |
      (best_clone_group) as $best |
      "- **\(.files[:3] | join(", "))\(if (.files | length) > 3 then " (+\((.files | length) - 3) more)" else "" end)** - \(.total_duplicated_lines) lines, \(.groups | length) groups" +
      (if $best != null and (($best.instances // []) | length) > 0 then
        "\n  - " + ([($best.instances // [])[] | "`\(.file):\(.start_line)-\(.end_line)`"] | join(", "))
      else "" end) +
      (if (.suggestions | length) > 0 then
        "\n" + ([.suggestions[] | "  - \(.description) (~\(.estimated_savings) lines)"] | join("\n"))
      else "" end)
    ] | join("\n")) +
    omission_tail($families_total - $families_rendered; $families_omitted; "families")
  else
    ((.clone_groups // []) | sort_by(clone_rank)) as $sorted |
    ($sorted | length) as $groups_listed |
    ((.clone_groups_omitted // 0) | tonumber) as $groups_omitted |
    ($groups_listed + $groups_omitted) as $groups_total |
    ([$groups_listed, 20] | min) as $groups_rendered |
    ([$sorted[:20][] |
      ([(.instances // [])[] | "`\(.file):\(.start_line)-\(.end_line)`"] | join(", ")) as $locs |
      "- **\(.line_count) lines, \(.token_count) tokens**, \($locs)"
    ] | join("\n")) +
    omission_tail($groups_total - $groups_rendered; $groups_omitted; "groups")
  end) +
  "\n\n</details>"
end
