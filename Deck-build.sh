#!/bin/bash
# Deck'i derler ve Deck.app oluşturur (Apple Silicon).
# Yerelde: Rust gerekir  →  curl https://sh.rustup.rs -sSf | sh   (ve Xcode komut satırı araçları)
# GitHub Actions'ta da aynı betik çalışır (CI ortam değişkeni varsa uygulamayı açmaz).
set -euo pipefail
cd "$(dirname "$0")"

APP="Deck.app"
TARGET="aarch64-apple-darwin"

rustup target add "$TARGET" >/dev/null 2>&1 || true
cargo build --release --target "$TARGET"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/$TARGET/release/Deck" "$APP/Contents/MacOS/Deck"
cp Deck-Info.plist "$APP/Contents/Info.plist"   # paket içinde adı Info.plist olmak zorunda
cp Deck.icns "$APP/Contents/Resources/Deck.icns"
codesign --force --deep --sign - "$APP"

echo "Hazır: $(pwd)/$APP"

if [ -z "${CI:-}" ]; then
  touch "$APP"
  open "$APP"
fi
