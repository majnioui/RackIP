#!/bin/sh
# Build build/RackIP-<version>.dmg with the app and an Applications shortcut.
set -eu

cd "$(dirname "$0")/.."
VERSION=$(cat VERSION)
DMG="build/RackIP-$VERSION.dmg"

scripts/bundle.sh

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
cp -R build/RackIP.app "$STAGE/"
ln -s /Applications "$STAGE/Applications"

rm -f "$DMG"
hdiutil create -volname "RackIP $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
echo "Built $DMG"
