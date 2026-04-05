#!/usr/bin/env bash
set -euo pipefail

REPO="IMPrimph/claude-sessions"
APP_NAME="Claude Sessions"

echo "=== $APP_NAME - Installer ==="
echo ""

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "$OS" != "Darwin" ]; then
  echo "Error: Only macOS is supported currently."
  echo "For other platforms, build from source: ./scripts/build.sh"
  exit 1
fi

# Map architecture
if [ "$ARCH" = "arm64" ]; then
  ARCH_FILTER="aarch64"
elif [ "$ARCH" = "x86_64" ]; then
  ARCH_FILTER="x86_64"
else
  echo "Error: Unsupported architecture: $ARCH"
  exit 1
fi

# Get latest release DMG URL matching architecture
echo "Fetching latest release for $ARCH..."
DOWNLOAD_URL=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" \
  | grep "browser_download_url.*${ARCH_FILTER}.*\.dmg" \
  | head -1 \
  | cut -d '"' -f 4)

if [ -z "$DOWNLOAD_URL" ]; then
  echo "No pre-built release found for your platform."
  echo ""
  echo "Build from source instead:"
  echo "  git clone https://github.com/$REPO.git"
  echo "  cd claude-sessions && ./scripts/build.sh"
  exit 1
fi

# Download
TMPDIR=$(mktemp -d)
DMG_PATH="$TMPDIR/claude-sessions.dmg"

echo "Downloading $(basename "$DOWNLOAD_URL")..."
curl -L --progress-bar -o "$DMG_PATH" "$DOWNLOAD_URL"

# Mount and install
echo "Installing..."
MOUNT_POINT=$(hdiutil attach "$DMG_PATH" -nobrowse | tail -1 | awk -F'\t' '{print $NF}')

if [ -d "/Applications/$APP_NAME.app" ]; then
  rm -rf "/Applications/$APP_NAME.app"
fi

cp -r "$MOUNT_POINT/$APP_NAME.app" /Applications/

hdiutil detach "$MOUNT_POINT" -quiet
rm -rf "$TMPDIR"

echo ""
echo "Installed! Open '$APP_NAME' from your Applications folder."
echo "Or run: open '/Applications/$APP_NAME.app'"
