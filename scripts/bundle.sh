#!/bin/sh
# Build RackIP.app in build/.
#
#   scripts/bundle.sh                          # ad-hoc signed, for this Mac
#   SIGN_IDENTITY="Developer ID Application: …" scripts/bundle.sh
#
# Building from source avoids Gatekeeper prompts; a downloaded build signed
# ad-hoc needs right-click › Open the first time.
set -eu

cd "$(dirname "$0")/.."
BUNDLE_ID="${BUNDLE_ID:-com.majnioui.rackip}"
VERSION=$(cat VERSION)
APP=build/RackIP.app

swift build -c release --product rackip

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$(swift build -c release --show-bin-path)/rackip" "$APP/Contents/MacOS/rackip"
[ -f assets/AppIcon.icns ] || swift scripts/make-icon.swift assets
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>RackIP</string>
    <key>CFBundleDisplayName</key>
    <string>RackIP</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleExecutable</key>
    <string>rackip</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.utilities</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

if [ -n "${SIGN_IDENTITY:-}" ]; then
    # Apple's timestamp service only stamps Apple-issued certificates; a
    # self-signed one signs without it.
    case "$SIGN_IDENTITY" in
        "Developer ID"*) stamp=--timestamp ;;
        *) stamp=--timestamp=none ;;
    esac
    codesign --force --options runtime "$stamp" --sign "$SIGN_IDENTITY" "$APP"
else
    codesign --force --sign - "$APP"
fi
codesign --verify --strict "$APP"
echo "Built $APP ($VERSION, $BUNDLE_ID)"
