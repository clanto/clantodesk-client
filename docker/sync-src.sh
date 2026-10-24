#!/usr/bin/env bash
# Eseguito nel container: allinea /work (volume) alla copia del repo ricevuta via tar.
# rsync copia solo i file cambiati e conserva le date: Cargo e Flutter ricompilano
# solo il necessario. I file generati nel container non vengono cancellati.
#   sync-src.sh <cartella sorgente>
set -euo pipefail
[ -f /.dockerenv ] || { echo "sync-src.sh va eseguito nel container" >&2; exit 2; }
SRC="${1:?cartella sorgente}"
[ -f "$SRC/Cargo.toml" ] || { echo "sorgente vuota o incompleta in $SRC" >&2; exit 1; }

rsync -a --delete \
  --exclude=/.git --exclude=/target \
  --exclude=/src/version.rs \
  --exclude=/src/bridge_generated.rs --exclude=/src/bridge_generated.io.rs \
  --exclude=/flutter/lib/generated_bridge.dart \
  --exclude=/flutter/lib/generated_bridge.freezed.dart \
  --exclude=/flutter/macos/Runner/bridge_generated.h \
  --exclude=/flutter/ios/Runner/bridge_generated.h \
  --exclude=/flutter/.dart_tool --exclude=/flutter/build \
  --exclude=/flutter/.flutter-plugins --exclude=/flutter/.flutter-plugins-dependencies \
  --exclude=/src/e2e_harness.rs \
  "$SRC"/ /work/
