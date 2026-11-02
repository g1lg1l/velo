#!/usr/bin/env bash
# Velo.app in a .dmg, and in a .tar.gz for self-updates (ad-hoc signed, not notarized), from
# target/release: the release workflow's macOS step, and a local release build's.
#   scripts/package-macos.sh <version> [name]     e.g. 0.9.0-beta macos-arm64
set -euo pipefail
VERSION="$1"
NAME="${2:-macos-arm64}"
cd "$(dirname "$0")/.."
rm -rf dmg tar
app=dmg/Velo.app
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" dist
cp target/release/velo target/release/velo-mcp "$app/Contents/MacOS/"
cp assets/icons/macos/velo.icns "$app/Contents/Resources/velo.icns"
# the usage strings make macOS ask for (and name Velo in) the audio permissions (spike S3)
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Velo</string>
  <key>CFBundleDisplayName</key><string>Velo</string>
  <key>CFBundleIdentifier</key><string>io.github.g1lg1l.velo</string>
  <key>CFBundleExecutable</key><string>velo</string>
  <key>CFBundleIconFile</key><string>velo</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Velo listens to your audio input to make the visuals react to the music.</string>
  <key>NSAudioCaptureUsageDescription</key><string>Velo listens to what your computer plays (Spotify, a DJ app) to make the visuals react to it.</string>
  <key>CFBundleDocumentTypes</key>
  <array><dict>
    <key>CFBundleTypeName</key><string>Velo project</string>
    <key>CFBundleTypeRole</key><string>Editor</string>
    <key>LSHandlerRank</key><string>Owner</string>
    <key>LSTypeIsPackage</key><true/>
    <key>LSItemContentTypes</key><array><string>io.github.g1lg1l.velo.project</string></array>
  </dict></array>
  <key>UTExportedTypeDeclarations</key>
  <array><dict>
    <key>UTTypeIdentifier</key><string>io.github.g1lg1l.velo.project</string>
    <key>UTTypeDescription</key><string>Velo project</string>
    <key>UTTypeConformsTo</key><array><string>com.apple.package</string></array>
    <key>UTTypeTagSpecification</key><dict><key>public.filename-extension</key><array><string>velo</string></array></dict>
  </dict></array>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$app"
# the tarball is what Velo updates itself from (a .dmg would need mounting)
dir="velo-$VERSION-$NAME"
mkdir -p "tar/$dir"
ditto "$app" "tar/$dir/Velo.app"
COPYFILE_DISABLE=1 tar -czf "dist/$dir.tar.gz" -C tar "$dir"
ln -s /Applications dmg/Applications
hdiutil create -volname Velo -srcfolder dmg -ov -format UDZO "dist/velo-$VERSION-$NAME.dmg"
# the build folders, once the disk image and the tarball are made
rm -rf dmg tar
