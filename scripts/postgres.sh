#!/usr/bin/env bash
# Project-owned local PostgreSQL cluster. Never touches a system cluster.
set -euo pipefail
cd "$(dirname "$0")/.."
umask 077
ROOT="$PWD/deploy/runtime/postgres"
DATA="$ROOT/data"
export PGPASSFILE="$ROOT/pgpass"
mkdir -p "$ROOT"
case "${1:-status}" in
  start)
    for command in initdb pg_ctl createdb python3; do
      command -v "$command" >/dev/null || { echo "Missing $command; install PostgreSQL first." >&2; exit 1; }
    done
    if [[ ! -f "$DATA/PG_VERSION" ]]; then
      if [[ -e deploy/runtime/postgres.env || -e "$DATA" ]]; then
        echo 'Partial database setup exists; inspect deploy/runtime/postgres before retrying.' >&2
        exit 1
      fi
      python3 - <<'PY'
import pathlib,secrets
root=pathlib.Path('deploy/runtime/postgres')
password=secrets.token_hex(32)
(root/'initial-password').write_text(password+'\n')
(root/'pgpass').write_text('127.0.0.1:55432:*:calcal:'+password+'\n')
pathlib.Path('deploy/runtime/postgres.env').write_text('DATABASE_URL=postgresql://calcal:'+password+'@127.0.0.1:55432/calcal\nTEST_DATABASE_URL=postgresql://calcal:'+password+'@127.0.0.1:55432/calcal_test\n')
PY
      initdb -D "$DATA" -U calcal --auth-host=scram-sha-256 --auth-local=scram-sha-256 --pwfile="$ROOT/initial-password" --encoding=UTF8 --locale=C > "$ROOT/init.log"
      rm "$ROOT/initial-password"
      cat >> "$DATA/postgresql.conf" <<'CONF'
listen_addresses = '127.0.0.1'
port = 55432
unix_socket_directories = ''
CONF
    fi
    if ! pg_ctl -D "$DATA" status >/dev/null 2>&1; then
      pg_ctl -D "$DATA" -l "$ROOT/server.log" start
    fi
    for database in calcal calcal_test; do
      exists="$(psql -h 127.0.0.1 -p 55432 -U calcal -d postgres -Atc "SELECT 1 FROM pg_database WHERE datname='$database'")"
      if [[ "$exists" != 1 ]]; then createdb -h 127.0.0.1 -p 55432 -U calcal "$database"; fi
    done
    echo 'Project Postgres ready on 127.0.0.1:55432. Credentials stay in deploy/runtime/postgres.env.'
    ;;
  stop) pg_ctl -D "$DATA" stop -m fast ;;
  status) pg_ctl -D "$DATA" status ;;
  *) echo 'Usage: ./scripts/postgres.sh {start|stop|status}' >&2; exit 2 ;;
esac
