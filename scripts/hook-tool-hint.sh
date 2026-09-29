#!/bin/sh
# Print one hint line for a tool that a git hook step did not find.
# mise.toml holds the pinned version. hk.pkl calls this script.
tool="$1"
version=$(sed -nE "s#^\"?([^\"= ]*[:/])?${tool}\"? *= *\"([^\"]+)\".*#\\2#p" mise.toml 2>/dev/null | head -n 1)
echo "${tool} not found; run 'mise install' (see mise.toml) or install ${tool} ${version:-(version in mise.toml)}" >&2
