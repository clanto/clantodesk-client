#!/usr/bin/env bash
# Analisi di sicurezza in container, senza strumenti installati in locale:
#   - OSV-Scanner: vulnerabilita' note nelle dipendenze (Cargo.lock, pubspec, Gradle)
#   - Semgrep: analisi statica del codice Clanto (regole Rust, Dart, segreti)
#
#   docker/security-scan.sh   # Semgrep decide l'esito; OSV e' informativo come in CI
#
# Il repo e' montato in sola lettura.
set -uo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
require_docker
OSV_IMAGE=ghcr.io/google/osv-scanner:v1.9.1
SEMGREP_IMAGE=semgrep/semgrep:1.95.0
fail=0

# Il repo arriva nel volume clantodesk-work-scan (vedi common.sh): nessun bind
# mount. Una sorgente vuota farebbe passare la scansione a vuoto, quindi si verifica.
build_image Dockerfile.check clantodesk-check
sync_volume scan
docker run --rm -v clantodesk-work-scan:/src:ro clantodesk-check \
  test -f /src/Cargo.lock -a -f /src/src/clanto/mod.rs \
  || { echo "ERRORE: sorgente non disponibile nel container" >&2; exit 2; }

echo "==> OSV-Scanner (dipendenze)"
OSV_OUT="$(mktemp)"
docker run --rm -v clantodesk-work-scan:/src:ro "$OSV_IMAGE" \
  --recursive --skip-git /src >"$OSV_OUT" 2>&1
osv=$(grep -c "osv.dev/" "$OSV_OUT" || true)
grep -q "Scanned /src/Cargo.lock" "$OSV_OUT" \
  || { echo "ERRORE: OSV-Scanner non ha letto Cargo.lock"; tail -20 "$OSV_OUT"; exit 2; }
grep -F "| https://osv.dev" "$OSV_OUT" | awk -F"|" '$3+0>=7' | sed "s/^/  /"
echo "  advisory nelle dipendenze: $osv (informativo, mostrati quelli con CVSS >= 7; triage P9)"
rm -f "$OSV_OUT"

echo
echo "==> Semgrep (codice Clanto e hook)"
# Escluse dopo revisione: `unsafe` e' solo FFI verso API di sistema (Event Log,
# AMSI, X11, Cocoa, Win32); `args` segnala il parser della CLI di core_main.
docker run --rm -v clantodesk-work-scan:/src:ro -w /src "$SEMGREP_IMAGE" \
  semgrep scan --metrics=off --error --quiet \
    --config p/rust --config p/secrets --config p/php \
    --exclude-rule rust.lang.security.unsafe-usage.unsafe-usage \
    --exclude-rule rust.lang.security.args.args \
    src/clanto flutter/lib/clanto api docker/e2e \
    src/server/connection.rs src/client.rs src/core_main.rs src/hbbs_http/sync.rs \
  || fail=1

echo
[ "$fail" = 0 ] && echo "SECURITY SCAN: nessun risultato" || echo "SECURITY SCAN: risultati da valutare"
exit $fail
