def san: tostring | gsub("%"; "%25") | gsub("\r"; "%0D") | gsub("\n"; "%0A");
def prop: select(type == "string" and length > 0) | san | gsub(","; "%2C") | gsub(":"; "%3A");
def n(default): if type == "number" then . else default end;
def nl: "%0A";
def short_path: san | split("/") | if length > 3 then .[-3:] | join("/") else join("/") end;
[
  (.clone_groups // [])[] | . as $group |
    ($group.instances | length) as $count |
    .instances[]? | . as $inst |
      ($group.instances | map(select(. != $inst))) as $others |
      "::warning file=\(.file | prop),line=\(.start_line | n(1)),endLine=\(.end_line | n(1)),col=\((.start_col | n(0)) + 1),title=Code duplication::\($group.line_count | san) duplicated lines (\($group.token_count | san) tokens)\(nl)\(nl)\($count) instances found. Also in:\($others | map(nl + "  \u2192 " + (.file | short_path) + ":" + (.start_line | san) + "-" + (.end_line | san)) | join(""))\(nl)\(nl)Extract a shared function to eliminate this duplication."
] | .[]
