#!/usr/bin/env bash
# Build a minimal .app bundle, zip, and DMG for Apple Silicon macOS.
set -euo pipefail

VERSION="${1:?version required}"
BINARY="${2:?path to release binary required}"
OUT_DIR="${3:?output directory required}"

APP_NAME="PLSQL IDE"
APP_DIR="${OUT_DIR}/${APP_NAME}.app"
STAGE="${OUT_DIR}/dmg-root"

rm -rf "${APP_DIR}" "${STAGE}"
mkdir -p "${APP_DIR}/Contents/MacOS"
mkdir -p "${STAGE}"

sed "s/VERSION_PLACEHOLDER/${VERSION}/g" packaging/macos/Info.plist \
  > "${APP_DIR}/Contents/Info.plist"

cp "${BINARY}" "${APP_DIR}/Contents/MacOS/plsqlide"
chmod +x "${APP_DIR}/Contents/MacOS/plsqlide"

cp -R "${APP_DIR}" "${STAGE}/"

ZIP_PATH="${OUT_DIR}/plsqlide-${VERSION}-macos-aarch64.zip"
DMG_PATH="${OUT_DIR}/plsqlide-${VERSION}-macos-aarch64.dmg"

rm -f "${ZIP_PATH}" "${DMG_PATH}"
ditto -c -k --sequesterRsrc --keepParent "${APP_DIR}" "${ZIP_PATH}"

hdiutil create \
  -volname "${APP_NAME}" \
  -srcfolder "${STAGE}" \
  -ov \
  -format UDZO \
  "${DMG_PATH}"

echo "Created ${ZIP_PATH}"
echo "Created ${DMG_PATH}"
