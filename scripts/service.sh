#!/usr/bin/env bash
# Manage one complete local stack. Compatible with macOS Bash 3.2.
set -euo pipefail
export PATH="$PATH:/usr/sbin:/sbin"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
RUNTIME="$ROOT/deploy/runtime/service"
umask 077
mkdir -p "$RUNTIME"
ACTION="${1:-help}"
MODE="${2:-dev}"
case "$MODE" in dev|prod) ;; *) echo 'Mode must be dev or prod.' >&2; exit 2 ;; esac
ps -p "$$" -o pid= >/dev/null || { echo 'Process inspection is required to safely manage services.' >&2; exit 1; }
STATE="$RUNTIME/$MODE.pid"
LOG="$RUNTIME/$MODE.log"

fingerprint() { ps -p "$1" -o lstart= -o command= 2>/dev/null; }
running() {
  local stored
  stored="$(cat "$STATE" 2>/dev/null)" || return 1
  [[ "$stored" == *$'\n'* ]] || return 1
  PID="${stored%%$'\n'*}"
  [[ "$PID" =~ ^[0-9]+$ ]] || return 1
  [[ "$(fingerprint "$PID")" == "${stored#*$'\n'}" ]] && kill -0 "$PID" 2>/dev/null
}

# The supervisor owns its children and stops the other component if either exits.
if [[ "$ACTION" == __run ]]; then
  children=()
  cleanup() {
    trap '' TERM INT
    if [[ ${#children[@]} -gt 0 ]]; then
      kill "${children[@]}" 2>/dev/null || true
      for ((i=0; i<10; i++)); do
        alive=false
        for child in "${children[@]}"; do
          if kill -0 "$child" 2>/dev/null; then alive=true; fi
        done
        if ! $alive; then break; fi
        sleep 1
      done
      for child in "${children[@]}"; do
        if kill -0 "$child" 2>/dev/null; then kill -KILL "$child" 2>/dev/null || true; fi
        wait "$child" 2>/dev/null || true
      done
    fi
    rm -f "$STATE" "$RUNTIME/$MODE.ready"
  }
  trap cleanup EXIT
  trap 'exit 0' TERM INT
  if [[ "$MODE" == dev ]]; then
    export AUTH_MODE=local PORT=3000 WEB_PORT="${WEB_PORT:-5180}"
    export APP_ORIGIN="http://127.0.0.1:$WEB_PORT"
    "$ROOT/target/debug/calcal" &
    children+=("$!")
    (cd web && exec node node_modules/vite/bin/vite.js --host 127.0.0.1) &
    children+=("$!")
    url="http://127.0.0.1:$WEB_PORT"
    expected=200
  else
    "$ROOT/scripts/serve.sh" &
    children+=("$!")
    cloudflared tunnel --config "$ROOT/deploy/cloudflared.local.yml" run &
    children+=("$!")
    url="http://127.0.0.1:3000/api/session"
    expected=401
  fi
  printf '%s\n%s\n' "$$" "$(fingerprint "$$")" > "$STATE"
  ready=false
  while true; do
    for child in "${children[@]}"; do
      if ! kill -0 "$child" 2>/dev/null; then
        echo 'A component exited; stopping the complete stack.' >&2
        exit 1
      fi
    done
    if ! $ready; then
      code="$(curl --noproxy '*' -s -o /dev/null -w '%{http_code}' --max-time 1 "$url" || true)"
      backend_code="$(curl --noproxy '*' -s -o /dev/null -w '%{http_code}' --max-time 1 http://127.0.0.1:3000/api/session || true)"
      if [[ "$code" == "$expected" && "$backend_code" == "$expected" ]]; then
        touch "$RUNTIME/$MODE.ready"
        ready=true
      fi
    fi
    sleep 1
  done
fi

case "$ACTION" in
  start|stop|status|restart) ;;
  *) echo 'Usage: ./scripts/service.sh {start|stop|status|restart} [dev|prod]'; exit 2 ;;
esac

if [[ "$ACTION" == status ]]; then
  if running; then
    echo "$MODE running (supervisor PID $PID). Log: $LOG"
    if [[ "$MODE" == prod ]]; then
      echo 'Local processes only; this does not verify Tunnel connectivity, DNS, or Access login.'
    fi
    exit 0
  fi
  echo "$MODE stopped. Log: $LOG"
  exit 1
fi

# Serialize mutations across modes, which share backend port 3000.
LOCK="$RUNTIME/control.lock"
if ! mkdir "$LOCK" 2>/dev/null; then
  echo "Another service command is active. If it was forcibly killed, remove the empty directory: $LOCK" >&2
  exit 1
fi
trap 'rmdir "$LOCK"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

stop() {
  if ! running; then
    rm -f "$STATE" "$RUNTIME/$MODE.ready"
    echo "$MODE already stopped."
    return
  fi
  kill -TERM "$PID"
  for ((attempt=0; attempt<20; attempt++)); do
    if ! running; then echo "$MODE stopped."; return; fi
    sleep 1
  done
  echo "Stop timed out; inspect $LOG before retrying." >&2
  return 1
}

start() {
  if running; then echo "$MODE already running (PID $PID)."; return; fi
  for command in cargo node npm curl lsof; do
    command -v "$command" >/dev/null || { echo "Missing command: $command" >&2; return 1; }
  done
  local web_port="${WEB_PORT:-5180}"
  if [[ ! "$web_port" =~ ^[0-9]+$ ]] || ((web_port < 1 || web_port > 65535)); then
    echo 'WEB_PORT must be between 1 and 65535.' >&2
    return 1
  fi
  local ports=(3000)
  if [[ "$MODE" == dev ]]; then
    [[ "$web_port" != 3000 ]] || { echo 'WEB_PORT cannot be 3000.' >&2; return 1; }
    ports+=("$web_port")
  else
    command -v cloudflared >/dev/null || { echo 'Missing command: cloudflared' >&2; return 1; }
    [[ -f .env ]] || { echo 'Create .env using .env.example and complete Cloudflare Access setup first. See docs/deployment.md.' >&2; return 1; }
    # Validate in a subshell without printing private configuration.
    (
      set -a
      source .env
      [[ "${AUTH_MODE:-cloudflare}" == cloudflare && "${PORT:-3000}" == 3000 && "${APP_ORIGIN:-https://finanio.app}" == https://finanio.app ]] || {
        echo 'Production requires Cloudflare auth, PORT=3000 and APP_ORIGIN=https://finanio.app.' >&2; exit 1;
      }
      for key in CF_ACCESS_TEAM CF_ACCESS_AUD ALLOWED_EMAIL; do
        [[ -n "${!key:-}" ]] || { echo "Missing configuration: $key" >&2; exit 1; }
      done
    ) || return 1
    [[ -f deploy/cloudflared.local.yml ]] || { echo 'Missing deploy/cloudflared.local.yml.' >&2; return 1; }
    cloudflared tunnel --config deploy/cloudflared.local.yml ingress validate
  fi
  for port in "${ports[@]}"; do
    if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
      echo "Port $port is already in use. Stop the existing service first; no process was killed." >&2
      return 1
    fi
  done
  if [[ "$MODE" == dev ]]; then
    [[ -f web/node_modules/vite/bin/vite.js ]] || npm --prefix web ci
    cargo build --locked
  else
    "$ROOT/scripts/build.sh"
  fi
  rm -f "$STATE" "$RUNTIME/$MODE.ready"
  # Node is already required by the frontend. A detached session survives the
  # launching terminal/session closing; no extra process-manager dependency.
  local supervisor
  supervisor="$(node - "$ROOT/scripts/service.sh" "$MODE" "$LOG" <<'NODE'
const { spawn } = require('node:child_process');
const { openSync, closeSync } = require('node:fs');
const [script, mode, log] = process.argv.slice(2);
const output = openSync(log, 'a', 0o600);
const child = spawn('/bin/bash', [script, '__run', mode], {
  detached: true,
  stdio: ['ignore', output, output],
});
closeSync(output);
child.on('error', () => { console.error('Could not launch service supervisor.'); process.exitCode = 1; });
child.on('spawn', () => { console.log(child.pid); child.unref(); });
NODE
  )"
  for ((attempt=0; attempt<30; attempt++)); do
    if ! kill -0 "$supervisor" 2>/dev/null; then
      echo "Startup failed; inspect $LOG." >&2
      return 1
    fi
    if [[ -f "$RUNTIME/$MODE.ready" ]] && running; then
      echo "$MODE started (PID $PID). Log: $LOG"
      if [[ "$MODE" == dev ]]; then
        echo "Open http://127.0.0.1:$web_port"
      else
        echo 'Local production stack started. Verify https://finanio.app separately with Access login.'
      fi
      return
    fi
    sleep 1
  done
  echo "Startup timed out; stopping stack. Inspect $LOG." >&2
  stop
  return 1
}

case "$ACTION" in
  start) start ;;
  stop) stop ;;
  restart) stop; start ;;
esac
