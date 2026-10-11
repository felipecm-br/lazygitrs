#!/usr/bin/env bash
# agent-note.sh — Bridge between Lazygitrs review notes and Tmux AI agent panes.
# Injects prompts/notes directly into the agent pane associated with the target worktree.
set -euo pipefail

TARGET_DIR="${1:-$PWD}"
PROMPT="${2:-}"
CURRENT_PANE="${TMUX_PANE:-}"

# Fallback: if PROMPT is empty and stdin is provided, read stdin
if [[ -z "$PROMPT" ]] && [[ ! -t 0 ]]; then
    PROMPT=$(cat)
fi

if [[ -z "$PROMPT" ]]; then
    echo "Usage: agent-note.sh <workspace_path> <prompt>" >&2
    exit 1
fi

# Detect matching pane in tmux whose current path matches TARGET_DIR
MATCHING_PANE=""
while IFS=$'\t' read -r p_id p_path p_cmd; do
    # Skip current lazygitrs popup pane
    if [[ -n "$CURRENT_PANE" && "$p_id" == "$CURRENT_PANE" ]] || [[ "$p_cmd" == "lazygitrs" ]]; then
        continue
    fi
    # Check if pane current path matches TARGET_DIR or repo root
    if [[ "$p_path" == "$TARGET_DIR"* ]] || [[ "$TARGET_DIR" == "$p_path"* ]]; then
        MATCHING_PANE="$p_id"
        break
    fi
done < <(tmux list-panes -a -F '#{pane_id}	#{pane_current_path}	#{pane_current_command}' 2>/dev/null || true)

if [[ -n "$MATCHING_PANE" ]]; then
    # Safely load into buffer and paste to avoid escaping/multiline truncation
    tmux set-buffer -b agent-note-buf "$PROMPT" 2>/dev/null || true
    tmux paste-buffer -b agent-note-buf -t "$MATCHING_PANE" -d 2>/dev/null || true
    tmux send-keys -t "$MATCHING_PANE" Enter 2>/dev/null || true

    if command -v acpd-cli >/dev/null 2>&1; then
        acpd-cli msg "󰊢 Note delivered to agent ($MATCHING_PANE)" >/dev/null 2>&1 || true
    else
        tmux display-message "󰊢 Note delivered to agent ($MATCHING_PANE)" 2>/dev/null || true
    fi
else
    if command -v acpd-cli >/dev/null 2>&1; then
        acpd-cli msg "󰊢 AI Note saved (no active agent pane found)" >/dev/null 2>&1 || true
    else
        tmux display-message "󰊢 AI Note saved (no active agent pane found)" 2>/dev/null || true
    fi
fi
