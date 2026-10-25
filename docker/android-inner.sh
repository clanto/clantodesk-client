#!/usr/bin/env bash
# Eseguito SOLO nel container da docker/android-build.sh, in /work già allineata.
#   android-inner.sh <arm64-v8a|armeabi-v7a|x86_64>
set -euo pipefail
[ -f /.dockerenv ] || { echo "android-inner.sh va eseguito nel container" >&2; exit 2; }
ABI="$1"
case "$ABI" in
  arm64-v8a)   TARGET=aarch64-linux-android;   NDK_SCRIPT=ndk_arm64.sh; PLATFORM=android-arm64; LIBCXX=aarch64-linux-android; ARCH=aarch64 ;;
  armeabi-v7a) TARGET=armv7-linux-androideabi; NDK_SCRIPT=ndk_arm.sh;   PLATFORM=android-arm;   LIBCXX=arm-linux-androideabi; ARCH=armv7 ;;
  x86_64)      TARGET=x86_64-linux-android;    NDK_SCRIPT=ndk_x64.sh;   PLATFORM=android-x64;   LIBCXX=x86_64-linux-android;  ARCH=x86_64 ;;
esac
step() { echo; echo "==> $*"; }
fail_log() { tail -"${2:-40}" "$1"; exit 1; }

step "fine riga LF negli script (il checkout Windows li ha in CRLF)"
find flutter -maxdepth 3 \( -name '*.sh' -o -name gradlew \) -not -path '*/build/*' -exec sed -i 's/\r$//' {} +
# Anche i port vcpkg: le patch in CRLF corrompono il configure di ffmpeg.
find res/vcpkg -type f -exec sed -i 's/\r$//' {} +

step "patch Flutter della CI"
# Il working tree Windows può averla in CRLF: git apply vuole i fine riga originali.
tr -d '\r' < /work/.github/patches/flutter_3.24.4_dropdown_menu_enableFilter.diff > /tmp/flutter.diff
PATCH=/tmp/flutter.diff
(cd /opt/flutter && { git apply --reverse --check "$PATCH" 2>/dev/null || git apply "$PATCH"; })

step "vcpkg ${VCPKG_COMMIT:0:12}"
export VCPKG_ROOT=/opt/vcpkg/vcpkg VCPKG_DEFAULT_BINARY_CACHE=/opt/vcpkg/cache VCPKG_DISABLE_METRICS=1
mkdir -p "$VCPKG_DEFAULT_BINARY_CACHE"
[ -d "$VCPKG_ROOT/.git" ] || git clone -q https://github.com/microsoft/vcpkg "$VCPKG_ROOT"
if [ "$(git -C "$VCPKG_ROOT" rev-parse HEAD)" != "$VCPKG_COMMIT" ]; then
  git -C "$VCPKG_ROOT" fetch -q origin
  git -C "$VCPKG_ROOT" checkout -q "$VCPKG_COMMIT"
  rm -f "$VCPKG_ROOT/vcpkg"
fi
[ -x "$VCPKG_ROOT/vcpkg" ] || "$VCPKG_ROOT/bootstrap-vcpkg.sh" -disableMetrics >/tmp/boot.log 2>&1 || fail_log /tmp/boot.log

step "dipendenze vcpkg $ABI (la prima volta: decine di minuti)"
if ! ./flutter/build_android_deps.sh "$ABI" >/tmp/vcpkg.log 2>&1; then
  tail -30 /tmp/vcpkg.log
  find "$VCPKG_ROOT/buildtrees" -name '*-err.log' -mmin -120 -exec tail -20 {} \; 2>/dev/null | tail -60
  exit 1
fi

step "bridge Rust<->Dart"
(cd flutter && flutter pub get >/tmp/pub.log 2>&1) || fail_log /tmp/pub.log 30
flutter_rust_bridge_codegen --rust-input ./src/flutter_ffi.rs \
  --dart-output ./flutter/lib/generated_bridge.dart \
  --c-output ./flutter/macos/Runner/bridge_generated.h >/tmp/frb.log 2>&1 \
  || { grep -v 'Skipping unresolvable' /tmp/frb.log | tail -40; exit 1; }

step "librustdesk $TARGET (variante ${CONN_TYPE:-?})"
# L'immagine base ha libyuv/opus per Linux con pkg-config: qui devono venire da
# vcpkg (Android), come sul runner della CI che non le ha.
# Solo per il target: le dipendenze di build per l'host usano ancora pkg-config.
TARGET_ENV="${TARGET//-/_}"
export "PKG_CONFIG_LIBDIR_${TARGET_ENV}=/nonexistent" "PKG_CONFIG_PATH_${TARGET_ENV}="
# bindgen per il target: header dell'NDK, non quelli di /usr/include dell'immagine.
export "BINDGEN_EXTRA_CLANG_ARGS_${TARGET_ENV}=--sysroot=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot"
export NO_PKG_CONFIG_libyuv=1 NO_PKG_CONFIG_opus=1 NO_PKG_CONFIG_vpx=1 NO_PKG_CONFIG_aom=1
bash "flutter/$NDK_SCRIPT" >/tmp/cargo.log 2>&1 || { grep -E -A7 '^error' /tmp/cargo.log | head -80; tail -5 /tmp/cargo.log; exit 1; }
JNI="flutter/android/app/src/main/jniLibs/$ABI"
mkdir -p "$JNI"
cp "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/$LIBCXX/libc++_shared.so" "$JNI/"
cp "/build/target/$TARGET/release/liblibrustdesk.so" "$JNI/librustdesk.so"

step "APK $PLATFORM"
# Come la CI: memoria di Gradle e firma di debug (solo nella copia di lavoro).
sed -i "s/org.gradle.jvmargs=-Xmx1024M/org.gradle.jvmargs=-Xmx2g/g" flutter/android/gradle.properties
sed -i "s/signingConfigs.release/signingConfigs.debug/g" flutter/android/app/build.gradle
(cd flutter && flutter build apk --release --target-platform "$PLATFORM" --split-per-abi --no-tree-shake-icons >/tmp/apk.log 2>&1) \
  || fail_log /tmp/apk.log 60
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1 | tr -d '\r')"
PREFIX=""; [ "${CONN_TYPE:-}" = incoming ] && PREFIX="host-"
NAME="clantodesk-${VERSION}-${PREFIX}${ARCH}.apk"
cp "flutter/build/app/outputs/flutter-apk/app-$ABI-release.apk" "/out/$NAME"
echo "APK=$NAME"
