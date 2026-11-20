#!/bin/bash
# Deck'i derler ve Deck.app oluşturur.
# Yerelde: Xcode veya "xcode-select --install" gerekir.
# GitHub Actions'ta da aynı betik çalışır (CI ortam değişkeni varsa uygulamayı açmaz).
set -euo pipefail
cd "$(dirname "$0")"

APP="Deck.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

swiftc -O -parse-as-library -swift-version 5 \
  -target arm64-apple-macos15.0 \
  Deck.swift \
  -o "$APP/Contents/MacOS/Deck"

cp Deck-Info.plist "$APP/Contents/Info.plist"   # paket içinde adı Info.plist olmak zorunda
cp Deck.icns "$APP/Contents/Resources/Deck.icns"
codesign --force --deep --sign - "$APP"

echo "Hazır: $(pwd)/$APP"

if [ -z "${CI:-}" ]; then
  # Sistem menüleri de Türkçe açılsın
  defaults write com.idris.deck AppleLanguages -array tr
  touch "$APP"
  open "$APP"
fi
