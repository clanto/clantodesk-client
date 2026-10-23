#!/usr/bin/env bash
# Eseguito SOLO dentro il container di build da docker/e2e/run.sh: copia il repo
# (montato in sola lettura su /src-ro), inietta l'harness e compila.
set -euo pipefail
[ -f /.dockerenv ] || { echo "build-inner.sh va eseguito nel container" >&2; exit 2; }

mkdir -p /work
tar -C /src-ro --exclude=./.git --exclude=./target -cf - . | tar -C /work -xf -
cd /work
# target/ sta in un volume persistente, il sorgente no: build.rs deve
# rigenerare src/version.rs anche quando Cargo.toml resta uguale.
touch Cargo.toml
cp docker/e2e/e2e_harness.rs src/e2e_harness.rs
printf '\npub mod e2e_harness;\n' >> src/lib.rs
perl -0 -i -pe 's/(        \} else if args\[0\] == "--option" \{)/        } else if args[0] == "--e2e" {\n            crate::e2e_harness::run(&args[1..]);\n            return None;\n$1/' src/core_main.rs
grep -q "e2e_harness::run" src/core_main.rs

cargo build --locked --features linux-pkg-config --bin rustdesk -j "$(nproc)" >/tmp/build.log 2>&1 \
  || { grep -E -A7 "^error" /tmp/build.log | head -80; exit 1; }
cp /build/target/debug/rustdesk /out/rustdesk
echo "    build OK"
