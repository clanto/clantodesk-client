#!/usr/bin/env bash
# Verifica Flutter in container: bridge, analyze, test Dart, cargo check --features flutter.
#
#   docker/flutter-check.sh                  # analyze di tutto lib/, tutti i test
#   docker/flutter-check.sh test/clanto_*    # solo i test indicati
#
# I file generati (bridge, .freezed.dart, .dart_tool) restano nel volume di
# lavoro del profilo `flutter`: il working tree non viene toccato.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

require_docker
echo "==> immagini (lente solo la prima volta)"
build_image Dockerfile.check clantodesk-check
build_image Dockerfile.flutter-check clantodesk-flutter-check

dev_run flutter clantodesk-flutter-check -- bash docker/flutter-inner.sh "$@"
