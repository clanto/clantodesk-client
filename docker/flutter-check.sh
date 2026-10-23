#!/usr/bin/env bash
# Verifica Flutter in container: bridge, analyze, test Dart, cargo check --features flutter.
#
#   docker/flutter-check.sh                  # analyze di tutto lib/, tutti i test
#   docker/flutter-check.sh test/clanto_*    # solo i test indicati
#
# Il repo viene montato in sola lettura e copiato nel container: i file generati
# (bridge, .freezed.dart, pubspec.lock) non toccano il working tree.
# Su Windows (Git Bash) usare MSYS_NO_PATHCONV=1.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
case "$(uname -s)" in MINGW*|MSYS*) REPO_ROOT="$(cygpath -m "$REPO_ROOT")" ;; esac
TESTS="${*:-}"

if ! docker info >/dev/null 2>&1; then
  echo "ERRORE: il daemon Docker non risponde. Avvia Docker Desktop e riprova." >&2
  exit 2
fi

echo "==> immagini (lente solo la prima volta)"
docker build -q -f "$REPO_ROOT/docker/Dockerfile.check" -t clantodesk-check "$REPO_ROOT/docker" >/dev/null
docker build -q -f "$REPO_ROOT/docker/Dockerfile.flutter-check" -t clantodesk-flutter-check "$REPO_ROOT/docker" >/dev/null
for v in clantodesk-cargo clantodesk-cargo-git clantodesk-pub-cache; do docker volume create "$v" >/dev/null; done

docker run --rm \
  -v "$REPO_ROOT":/src-ro:ro \
  -v clantodesk-cargo:/root/.cargo/registry \
  -v clantodesk-cargo-git:/root/.cargo/git \
  -v clantodesk-pub-cache:/root/.pub-cache \
  -e CARGO_TARGET_DIR=/build/target \
  -e TESTS="$TESTS" \
  clantodesk-flutter-check \
  bash -c '
    set -uo pipefail
    mkdir -p /work /build/target
    tar -C /src-ro --exclude=./.git --exclude=./target --exclude=./flutter/build \
        --exclude=./flutter/.dart_tool -cf - . | tar -C /work -xf -
    cd /work
    fail=0
    step() { echo; echo "==> $*"; }

    step "flutter pub get"
    (cd flutter && flutter pub get >/tmp/pub.log 2>&1) || { tail -30 /tmp/pub.log; exit 1; }

    step "flutter_rust_bridge_codegen"
    flutter_rust_bridge_codegen --rust-input ./src/flutter_ffi.rs \
      --dart-output ./flutter/lib/generated_bridge.dart \
      --c-output ./flutter/macos/Runner/bridge_generated.h >/tmp/frb.log 2>&1 \
      || { tail -40 /tmp/frb.log; exit 1; }

    step "flutter analyze (errori)"
    (cd flutter && flutter analyze --no-fatal-infos --no-fatal-warnings lib test >/tmp/analyze.log 2>&1)
    grep -E "^\s*error " /tmp/analyze.log && fail=1 || echo "nessun errore"
    echo "warning/info nei file Clanto o toccati dal fork:"
    grep -E "lib/clanto/|desktop_setting_page.dart|mobile/pages/settings_page.dart" /tmp/analyze.log \
      | grep -vE "^\s*info .*(prefer_const|use_key_in_widget|library_private_types|deprecated_member_use|unnecessary_)" \
      || echo "  nessuno"

    step "flutter test ${TESTS:-(tutti)}"
    (cd flutter && flutter test ${TESTS} >/tmp/test.log 2>&1); rc=$?
    grep -E "^[0-9:+ -]+.*\[E\]|Some tests failed|All tests passed" /tmp/test.log | tail -25
    tail -3 /tmp/test.log
    [ "$rc" = 0 ] || fail=1

    step "cargo check --features flutter,linux-pkg-config"
    cargo check --locked --features flutter,linux-pkg-config -j "$(nproc)" >/tmp/cc.log 2>&1; rc=$?
    grep -E -A5 "^error" /tmp/cc.log | head -60; grep -E "Finished" /tmp/cc.log
    [ "$rc" = 0 ] || fail=1

    echo; [ "$fail" = 0 ] && echo "FLUTTER CHECK: OK" || echo "FLUTTER CHECK: FALLITO"
    exit $fail
  '
