#!/usr/bin/env bash

# Escapes a workflow command message, so a newline or a percent sign in an
# error message cannot end the command or start a new one.
escape_command_data() {
  local value=$1
  value=${value//'%'/'%25'}
  value=${value//$'\r'/'%0D'}
  value=${value//$'\n'/'%0A'}
  printf '%s' "$value"
}
