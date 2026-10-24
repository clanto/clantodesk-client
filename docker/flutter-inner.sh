#!/usr/bin/env bash
# Eseguito nel container da docker/flutter-check.sh, in /work gia' allineata.
#   flutter-inner.sh [test da eseguire...]
set -uo pipefail
[ -f /.dockerenv ] || { echo "flutter-inner.sh va eseguito nel container" >&2; exit 2; }
TESTS="$*"
fail=0
step() { echo; echo "==> $*"; }

step "flutter pub get"
(cd flutter && flutter pub get >/tmp/pub.log 2>&1) || { tail -30 /tmp/pub.log; exit 1; }

step "flutter_rust_bridge_codegen"
flutter_rust_bridge_codegen --rust-input ./src/flutter_ffi.rs \
  --dart-output ./flutter/lib/generated_bridge.dart \
  --c-output ./flutter/macos/Runner/bridge_generated.h >/tmp/frb.log 2>&1 \
  || { grep -v 'Skipping unresolvable' /tmp/frb.log | tail -40; exit 1; }

step "flutter analyze (errori)"
(cd flutter && flutter analyze --no-fatal-infos --no-fatal-warnings lib test >/tmp/analyze.log 2>&1)
grep -E "^\s*error " /tmp/analyze.log && fail=1 || echo "nessun errore"
echo "warning nei file Clanto o toccati dal fork:"
grep -E "^\s*warning .*(lib/clanto/|desktop_setting_page.dart|mobile/pages/settings_page.dart)" /tmp/analyze.log \
  || echo "  nessuno"

step "flutter test ${TESTS:-(tutti)}"
(cd flutter && flutter test ${TESTS} >/tmp/test.log 2>&1); rc=$?
grep -E "\[E\]|Some tests failed" /tmp/test.log | tail -25
tail -c 300 /tmp/test.log | tr '\r' '\n' | grep -E "All tests passed|tests? failed" | tail -1
[ "$rc" = 0 ] || fail=1

step "cargo check --features flutter,linux-pkg-config"
cargo check --locked --features flutter,linux-pkg-config -j "$(nproc)" >/tmp/cc.log 2>&1; rc=$?
grep -E -A5 "^error" /tmp/cc.log | head -60; grep -E "Finished" /tmp/cc.log
[ "$rc" = 0 ] || fail=1

echo; [ "$fail" = 0 ] && echo "FLUTTER CHECK: OK" || echo "FLUTTER CHECK: FALLITO"
exit $fail
