#!/usr/bin/env bash
# Prepares a web build on Flutter 3.44.x. Companion to
# apply_flutter_3.44_source_patches.sh (which it runs first): the web target
# additionally needs flutter/web/fonts refreshed with
# the font paths the 3.44 engine requests for offline/air-gapped support
# (rustdesk-server-pro#996; see flutter/web/fonts/sync_fonts.py).
#
# Run from the repository root with Flutter 3.44.x on PATH, then build:
#   bash .github/patches/apply_flutter_3.44_web_patches.sh
#   (cd flutter && flutter build web --release)   # or ./web/js/flutter_build.py
#
# Idempotent. To undo the source changes locally:
#   git checkout -- flutter/lib/common.dart flutter/pubspec.yaml flutter/pubspec.lock
set -euo pipefail

flutter --version | grep -q "Flutter 3\.44\." || {
  echo "Flutter 3.44.x must be on PATH; found:" >&2
  flutter --version | grep "^Flutter" >&2 || true
  exit 1
}

# Shared 3.44 source/pubspec patches own their complete-state validation.
bash .github/patches/apply_flutter_3.44_source_patches.sh

# Populate the pub cache with the 3.44 dependency resolution.
(cd flutter && flutter pub get)

# Mirror the fonts this engine version requests into flutter/web/fonts.
python3 flutter/web/fonts/sync_fonts.py

# Fail loudly if any expected state is missing:
grep -qF 'google_fonts: 8.1.0' flutter/pubspec.yaml

echo "Flutter 3.44 web patches applied."
