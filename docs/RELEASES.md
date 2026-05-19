# Woom releases — operator runbook

Source of truth: SDD workspace `sdd-2508eeb82e` (spec + plan + phase
files under `~/Library/Application Support/com.woom.desktop/sdd-workspaces/`).
This file is the field-runbook for cutting + understanding macOS
auto-updates.

## Trust model — TL;DR

| What | Where | Required? | Rotation |
|---|---|---|---|
| ed25519 **public key** | baked into the app binary via `tauri.conf.json` → `plugins.updater.pubkey` | **Yes** | New release that ships a new pubkey. Old installs trust signatures matching the old key until upgraded. |
| ed25519 **private key** | GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY` (+ `..._PASSWORD`) — never on disk in this repo | **Yes** | Generate a new keypair, commit the new pubkey, rotate the secret. Bumps the trust root for all future installs. |
| Apple **Developer ID cert** | secret `APPLE_CERTIFICATE` (+ `..._PASSWORD`) | Optional | Apple manages — renew yearly via developer.apple.com. |
| Apple **notarization creds** | secrets `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Optional | App-specific password rotated as Apple expires it. |

The ed25519 pair is the only **required** trust root. When the Apple
secrets are absent, the release workflow skips `codesign --verify`,
`notarytool submit`, and `stapler staple`; the DMG ships ad-hoc-signed
and users see a one-time Gatekeeper warning (right-click → Open, or
`xattr -dr com.apple.quarantine /Applications/Woom.app`). The auto-
update path is unaffected because Tauri's updater trusts the ed25519
signature directly.

> Phase 1's job is the public-key half of the trust root. Phase 2
> wires the CI secrets. Phase 5 adds a smoke test that catches a
> pubkey/private-key parity mismatch BEFORE shipping.

## One-time setup (maintainer)

Run the following ONCE locally — the artifacts are personal to
the maintainer and never land in this repo. Both `*.tauri.key` and
`*.tauri.key.pub` are git-ignored as a safety net.

```bash
# 1. Generate the ed25519 keypair. Pick a memorable passphrase
#    or `--no-password` for a CI-friendly key (recommended;
#    GitHub secrets are already encrypted at rest).
pnpm --filter @woom/desktop tauri signer generate -w ~/.tauri/woom-update.key

# 2. Copy the public key VERBATIM (single base64 line, drop the
#    `untrusted comment:` header) into:
#       apps/desktop/src-tauri/tauri.conf.json
#       → plugins.updater.pubkey
#    The placeholder string today is
#    `PLACEHOLDER_REPLACE_WITH_TAURI_SIGNER_OUTPUT`.
cat ~/.tauri/woom-update.key.pub
# → untrusted comment: minisign public key XXXXXXXX
# → RWQ...            <-- this is what goes in tauri.conf.json

# 3. Commit + push the updated tauri.conf.json. From this point
#    on, the running app trusts ONLY signatures made by the
#    matching private key.

# 4. Upload the private key + password to GitHub Actions secrets
#    (Settings → Actions → Secrets → New repository secret):
#      TAURI_SIGNING_PRIVATE_KEY            <-- contents of ~/.tauri/woom-update.key
#      TAURI_SIGNING_PRIVATE_KEY_PASSWORD   <-- passphrase (or empty)
```

After step 3, `pnpm tauri build` will refuse to sign updates if
the CLI can't see the private key — that's the right failure
mode for a clean dev environment.

## Manifest schema

The updater plugin fetches one JSON file per release. Woom
publishes it as a release asset named `latest-mac.json` (so the
stable URL `…/releases/latest/download/latest-mac.json` always
resolves to the newest release's manifest).

Shape we ship:

```json
{
  "version": "1.4.2",
  "notes": "Markdown release notes — escaped newlines.\n\n- Bullet one\n- Bullet two",
  "pub_date": "2026-05-19T12:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<base64 ed25519 signature over the .dmg bytes>",
      "url": "https://github.com/walpakhart/Woom/releases/download/v1.4.2/Woom_1.4.2_aarch64.dmg",
      "sha256": "<lowercase hex sha256 of the .dmg bytes>"
    },
    "darwin-x86_64": {
      "signature": "…",
      "url": "https://github.com/walpakhart/Woom/releases/download/v1.4.2/Woom_1.4.2_x86_64.dmg",
      "sha256": "…"
    }
  }
}
```

### Field-by-field

- `version` — bare semver, no leading `v`. Must match
  `apps/desktop/package.json`'s `version` AND
  `apps/desktop/src-tauri/Cargo.toml`'s `version`.
- `notes` — Markdown. The release-notes pane (Phase 4) renders this
  via `Markdown.svelte`, so GFM features (tables, fenced code,
  task lists) work.
- `pub_date` — RFC3339. Surfaced in the release-notes pane header.
- `platforms.<target>` — Tauri's target triple convention:
  - `darwin-aarch64` for Apple Silicon (M-series).
  - `darwin-x86_64` for Intel.
  - We ship a universal binary; the same DMG fulfils both entries,
    so `url` will be identical across keys. Tauri's plugin still
    needs both entries present to match per-arch.
- `platforms.<target>.signature` — base64 ed25519 from
  `tauri signer sign`. Output suffixed with `.sig` next to the DMG.
- `platforms.<target>.url` — direct download URL of the DMG.
- `platforms.<target>.sha256` — **Woom-specific.** The Tauri
  plugin doesn't read it. Our custom verification hook (Phase 5,
  `updater.rs::verify_download`) compares the computed sha256 of
  the downloaded bytes against this field before delegating to
  `Update.install()`. Defence-in-depth — protects against a private-
  key leak that the attacker also uses to forge signatures.

## GitHub Actions secrets (one-time setup)

Before the first automated release, populate Repository Settings →
Secrets and variables → Actions:

| Secret | What it is | Where it comes from |
|---|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | ed25519 private key (full file contents) | `cat ~/.tauri/woom-update.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | passphrase used when generating the key (empty OK) | maintainer chose at `tauri signer generate` time |
| `APPLE_CERTIFICATE` | base64-encoded Developer ID Application `.p12` | `base64 -i DeveloperID.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | `.p12` export password | maintainer chose at export time |
| `APPLE_SIGNING_IDENTITY` | full cert CN | e.g. `Developer ID Application: Nikolay Khartanovich (ABCDE12345)` |
| `APPLE_ID` | Apple developer account email | appleid.apple.com |
| `APPLE_PASSWORD` | app-specific password for notarytool | appleid.apple.com → App-Specific Passwords |
| `APPLE_TEAM_ID` | 10-char Apple team id | developer.apple.com → Membership |

Verify the set by triggering `release-verify.yml` manually (Actions →
Release verify → Run workflow) — it builds + dry-runs the manifest
WITHOUT touching the Apple secrets, so a failure is purely structural.

## Cutting a release

CI does all the work once a `v*` tag is pushed. Steps for the
maintainer:

```bash
# 1. Bump versions in sync.
#    apps/desktop/package.json          → "version": "1.4.2"
#    apps/desktop/src-tauri/Cargo.toml  → version = "1.4.2"
#    apps/desktop/src-tauri/tauri.conf.json → "version": "1.4.2"
#    (Cargo.lock will bump automatically on next build.)

# 2. Update CHANGELOG.md with notes under "## 1.4.2".
#    The manifest builder extracts this section verbatim into
#    `latest-mac.json` → `notes`. Markdown is allowed.

# 3. Commit + tag + push.
git add apps/desktop/package.json apps/desktop/src-tauri/Cargo.toml \
        apps/desktop/src-tauri/tauri.conf.json CHANGELOG.md
git commit -m "release: 1.4.2"
git tag v1.4.2
git push origin main --tags

# 4. Watch the workflow at /actions/workflows/release.yml — about
#    8-15 min on a cold cache (sidecars + universal Tauri build +
#    notarize wait). Cached runs ~5 min.

# 5. Verify the release at /releases/latest:
#    - Woom_1.4.2_universal.dmg
#    - Woom_1.4.2_universal.dmg.sig
#    - latest-mac.json
#    All three must be present.

# 6. From a 1.4.1 install: Settings → Updates → Check for updates.
#    Should pick up 1.4.2 within seconds.
```

The CI flow (`.github/workflows/release.yml`):

1. Checkout + cache restore (cargo + pnpm).
2. `pnpm sidecars:release` then `tauri build --target universal-apple-darwin`.
3. Tauri's bundler signs the DMG with the ed25519 private key from
   `TAURI_SIGNING_PRIVATE_KEY` AND code-signs the `.app` with the
   Apple Developer ID certificate from `APPLE_CERTIFICATE`.
4. `xcrun notarytool submit … --wait` for Apple notarization.
5. `xcrun stapler staple` bakes the notarization ticket into the
   DMG so offline Gatekeeper checks pass.
6. `node scripts/make-manifest.mjs --dmg <path> --signature <path>.sig …`
   assembles `dist/latest-mac.json` with the exact schema documented
   above, including the sha256 defence-in-depth field.
7. `softprops/action-gh-release@v2` creates the GitHub Release and
   attaches all three artifacts. The auto-generated release notes
   (from commits since the previous tag) live alongside the explicit
   CHANGELOG-extracted `notes` field in the manifest.

If a step fails, the tag stays on the repo but the release is NOT
created — fix the issue, delete the tag (`git push --delete origin
v1.4.2`), and retry. Don't rerun the workflow against a partial
release artifact — that's how you ship something that fails
signature verification on user installs.

## Manual fallback (when CI is down)

Should the workflow be unavailable, the same steps run locally:

```bash
pnpm install --frozen-lockfile
pnpm sidecars:release
APPLE_SIGNING_IDENTITY="Developer ID Application: <Name> (<TEAMID>)" \
  pnpm -C apps/desktop tauri build --target universal-apple-darwin
xcrun notarytool submit \
  apps/desktop/src-tauri/target/universal-apple-darwin/release/bundle/dmg/Woom_1.4.2_universal.dmg \
  --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" --wait
xcrun stapler staple \
  apps/desktop/src-tauri/target/universal-apple-darwin/release/bundle/dmg/Woom_1.4.2_universal.dmg
node scripts/make-manifest.mjs \
  --dmg apps/desktop/src-tauri/target/universal-apple-darwin/release/bundle/dmg/Woom_1.4.2_universal.dmg \
  --signature apps/desktop/src-tauri/target/universal-apple-darwin/release/bundle/dmg/Woom_1.4.2_universal.dmg.sig \
  --version 1.4.2 --tag v1.4.2 --repo walpakhart/Woom \
  --changelog CHANGELOG.md --out dist/latest-mac.json
# Upload the three artifacts via `gh release create v1.4.2 …` or the
# web UI; mark NOT prerelease so `releases/latest/download/` resolves.
```

## Future scope (Sparkle, dual-key, etc.)

- **Sparkle / delta updates** — only worth switching if the binary
  grows past ~150 MB or installs need to support older macOS than
  Tauri can target. Today the universal DMG is ~40 MB; full
  downloads are the right trade-off.
- **Dual-key signing** (manifest-key + artifact-key) — would
  shrink the blast radius of a single key leak. Spec's Open
  Questions section parks this for v2.
- **Beta / nightly channels** — Settings UI placeholder lives in
  Phase 4; full multi-channel support waits until Stable has
  been shipping cleanly for ≥4 weeks (per spec's "Out of scope").
- **GitHub Pages-hosted manifest** — fallback if
  `releases/latest/download/` aliases turn out to be unreliable
  for assets. Phase 5's smoke test surfaces the failure if so.
