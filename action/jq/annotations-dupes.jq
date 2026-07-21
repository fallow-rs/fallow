def san: gsub("\n"; " ") | gsub("\r"; " ") | gsub("%"; "%25");
def prop: gsub("%"; "%25") | gsub("\r"; "%0D") | gsub("\n"; "%0A") | gsub(","; "%2C") | gsub(":"; "%3A");
def nl: "%0A";
def short_path: split("/") | if length > 3 then .[-3:] | join("/") else join("/") end;
[
  (.clone_groups // [])[] | . as $group |
    ($group.instances | length) as $count |
    .instances[]? | . as $inst |
      ($group.instances | map(select(. != $inst))) as $others |
      "::warning file=\(.file | prop),line=\(.start_line),endLine=\(.end_line),col=\(.start_col + 1),title=Code duplication::\($group.line_count) duplicated lines (\($group.token_count) tokens)\(nl)\(nl)\($count) instances found. Also in:\($others | map(nl + "  \u2192 " + (.file | short_path) + ":" + (.start_line | tostring) + "-" + (.end_line | tostring)) | join(""))\(nl)\(nl)Extract a shared function to eliminate this duplication."
] | .[]
