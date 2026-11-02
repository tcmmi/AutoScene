#!/usr/bin/env sh
set -e

# AMVerge Deadframes Entrypoint
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Prioritize virtual environment if one exists in the extension directory
if [ -f "$SCRIPT_DIR/.venv/bin/python" ]; then
    exec "$SCRIPT_DIR/.venv/bin/python" "$SCRIPT_DIR/main.py" "$@"
elif command -v uv >/dev/null 2>&1; then
    exec uv run "$SCRIPT_DIR/main.py" "$@"
elif command -v python3 >/dev/null 2>&1; then
    exec python3 "$SCRIPT_DIR/main.py" "$@"
else
    echo "Python 3 is required to run AMVerge Deadframe Remover" >&2
    exit 1
fi

