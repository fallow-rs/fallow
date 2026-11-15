#!/usr/bin/env bash
# Adds a swap file on a Linux CI runner and prints the memory and swap after.
# When the directory of the file does not exist, or its file system has less
# free space than the swap size plus a reserve, the script prints a warning and
# adds no swap: a missing swap file must not fail the job.
#
# Usage: bash scripts/ci-add-swap.sh <swap file> <size in GiB>
set -u

RESERVE_GIB=4

if [ "$#" -ne 2 ] || ! [[ "$2" =~ ^[1-9][0-9]*$ ]]; then
  echo "usage: bash scripts/ci-add-swap.sh <swap file> <size in GiB>" >&2
  exit 2
fi

file="$1"
size_gib="$2"
dir="$(dirname "$file")"

warn() {
  echo "::warning::No extra swap: $1"
  exit 0
}

[ -d "$dir" ] || warn "$dir does not exist"
free_gib="$(df -Pk "$dir" | awk 'NR == 2 { print int($4 / 1024 / 1024) }')"
[ "$free_gib" -ge $((size_gib + RESERVE_GIB)) ] ||
  warn "$dir has ${free_gib} GiB free, the swap file needs $((size_gib + RESERVE_GIB)) GiB"

sudo_cmd=""
[ "$(id -u)" -eq 0 ] || sudo_cmd="sudo -n"

drop() {
  $sudo_cmd rm -f "$file"
  warn "$1"
}

$sudo_cmd fallocate -l "${size_gib}G" "$file" || drop "fallocate failed for $file"
$sudo_cmd chmod 600 "$file"
$sudo_cmd mkswap "$file" >/dev/null || drop "mkswap failed for $file"
$sudo_cmd swapon "$file" || drop "swapon failed for $file"
free -m
