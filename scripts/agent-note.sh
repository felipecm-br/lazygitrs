#!/usr/bin/env bash
# agent-note.sh — Bridge between Lazygitrs review notes and Tmux AI agent panes.
# Injects prompts/notes directly into the AI agent pane in the current session / worktree.
set -euo pipefail

TARGET_DIR="${1:-$PWD}"
PROMPT="${2:-}"
CURRENT_PANE="${TMUX_PANE:-}"

# Fallback: if PROMPT is empty and stdin is provided, read stdin
if [[ -z "$PROMPT" ]] && [[ ! -t 0 ]]; then
    PROMPT=$(cat)
fi

# Strip surrounding single/double quotes that may have been added by caller escaping
TARGET_DIR="${TARGET_DIR#\'}"; TARGET_DIR="${TARGET_DIR%\'}"
TARGET_DIR="${TARGET_DIR#\"}"; TARGET_DIR="${TARGET_DIR%\"}"
PROMPT="${PROMPT#\'}"; PROMPT="${PROMPT%\'}"
PROMPT="${PROMPT#\"}"; PROMPT="${PROMPT%\"}"

if [[ -z "$PROMPT" ]]; then
    echo "Usage: agent-note.sh <workspace_path> <prompt>" >&2
    exit 1
fi

# Canonicalize TARGET_DIR if it exists
if [[ -d "$TARGET_DIR" ]]; then
    TARGET_DIR="$(cd "$TARGET_DIR" 2>/dev/null && pwd -P)"
fi

# Determine target tmux session:
# Priority 1: Explicit TMUX_ORIGIN_SESSION from lazygitrs-popup.sh
# Priority 2: Active client session from tmux display-message
TARGET_SESSION="${TMUX_ORIGIN_SESSION:-}"
if [[ -z "$TARGET_SESSION" ]]; then
    TARGET_SESSION=$(tmux display-message -p '#{session_name}' 2>/dev/null || echo "")
fi

MATCHING_PANE=""

# Helper function to check if a command name or title is an AI agent
is_agent_cmd() {
    local cmd="$1" title="${2:-}"
    [[ "$cmd" =~ ^(agy|agy-bin|antigravity|opencode|claude|codex|muse|gemini|aider)$ ]] || \
    [[ "$title" =~ (agy|antigravity|opencode|claude|codex|muse) ]]
}

# Helper function to check if a command is an editor/pager that shouldn't receive prompts
is_editor_cmd() {
    local cmd="$1"
    [[ "$cmd" =~ ^(nvim|vim|vi|nano|less|man|top|htop|btop|lazygitrs|lazygit)$ ]]
}

# Strategy A: Check TMUX_ORIGIN_PANE if provided
if [[ -n "${TMUX_ORIGIN_PANE:-}" ]]; then
    origin_info=$(tmux display-message -p -t "$TMUX_ORIGIN_PANE" '#{pane_id}|#{pane_current_command}|#{pane_title}|#{@ai_agent_state_raw}|#{@ai_agent_title}' 2>/dev/null || true)
    if [[ -n "$origin_info" ]]; then
        IFS='|' read -r o_id o_cmd o_title o_raw o_atitle <<< "$origin_info"
        if [[ -n "$o_raw" || -n "$o_atitle" ]] || is_agent_cmd "$o_cmd" "$o_title"; then
            MATCHING_PANE="$o_id"
        fi
    fi
fi

# Strategy B: Search for an AI agent pane within TARGET_SESSION first!
if [[ -z "$MATCHING_PANE" && -n "$TARGET_SESSION" ]]; then
    while IFS='|' read -r p_id p_path p_cmd p_title p_raw p_atitle p_pid; do
        [[ -n "$CURRENT_PANE" && "$p_id" == "$CURRENT_PANE" ]] && continue
        [[ "$p_cmd" == "lazygitrs" ]] && continue

        if [[ -n "$p_raw" || -n "$p_atitle" ]] || is_agent_cmd "$p_cmd" "$p_title"; then
            MATCHING_PANE="$p_id"
            break
        fi

        if [[ -n "$p_pid" ]] && ps -s "$p_pid" -o comm= 2>/dev/null | grep -qE "^(agy|agy-bin|antigravity|opencode|claude|codex|muse|gemini|aider)$"; then
            MATCHING_PANE="$p_id"
            break
        fi
    done < <(tmux list-panes -s -t "$TARGET_SESSION" -F '#{pane_id}|#{pane_current_path}|#{pane_current_command}|#{pane_title}|#{@ai_agent_state_raw}|#{@ai_agent_title}|#{pane_pid}' 2>/dev/null || true)
fi

# Strategy C: Fall back to TMUX_ORIGIN_PANE (if not an editor) or matching path in TARGET_SESSION
if [[ -z "$MATCHING_PANE" && -n "$TARGET_SESSION" ]]; then
    if [[ -n "${TMUX_ORIGIN_PANE:-}" ]]; then
        o_cmd=$(tmux display-message -p -t "$TMUX_ORIGIN_PANE" '#{pane_current_command}' 2>/dev/null || true)
        if ! is_editor_cmd "$o_cmd"; then
            MATCHING_PANE="$TMUX_ORIGIN_PANE"
        fi
    fi

    if [[ -z "$MATCHING_PANE" ]]; then
        while IFS='|' read -r p_id p_path p_cmd; do
            [[ -n "$CURRENT_PANE" && "$p_id" == "$CURRENT_PANE" ]] && continue
            is_editor_cmd "$p_cmd" && continue

            if [[ -d "$p_path" ]]; then
                c_path="$(cd "$p_path" 2>/dev/null && pwd -P)"
            else
                c_path="$p_path"
            fi

            if [[ "$c_path" == "$TARGET_DIR"* ]] || [[ "$TARGET_DIR" == "$c_path"* ]]; then
                MATCHING_PANE="$p_id"
                break
            fi
        done < <(tmux list-panes -s -t "$TARGET_SESSION" -F '#{pane_id}|#{pane_current_path}|#{pane_current_command}' 2>/dev/null || true)
    fi
fi

# Strategy D: Global fallback across all sessions (if target session had no match)
if [[ -z "$MATCHING_PANE" ]]; then
    while IFS='|' read -r p_id p_path p_cmd p_title p_raw p_atitle; do
        [[ -n "$CURRENT_PANE" && "$p_id" == "$CURRENT_PANE" ]] && continue
        is_editor_cmd "$p_cmd" && continue

        if [[ -n "$p_raw" || -n "$p_atitle" ]] || is_agent_cmd "$p_cmd" "$p_title"; then
            if [[ -d "$p_path" ]]; then
                c_path="$(cd "$p_path" 2>/dev/null && pwd -P)"
            else
                c_path="$p_path"
            fi
            if [[ "$c_path" == "$TARGET_DIR"* ]] || [[ "$TARGET_DIR" == "$c_path"* ]]; then
                MATCHING_PANE="$p_id"
                break
            fi
        fi
    done < <(tmux list-panes -a -F '#{pane_id}|#{pane_current_path}|#{pane_current_command}|#{pane_title}|#{@ai_agent_state_raw}|#{@ai_agent_title}' 2>/dev/null || true)
fi

if [[ -n "$MATCHING_PANE" ]]; then
    # Atomic delivery with bracketed paste mode (-p)
    tmux load-buffer -b "agent_note_prompt" - <<< "$PROMPT" 2>/dev/null || true
    if tmux paste-buffer -b "agent_note_prompt" -t "$MATCHING_PANE" -p -d 2>/dev/null; then
        tmux send-keys -t "$MATCHING_PANE" Enter 2>/dev/null || true
    else
        flat_prompt=$(tr '\n' ' ' <<< "$PROMPT")
        tmux send-keys -l -t "$MATCHING_PANE" -- "$flat_prompt" 2>/dev/null || true
        tmux send-keys -t "$MATCHING_PANE" Enter 2>/dev/null || true
    fi
    tmux display-message "󰊢 Review note dispatched to AI agent in pane $MATCHING_PANE" 2>/dev/null || true
else
    echo "AI Note saved (no active agent pane found for $TARGET_DIR)" >&2
fi
