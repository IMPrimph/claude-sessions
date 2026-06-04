#!/usr/bin/env bash
set -euo pipefail

# Prints download + rough-usage stats for Claude Sessions, pulled live from the
# GitHub Releases API. No tracking, no third party — just the counts GitHub already
# keeps for every release asset.
#
#   .dmg / .app.tar.gz downloads  →  installs
#   latest.json on the newest release  →  update-checks (a proxy for app launches,
#                                          since the auto-updater fetches it on start)
#
# Requires the GitHub CLI (`gh`) to be installed and authenticated.

REPO="IMPrimph/claude-sessions"

if ! command -v gh &>/dev/null; then
  echo "Error: GitHub CLI (gh) is not installed — https://cli.github.com/"
  exit 1
fi

echo "=== Claude Sessions — download stats ==="
echo "Repo: $REPO"
echo ""

# Pull every release with its assets in one call.
releases_json=$(gh api "repos/$REPO/releases" --paginate)

# Per-release table: installs (dmg) + updater bundles (.app.tar.gz) + launch proxy.
echo "Per release:"
printf "  %-10s %-12s %8s %8s %12s\n" "TAG" "PUBLISHED" "INSTALLS" "BUNDLES" "LAUNCHCHECKS"
echo "$releases_json" | jq -r '
  .[]
  | . as $release
  | {
      tag: .tag_name,
      published: (.published_at | split("T")[0]),
      installs: ([.assets[] | select(.name | endswith(".dmg")) | .download_count] | add // 0),
      bundles: ([.assets[] | select(.name | endswith(".app.tar.gz")) | .download_count] | add // 0),
      launchchecks: ([.assets[] | select(.name == "latest.json") | .download_count] | add // 0)
    }
  | "  \(.tag)\t\(.published)\t\(.installs)\t\(.bundles)\t\(.launchchecks)"
' | awk -F'\t' '{ printf "  %-10s %-12s %8s %8s %12s\n", $1, $2, $3, $4, $5 }'

echo ""

# Totals.
total_installs=$(echo "$releases_json" | jq '[.[].assets[] | select(.name | endswith(".dmg")) | .download_count] | add // 0')
total_bundles=$(echo "$releases_json" | jq '[.[].assets[] | select(.name | endswith(".app.tar.gz")) | .download_count] | add // 0')

# Launch proxy = latest.json count on the most recently published, non-draft release,
# because the auto-updater always fetches /releases/latest/download/latest.json.
latest_launchchecks=$(echo "$releases_json" | jq '
  [.[] | select(.draft == false)]
  | sort_by(.published_at) | reverse | .[0]
  | [.assets[] | select(.name == "latest.json") | .download_count] | add // 0
')
latest_tag=$(echo "$releases_json" | jq -r '
  [.[] | select(.draft == false)] | sort_by(.published_at) | reverse | .[0].tag_name
')

echo "Totals:"
echo "  DMG installs (all releases):     $total_installs"
echo "  Updater bundles (auto-updates):  $total_bundles"
echo "  Launch-checks on $latest_tag:    $latest_launchchecks  (rough active-usage proxy)"
echo ""
echo "Note: GitHub only exposes a point-in-time snapshot — run this anytime for the"
echo "current numbers. For history over time, enable the track-downloads workflow."
