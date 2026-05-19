#!/usr/bin/env node
// scripts/make-manifest.mjs
//
// Build the `latest-mac.json` Tauri updater manifest from a notarized
// + signed universal DMG produced by the release workflow.
//
// Usage:
//   node scripts/make-manifest.mjs \
//     --dmg <path/to/Woom_1.4.2_universal.dmg> \
//     --signature <path/to/Woom_1.4.2_universal.dmg.sig> \
//     --version 1.4.2 \
//     --tag v1.4.2 \
//     --repo walpakhart/Woom \
//     --changelog CHANGELOG.md \
//     --out dist/latest-mac.json
//
// Optional flag `--dry-run` skips writing + reads no signature; useful
// for the PR-time verify workflow which doesn't have a real signature
// yet. Validates schema either way.
//
// Output schema (matches Tauri 2's updater + our defence-in-depth
// `sha256` extension — see docs/RELEASES.md):
//
//   {
//     "version": "1.4.2",
//     "notes":   "<markdown extracted from CHANGELOG.md>",
//     "pub_date": "<RFC3339>",
//     "platforms": {
//       "darwin-aarch64": { "signature": "...", "url": "...", "sha256": "..." },
//       "darwin-x86_64":  { "signature": "...", "url": "...", "sha256": "..." }
//     }
//   }
//
// We ship a universal binary; both target keys point at the SAME DMG
// URL with the SAME signature + sha256. Tauri's plugin requires both
// entries to be present to match per-arch installs.

import { readFileSync, writeFileSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { argv, exit } from 'node:process';

function parseArgs(args) {
  const out = { dryRun: false };
  for (let i = 2; i < args.length; i++) {
    const a = args[i];
    if (a === '--dry-run') { out.dryRun = true; continue; }
    if (a.startsWith('--')) {
      const key = a.slice(2);
      const v = args[i + 1];
      if (v === undefined || v.startsWith('--')) {
        die(`flag --${key} requires a value`);
      }
      out[camel(key)] = v;
      i += 1;
    }
  }
  return out;
}

function camel(k) {
  return k.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
}

function die(msg) {
  console.error(`make-manifest: ${msg}`);
  exit(1);
}

function readChangelogSection(path, version) {
  let raw;
  try {
    raw = readFileSync(path, 'utf8');
  } catch {
    // No CHANGELOG yet — non-fatal; ship empty notes.
    return '';
  }
  // Extract the section under `## <version>` (or `## v<version>`).
  // Stops at the next `## ` heading or EOF. Case-insensitive on the v.
  const re = new RegExp(`^##\\s+v?${version.replace(/\./g, '\\.')}\\b[\\s\\S]*?(?=^##\\s+v?[\\d.]+\\b|\\Z)`, 'mi');
  const m = re.exec(raw);
  if (!m) return '';
  // Trim the header line itself — Tauri shows the version separately;
  // we only want the body bullets / prose.
  return m[0].replace(/^##\s+v?[\d.]+.*\r?\n/, '').trim();
}

function sha256(path) {
  const h = createHash('sha256');
  h.update(readFileSync(path));
  return h.digest('hex');
}

function main() {
  const a = parseArgs(argv);
  if (!a.dmg) die('--dmg required');
  if (!a.version) die('--version required');
  if (!a.repo) die('--repo required (e.g. walpakhart/Woom)');
  const tag = a.tag ?? `v${a.version}`;

  // Validate the DMG exists; in dry-run we tolerate a missing file
  // (lets PR-time verify run against a synthetic version bump).
  let dmgSize = 0;
  let dmgSha = '';
  try {
    dmgSize = statSync(a.dmg).size;
    dmgSha = sha256(a.dmg);
  } catch (e) {
    if (!a.dryRun) die(`cannot read DMG at ${a.dmg}: ${e.message}`);
  }

  // Signature is whatever `tauri signer sign` wrote next to the DMG;
  // dry-run replaces it with a placeholder so the schema still
  // validates downstream.
  let signature = '';
  if (a.signature) {
    try {
      signature = readFileSync(a.signature, 'utf8').trim();
    } catch (e) {
      if (!a.dryRun) die(`cannot read signature at ${a.signature}: ${e.message}`);
    }
  }
  if (!signature && a.dryRun) signature = 'DRY_RUN_PLACEHOLDER_SIGNATURE';
  if (!signature) die('signature missing (pass --signature or use --dry-run)');

  const dmgFilename = a.dmg.split('/').pop();
  const url = `https://github.com/${a.repo}/releases/download/${tag}/${dmgFilename}`;
  const notes = a.changelog ? readChangelogSection(a.changelog, a.version) : '';
  const pubDate = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');

  const platformEntry = { signature, url, sha256: dmgSha };
  const manifest = {
    version: a.version,
    notes,
    pub_date: pubDate,
    platforms: {
      'darwin-aarch64': platformEntry,
      'darwin-x86_64': platformEntry,
    },
  };

  // Inline schema check — explicit so missing fields fail loud.
  validate(manifest, a.dryRun);

  if (a.dryRun) {
    console.log('make-manifest: dry-run OK');
    console.log(JSON.stringify({ ...manifest, _dmgSize: dmgSize }, null, 2));
    return;
  }
  if (!a.out) die('--out required (unless --dry-run)');
  writeFileSync(a.out, JSON.stringify(manifest, null, 2) + '\n');
  console.log(`make-manifest: wrote ${a.out} (${dmgSize} bytes DMG, sha256=${dmgSha.slice(0, 16)}…)`);
}

function validate(m, dry) {
  const must = (cond, msg) => { if (!cond) die(`schema: ${msg}`); };
  must(typeof m.version === 'string' && /^\d+\.\d+\.\d+/.test(m.version), 'version must be semver');
  must(typeof m.notes === 'string', 'notes must be a string');
  must(typeof m.pub_date === 'string' && /^\d{4}-\d{2}-\d{2}T/.test(m.pub_date), 'pub_date must be RFC3339');
  must(m.platforms && typeof m.platforms === 'object', 'platforms required');
  for (const arch of ['darwin-aarch64', 'darwin-x86_64']) {
    const p = m.platforms[arch];
    must(p && typeof p === 'object', `platforms.${arch} required`);
    must(typeof p.signature === 'string' && p.signature.length > 0, `platforms.${arch}.signature required`);
    must(typeof p.url === 'string' && p.url.startsWith('https://'), `platforms.${arch}.url must be HTTPS`);
    if (!dry) {
      must(typeof p.sha256 === 'string' && /^[0-9a-f]{64}$/.test(p.sha256), `platforms.${arch}.sha256 must be hex(64)`);
    }
  }
}

main();
