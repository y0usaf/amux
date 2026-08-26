#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "--version" ]]; then
  printf '%s\n' 'pi demo 0.1.0'
  exit 0
fi

render_initial() {
  printf '\033[2J\033[H'
  printf '\033[1;36mAcme CLI\033[0m  synthetic demo agent\r\n'
  printf '\033[90m────────────────────────────────────────────────────────────────────────\033[0m\r\n'
  printf 'Workspace  \033[1m/demo/project\033[0m\r\n'
  printf 'Status     ready\r\n\r\n'
  printf '\033[33m›\033[0m '
}

render_result() {
  printf '\033[2J\033[H'
  printf '\033[1;36mAcme CLI\033[0m  synthetic demo agent\r\n'
  printf '\033[90m────────────────────────────────────────────────────────────────────────\033[0m\r\n'
  printf '\033[90mYou\033[0m  Please inspect this project and suggest the next change.\r\n\r\n'
  printf '\033[32m✓\033[0m Read README.md\r\n'
  printf '\033[32m✓\033[0m Read src/main.rs\r\n\r\n'
  printf '\033[1mFocused next step\033[0m\r\n'
  printf 'Add a small command parser and a focused test for JSON output.\r\n\r\n'
  printf '\033[90m2 files inspected · no network · demo fixture\033[0m\r\n'
  printf '\033[33m›\033[0m '
}

render_initial
IFS= read -r _request || exit 0
sleep 1
render_result
while IFS= read -r _line; do :; done
