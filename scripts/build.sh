#!/usr/bin/env bash
set -euo pipefail

echo "=== Claude Sessions - Install ==="
echo ""

# Check prerequisites
check_cmd() {
  if ! command -v "$1" &>/dev/null; then
    echo "Error: $1 is not installed."
    echo "$2"
    exit 1
  fi
}

check_cmd node "Install Node.js: https://nodejs.org/"
check_cmd npm "Install Node.js (includes npm): https://nodejs.org/"

if ! command -v cargo &>/dev/null; then
  if [ -f "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
  else
    echo "Error: Rust is not installed."
    echo "Install it: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
  fi
fi

# Navigate to project root
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

echo "Installing dependencies..."
npm install

echo ""
echo "Building production app..."
npx tauri build

APP_PATH="$PROJECT_DIR/src-tauri/target/release/bundle/macos/Claude Sessions.app"
DMG_PATH=$(find "$PROJECT_DIR/src-tauri/target/release/bundle/dmg/" -name "*.dmg" 2>/dev/null | head -1)

echo ""
echo "=== Build complete ==="
echo ""

if [ "$(uname)" = "Darwin" ]; then
  read -p "Install to /Applications? [y/N] " answer
  if [[ "$answer" =~ ^[Yy]$ ]]; then
    echo "Copying to /Applications..."
    rm -rf "/Applications/Claude Sessions.app"
    cp -r "$APP_PATH" /Applications/
    echo "Installed! You can find 'Claude Sessions' in your Applications folder."
  else
    echo "App built at:"
    echo "  $APP_PATH"
    [ -n "$DMG_PATH" ] && echo "  $DMG_PATH"
    echo ""
    echo "To install manually, drag 'Claude Sessions.app' to /Applications."
  fi
else
  echo "App built at:"
  echo "  $PROJECT_DIR/src-tauri/target/release/bundle/"
fi
