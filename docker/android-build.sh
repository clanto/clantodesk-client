#!/usr/bin/env bash
# APK Android in container, con gli stessi passi del job Android di flutter-build.yml.
#
#   docker/android-build.sh                       # arm64-v8a, variante outgoing (Play)
#   docker/android-build.sh x86_64 incoming       # emulatore, variante host
#
# L'APK (firmato con la chiave di debug) finisce in target/android/. vcpkg, le
# librerie compilate e la cache di Gradle stanno in volumi: la prima build dura
# a lungo (ffmpeg, aom, libvpx), le successive ricompilano solo ciò che cambia.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

ABI="${1:-arm64-v8a}"
ROLE="${2:-outgoing}"
case "$ABI" in arm64-v8a|armeabi-v7a|x86_64) ;; *) echo "ABI: arm64-v8a, armeabi-v7a o x86_64" >&2; exit 2 ;; esac
case "$ROLE" in outgoing|incoming) ;; *) echo "variante: outgoing o incoming" >&2; exit 2 ;; esac
VCPKG_COMMIT="$(sed -n 's/^  VCPKG_COMMIT_ID: "\([0-9a-f]*\)".*/\1/p' "$REPO_DIR/.github/workflows/flutter-build.yml")"
[ -n "$VCPKG_COMMIT" ] || { echo "VCPKG_COMMIT_ID non trovato in flutter-build.yml" >&2; exit 2; }

require_docker
echo "==> immagini (la prima volta scarica SDK e NDK)"
build_image Dockerfile.check clantodesk-check
build_image Dockerfile.flutter-check clantodesk-flutter-check
build_image Dockerfile.android clantodesk-android

dev_run android clantodesk-android \
  -v clantodesk-android-vcpkg:/opt/vcpkg \
  -v clantodesk-android-gradle:/root/.gradle \
  -v clantodesk-android-out:/out \
  -e VCPKG_COMMIT="$VCPKG_COMMIT" \
  -e CONN_TYPE="$ROLE" \
  -- bash docker/android-inner.sh "$ABI" | tee /dev/stderr | grep '^APK=' > /tmp/clanto-apk.$$ || true

APK="$(sed -n 's/^APK=//p' /tmp/clanto-apk.$$)"; rm -f /tmp/clanto-apk.$$
[ -n "$APK" ] || { echo "ANDROID BUILD: FALLITA"; exit 1; }
mkdir -p "$REPO_DIR/target/android"
docker run --rm -v clantodesk-android-out:/out clantodesk-check cat "/out/$APK" > "$REPO_DIR/target/android/$APK"
echo "ANDROID BUILD: OK -> target/android/$APK"
