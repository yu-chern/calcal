#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export AUTH_MODE=local
export WEB_PORT="${WEB_PORT:-5180}"
export APP_ORIGIN="http://127.0.0.1:${WEB_PORT}"
export PORT=3000
cargo build --locked
target/debug/calcal &
backend_pid=$!
cleanup() { kill "$backend_pid" "${frontend_pid:-}" 2>/dev/null || true; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
(cd web && exec node node_modules/vite/bin/vite.js --host 127.0.0.1) &
frontend_pid=$!
while kill -0 "$backend_pid" 2>/dev/null && kill -0 "$frontend_pid" 2>/dev/null; do
  sleep 1
done
echo 'A development server stopped; shutting down both processes.' >&2
exit 1
