#!/usr/bin/env bash
# Tutte le verifiche in sequenza, con riepilogo finale:
# test Rust della lib, Flutter, E2E 2FA, scansione di sicurezza.
#
#   docker/test-all.sh
#
# I log completi restano in $LOG_DIR (default: una cartella temporanea).
set -uo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="${LOG_DIR:-$(mktemp -d)}"
declare -A RESULT

run() { # nome comando...
  local name="$1"; shift
  echo "==> $name (log: $LOG_DIR/$name.log)"
  if "$@" >"$LOG_DIR/$name.log" 2>&1; then RESULT[$name]=OK; else RESULT[$name]=FALLITO; fi
  tail -15 "$LOG_DIR/$name.log" | sed 's/^/    /'
}

run rust "$DIR/check.sh" --test clanto:: core_main::
run flutter "$DIR/flutter-check.sh"
run e2e "$DIR/e2e/run.sh"
run security "$DIR/security-scan.sh"

echo
echo "Riepilogo"
fail=0
for name in rust flutter e2e security; do
  printf '  %-9s %s\n' "$name" "${RESULT[$name]}"
  [ "${RESULT[$name]}" = OK ] || fail=1
done
exit $fail
