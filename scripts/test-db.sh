#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Use only the dedicated test DB, never DATABASE_URL.
if [[ -z "${TEST_DATABASE_URL:-}" && -f deploy/runtime/postgres.env ]]; then
  set -a
  source deploy/runtime/postgres.env
  set +a
fi
[[ -n "${TEST_DATABASE_URL:-}" ]] || { echo 'Set TEST_DATABASE_URL to an isolated test database, or run scripts/postgres.sh start.' >&2; exit 1; }
exec cargo test --locked --test agent -- --ignored
