#!/bin/zsh
# Compila Carrusel en modo release y lo empaqueta como Carrusel.app
# Uso:  ./build_mac.sh            (para tu Mac)
#       ./build_mac.sh universal  (Apple Silicon + Intel)
set -e
cd "$(dirname "$0")"

APP="Carrusel"
BIN="carrusel"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

if [[ "$1" == "universal" ]]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  mkdir -p target/universal
  lipo -create -output "target/universal/$BIN" \
    "target/aarch64-apple-darwin/release/$BIN" \
    "target/x86_64-apple-darwin/release/$BIN"
  BUILT="target/universal/$BIN"
else
  cargo build --release
  BUILT="target/release/$BIN"
fi

OUT="dist/$APP.app"
rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources"
cp "$BUILT" "$OUT/Contents/MacOS/$BIN"

cat > "$OUT/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP</string>
  <key>CFBundleDisplayName</key><string>$APP</string>
  <key>CFBundleIdentifier</key><string>com.carrusel.app</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>$BIN</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.photography</string>
</dict>
</plist>
PLIST

# Icono opcional: coloca un icon.png de 1024x1024 junto a este script.
if [[ -f icon.png ]]; then
  ICONSET=$(mktemp -d)/AppIcon.iconset
  mkdir -p "$ICONSET"
  for s in 16 32 128 256 512; do
    sips -z $s $s icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
    sips -z $((s*2)) $((s*2)) icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
  done
  iconutil -c icns "$ICONSET" -o "$OUT/Contents/Resources/AppIcon.icns"
  /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string AppIcon" "$OUT/Contents/Info.plist"
fi

# Firma local (ad-hoc) para que macOS la deje abrir en tu propio Mac.
codesign --force --deep --sign - "$OUT"

echo "Listo: $OUT"
echo "Arrástrala a /Applications o ábrela con:  open \"$OUT\""
