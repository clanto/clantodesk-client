#!/usr/bin/env bash
# Costruisce la Camera Extension di ClantoDesk (flutter/macos/CameraExtension),
# la firma e la incorpora nell'app; prepara gli entitlement dell'app per
# sign-macos-app.sh. Va eseguito prima della firma dell'app.
#
#   build-macos-camera-extension.sh <ClantoDesk.app> <identità> <team-id> <x86_64|aarch64> <entitlement-app-out>
#
# Ambiente: MACOS_APP_PROVISIONING_PROFILE_BASE64, MACOS_CAMERA_PROVISIONING_PROFILE_BASE64
# (profili Developer ID con la capability System Extension e l'App Group
# <team-id>.it.clanto.clantodesk), VERSION.
set -euo pipefail

app=$1
identity=$2
team=$3
arch=$4
app_entitlements_out=$5

ext_id=it.clanto.clantodesk.camera
group="$team.it.clanto.clantodesk"
src="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../flutter/macos/CameraExtension" && pwd)"
case "$arch" in
  aarch64) swift_arch=arm64 ;;
  x86_64) swift_arch=x86_64 ;;
  *) echo "architettura non valida: $arch" >&2; exit 1 ;;
esac
[[ "$team" =~ ^[A-Z0-9]{10}$ ]] || { echo "team id non valido" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
bundle="$work/$ext_id.systemextension"
mkdir -p "$bundle/Contents/MacOS"

xcrun swiftc -O -target "$swift_arch-apple-macos13.0" -sdk "$(xcrun --show-sdk-path --sdk macosx)" \
  "$src/main.swift" -o "$bundle/Contents/MacOS/$ext_id"
sed -e "s/__TEAM__/$team/g" -e "s/__VERSION__/${VERSION:?}/g" "$src/Info.plist" > "$bundle/Contents/Info.plist"
plutil -lint "$bundle/Contents/Info.plist" >/dev/null

printf '%s' "${MACOS_CAMERA_PROVISIONING_PROFILE_BASE64:?}" | tr -d '[:space:]' | base64 --decode \
  > "$bundle/Contents/embedded.provisionprofile"
sed "s/__TEAM__/$team/g" "$src/CameraExtension.entitlements" > "$work/extension.entitlements"
codesign --force --options runtime --timestamp --sign "$identity" \
  --entitlements "$work/extension.entitlements" "$bundle"
codesign --verify --strict --verbose=2 "$bundle"

target="$app/Contents/Library/SystemExtensions"
mkdir -p "$target"
rm -rf "${target:?}/$ext_id.systemextension"
cp -R "$bundle" "$target/"
printf '%s' "${MACOS_APP_PROVISIONING_PROFILE_BASE64:?}" | tr -d '[:space:]' | base64 --decode \
  > "$app/Contents/embedded.provisionprofile"

# Entitlement dell'app: quelli di Release più installazione dell'estensione e App Group.
cp "$(dirname "${BASH_SOURCE[0]}")/../../flutter/macos/Runner/Release.entitlements" "$app_entitlements_out"
pb() { /usr/libexec/PlistBuddy -c "$1" "$app_entitlements_out"; }
pb "Add :com.apple.application-identifier string $team.it.clanto.clantodesk"
pb "Add :com.apple.developer.team-identifier string $team"
pb "Add :com.apple.developer.system-extension.install bool true"
pb "Add :com.apple.security.application-groups array"
pb "Add :com.apple.security.application-groups:0 string $group"
plutil -lint "$app_entitlements_out" >/dev/null
echo "Camera Extension incorporata: $target/$ext_id.systemextension"
