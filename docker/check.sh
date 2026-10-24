#!/usr/bin/env bash
# Type-check del crate Rust in container, senza toolchain Rust in locale.
#
#   docker/check.sh                          # cargo check, feature linux-pkg-config
#   docker/check.sh --test clanto:: core_main::   # cargo test della lib con questi filtri
#
# Il working tree non viene toccato: il repo e' montato in sola lettura e
# sincronizzato in un volume (vedi docker/common.sh). Per la feature `flutter`
# usare docker/flutter-check.sh, che genera prima il bridge.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

require_docker
echo "==> immagine clantodesk-check"
build_image Dockerfile.check clantodesk-check

if [ "${1:-}" = "--test" ]; then
  shift
  echo "==> cargo test --lib -- $*"
  dev_run check clantodesk-check -- \
    "cargo test --locked --features linux-pkg-config --lib -j \$(nproc) -- $* 2>&1 \
       | grep -E '^test .*FAILED|^failures:|test result|panicked|^error' -A3; \
     exit \${PIPESTATUS[0]}"
else
  echo "==> cargo check --features linux-pkg-config"
  dev_run check clantodesk-check -- \
    "cargo check --locked --features linux-pkg-config -j \$(nproc) 2>&1 \
       | grep -E '^error|Finished' -A6; exit \${PIPESTATUS[0]}"
fi
