#!/usr/bin/env bash
# start.sh — set up (once) and launch PikaBull
# Uses uv for fast venv + dependency management.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

VENV=".venv"
PORT="${PORT:-8000}"
HOST="${HOST:-127.0.0.1}"

# ── Colours ────────────────────────────────────────────────────────────────────
green()  { printf '\033[0;32m%s\033[0m\n' "$*"; }
yellow() { printf '\033[0;33m%s\033[0m\n' "$*"; }
red()    { printf '\033[0;31m%s\033[0m\n' "$*"; }
bold()   { printf '\033[1m%s\033[0m\n'   "$*"; }

bold "━━━  PikaBull ⚡🐂  |  Multi-Agent Stock Analyst  ━━━"

# ── Check uv ──────────────────────────────────────────────────────────────────
if ! command -v uv &>/dev/null; then
  red "uv not found. Install it with:"
  echo "  curl -LsSf https://astral.sh/uv/install.sh | sh"
  exit 1
fi

# ── Create venv (only if it doesn't exist) ────────────────────────────────────
if [ ! -d "$VENV" ]; then
  yellow "Creating virtual environment with uv…"
  uv venv "$VENV" --python 3.11
  green "Virtual environment created at $VENV/"
else
  green "Virtual environment already exists — skipping creation"
fi

# ── Install / sync dependencies ───────────────────────────────────────────────
yellow "Syncing dependencies…"
uv pip install -r requirements.txt --quiet
green "Dependencies up to date"

# ── .env check ────────────────────────────────────────────────────────────────
if [ ! -f ".env" ]; then
  if [ -f ".env.example" ]; then
    cp ".env.example" ".env"
    yellow ".env not found — copied from .env.example"
    yellow "→ Edit .env to set your API key and provider before re-running"
    exit 0
  else
    red ".env file not found. Create one based on .env.example"
    exit 1
  fi
fi

# ── Validate that at least one API key is set ─────────────────────────────────
provider="$(grep -E '^LLM_PROVIDER=' .env | cut -d= -f2 | tr -d '[:space:]')"
provider="${provider:-anthropic}"

case "$provider" in
  anthropic)
    key="$(grep -E '^ANTHROPIC_API_KEY=' .env | cut -d= -f2 | tr -d '[:space:]')"
    if [[ -z "$key" || "$key" == sk-ant-* && ${#key} -lt 20 ]]; then
      yellow "Warning: ANTHROPIC_API_KEY looks unset in .env"
    fi
    ;;
  openai)
    key="$(grep -E '^OPENAI_API_KEY=' .env | cut -d= -f2 | tr -d '[:space:]')"
    if [[ -z "$key" ]]; then
      yellow "Warning: OPENAI_API_KEY looks unset in .env"
    fi
    ;;
  ollama)
    base_url="$(grep -E '^OLLAMA_BASE_URL=' .env | cut -d= -f2 | tr -d '[:space:]')"
    base_url="${base_url:-http://localhost:11434/v1}"
    yellow "Provider: Ollama — make sure Ollama is running at $base_url"
    ;;
  minimax)
    key="$(grep -E '^MINIMAX_API_KEY=' .env | cut -d= -f2 | tr -d '[:space:]')"
    if [[ -z "$key" ]]; then
      yellow "Warning: MINIMAX_API_KEY looks unset in .env"
    fi
    ;;
esac

# ── Launch ─────────────────────────────────────────────────────────────────────
green ""
green "Provider : $provider"
green "Address  : http://$HOST:$PORT"
green ""

exec uv run --no-sync uvicorn app:app \
  --host "$HOST" \
  --port "$PORT" \
  --reload
