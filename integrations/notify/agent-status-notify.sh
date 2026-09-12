#!/usr/bin/env bash
# Records agent status for the zellij agent-status plugins.
#
# Usage: agent-status-notify <source> <status> [decision]
#   source: short producer name, e.g. "jetski" (recorded for debugging)
#   status: working | blocked | done | idle | unknown | error
#
# State lives on disk, one file per pane, rather than being pushed into
# zellij over a pipe. The plugins poll it, so this script needs neither
# zellij nor a subprocess and cannot block the calling agent's hook.
#
# The location is dictated by zellij's plugin sandbox. Plugins get exactly
# four preopened directories -- /host, /data, /cache and /tmp -- so $HOME is
# unreachable from inside the wasm guest. Guest /tmp is the host's
# ZELLIJ_TMP_DIR, defined in zellij-utils/src/consts.rs as
# temp_dir()/zellij-<uid>, so this is the one path both sides can name:
#
#   host:  ${TMPDIR:-/tmp}/zellij-<uid>/agent-status/<session>/<pane_id>
#   guest: /tmp/agent-status/<session>/<pane_id>
#
# Contents: "<status> <epoch_seconds> <source>".
# `idle` removes the file; a missing file means "idle/no supported agent here".
set -euo pipefail

source_name="${1:?usage: agent-status-notify <source> <status> [decision]}"
status="${2:?usage: agent-status-notify <source> <status> [decision]}"
decision="${3:-}"

# Emit valid JSON on stdout first, so hook runners (Jetski, Gemini CLI,
# Claude Code) get a well-formed response even if the write below fails.
if [ -n "$decision" ]; then
  printf '{"decision":"%s"}\n' "$decision"
else
  printf '{}\n'
fi

[ -n "${ZELLIJ_PANE_ID:-}" ] || exit 0
[ -n "${ZELLIJ_SESSION_NAME:-}" ] || exit 0

zellij_tmp="${TMPDIR:-/tmp}/zellij-$(id -u)"
state_dir="${zellij_tmp}/agent-status/${ZELLIJ_SESSION_NAME}"
pane_id="${ZELLIJ_PANE_ID#terminal_}"
target="${state_dir}/${pane_id}"

if [ "$status" = "idle" ]; then
  rm -f -- "$target"
  exit 0
fi

mkdir -p -- "$state_dir"

# Write to a temp file in the same directory, then rename, so a reader
# never observes a half-written record.
tmp="${target}.$$"
printf '%s %s %s\n' "$status" "$(date +%s)" "$source_name" >"$tmp"
mv -f -- "$tmp" "$target"
