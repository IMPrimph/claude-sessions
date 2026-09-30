# Releasing Evertusk

How a release is cut. **Releases are triggered by pushing a `v*` git tag** —
GitHub Actions (`.github/workflows/release.yml`) does the macOS build and
publishes the GitHub Release. You do not build the `.dmg` locally.

## TL;DR

```bash
# 1. bump version in the 4 files below (+ sync Cargo.lock)
# 2. verify gates pass
# 3. commit to main, tag, push tag -> CI builds & publishes
git commit -am "release: vX.Y.Z — <summary>"
git tag -a vX.Y.Z -m "Evertusk vX.Y.Z"
git push origin main
git push origin vX.Y.Z          # <-- this is what triggers the release build
```

## Versioning

Semver. **Fix → patch** (1.1.0 → 1.1.1). **New feature → minor** (1.0.1 →
1.1.0). Big/breaking → major.

Bump the version in **all four** of these, then sync the lockfile:

| File | Field |
|---|---|
| `package.json` | `"version"` |
| `src-tauri/tauri.conf.json` | `"version"` — this is the one the app reports at runtime |
| `src-tauri/Cargo.toml` | `version` under `[package]` |
| `website/src/pages/index.astro` | `const VERSION` — drives the download link + JSON-LD |

```bash
OLD=1.1.0 ; NEW=1.1.1
sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" package.json src-tauri/tauri.conf.json
sed -i '' "s/^version = \"$OLD\"/version = \"$NEW\"/" src-tauri/Cargo.toml
sed -i '' "s/const VERSION = \"$OLD\"/const VERSION = \"$NEW\"/" website/src/pages/index.astro
( cd src-tauri && cargo check --quiet )      # updates Cargo.lock to the new version
# sanity: no old version left anywhere in source
grep -rn "$OLD" package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json website/src/pages/index.astro src/ src-tauri/src/
```

> The in-app version badge (top-left of the project grid) reads the version
> **dynamically** via `getVersion()` from `@tauri-apps/api/app` (backed by
> `tauri.conf.json`) — so there is **no hardcoded version string in the frontend
> to bump**. It was hardcoded once; that shipped the wrong number in a public
> release (see Lessons). Keep it dynamic.

## Gates (run before tagging — evidence, not vibes)

```bash
( cd src-tauri && cargo test --lib --quiet )                 # Rust unit tests
npm run build                                                # frontend (vite) build
npm run check                                                # type check (svelte-check) — must report 0 errors
( cd src-tauri && cargo clippy --all-targets -- -D warnings ) # lint — must be clean
```

The same checks run on every push via `.github/workflows/ci.yml`. (`svelte-check@4.1.0`
crashes with TypeScript 5.9; the "phantom `never`" errors newer versions reported were
real — `$state(null)` narrowed the variable type — and are fixed with `$state<T | null>(null)`.)

## Cut the release

```bash
git add -A
git commit -m "release: vX.Y.Z — <one-line summary>"
git tag -a vX.Y.Z -m "Evertusk vX.Y.Z"
git push origin main
git push origin vX.Y.Z          # triggers .github/workflows/release.yml
```

Commit trailer (per this account's attribution rule):
`Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`

## What the CI does (`release.yml`)

Triggered by any tag matching `v*`. On `macos-latest`:

- Builds with `tauri-apps/tauri-action@v0`, target **`universal-apple-darwin`**
  (one app for Apple Silicon + Intel).
- Signs + notarizes with Apple **only when** the `APPLE_*` secrets exist (see the
  "Apple signing" step); without them the build is unsigned, as before.
- Produces `Evertusk_X.Y.Z_universal.dmg`, the `.app.tar.gz` + `.sig`, and
  `latest.json` (the auto-updater manifest, `includeUpdaterJson: true`).
- Signs the updater artifacts with repo secrets **`TAURI_SIGNING_PRIVATE_KEY`**
  and **`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`** (already configured; if a build
  fails, check these first).
- Creates a GitHub Release named `Evertusk vX.Y.Z` and **publishes it immediately**
  (`releaseDraft: false`). Draft releases were the old default and are why the site's
  download link 404s for v1.1.0 — nobody clicked Publish, so the asset URL never existed.

Watch it:

```bash
gh run list -R IMPrimph/claude-sessions --workflow=release.yml --limit 1
gh release view vX.Y.Z -R IMPrimph/claude-sessions --json isDraft,assets
```

Build takes ~4–6 min.

## After it publishes

- **Auto-update:** existing installs pull `latest.json` and update to the new
  version. No action needed.
- **Website:** the `main` push auto-redeploys the site on Vercel
  (`claude-sessions-blond.vercel.app`); the download button now points at
  `vX.Y.Z/Evertusk_X.Y.Z_universal.dmg`. That asset only exists once the
  release is published — the link 404s until then.
- **Install stats:** a scheduled workflow (`track-downloads.yml`) commits daily
  `stats/downloads.jsonl` snapshots (`[skip ci]`); the site reads the latest for
  its install count.

## Lessons (don't repeat these)

- **A published release is immutable.** If a bad build ships, do **not** delete
  the release / retag the same version — cut the next patch instead. (v1.0.0
  shipped with a wrong in-app badge; the fix went out as v1.0.1, not a v1.0.0
  do-over.)
- **Grep for the old version across _all_ file types**, not just json/toml — a
  hardcoded version once hid in a `.svelte` file and reached a public build.
- **Push the tag last**, after `main`, so the CI checks out a commit that's
  already on the branch.
- **Env / auth:** the remote is `github-personal:IMPrimph/claude-sessions`
  (SSH host alias); `gh` is authed as `vishnu-10x`. Push uses the SSH key, `gh`
  uses its own token — both already set up on this machine.
