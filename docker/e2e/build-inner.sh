#!/usr/bin/env bash
# Eseguito SOLO nel container di build da docker/e2e/run.sh, dopo sync-src.sh:
# inietta l'harness nella copia del repo in /work e compila.
set -euo pipefail
[ -f /.dockerenv ] || { echo "build-inner.sh va eseguito nel container" >&2; exit 2; }
cd /work

cp docker/e2e/e2e_harness.rs src/e2e_harness.rs
printf '\npub mod e2e_harness;\n' >> src/lib.rs
perl -0 -i -pe 's/(        \} else if args\[0\] == "--option" \{)/        } else if args[0] == "--e2e" {\n            crate::e2e_harness::run(&args[1..]);\n            return None;\n$1/' src/core_main.rs
grep -q "e2e_harness::run" src/core_main.rs

cargo build --locked --features linux-pkg-config --bin rustdesk -j "$(nproc)" >/tmp/build.log 2>&1 \
  || { grep -E -A7 "^error" /tmp/build.log | head -80; exit 1; }
cp /build/target/debug/rustdesk /out/rustdesk
echo "    build OK"
