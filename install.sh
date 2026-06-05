#!/usr/bin/env bash
# Cài đặt JakShell
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo chưa được cài. Cài Rust trước qua https://rustup.rs" >&2
  exit 1
fi

echo "→ build (release)…"
cargo build --release

BIN="$SCRIPT_DIR/target/release/jaksh"
DEST="${PREFIX:-$HOME/.local/bin}"
mkdir -p "$DEST"
cp -f "$BIN" "$DEST/jaksh"
echo "✓ đã cài: $DEST/jaksh"

# Sample config nếu chưa có
if [[ ! -f "$HOME/.jakshrc.toml" ]]; then
  cp "$SCRIPT_DIR/examples/jakshrc.toml" "$HOME/.jakshrc.toml"
  echo "✓ tạo ~/.jakshrc.toml mẫu"
fi
if [[ ! -f "$HOME/.jakshrc" ]]; then
  cp "$SCRIPT_DIR/examples/jakshrc" "$HOME/.jakshrc"
  echo "✓ tạo ~/.jakshrc mẫu"
fi

echo
echo "Chạy thử bằng:  $DEST/jaksh"
echo "Đặt làm shell mặc định (tuỳ chọn):"
echo "  echo $DEST/jaksh | sudo tee -a /etc/shells"
echo "  chsh -s $DEST/jaksh"
