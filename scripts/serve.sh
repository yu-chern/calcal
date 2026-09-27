#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -f .env ]]; then
  set -a
  source .env
  set +a
fi
if [[ "${AUTH_MODE:-cloudflare}" != cloudflare ]]; then
  echo 'serve.sh requires Cloudflare authentication; use scripts/dev.sh locally.' >&2
  exit 1
fi
test -f web/dist/index.html || { echo 'Run scripts/build.sh first.' >&2; exit 1; }
exec target/release/calcal
