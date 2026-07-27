#!/usr/bin/env node
// Check that the app version is identical across the three hand-maintained
// manifests (src-tauri/tauri.conf.json, src-tauri/Cargo.toml, ui/package.json).
// Optionally pass a git tag (e.g. `v0.1.5`) to also require tag == version —
// release.yml uses this to fail fast before building a broken updater manifest.
//
// Usage:  node scripts/check-version-sync.mjs [vX.Y.Z]
// Exit 0 = in sync, exit 1 = mismatch (message names every source).

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");

const sources = {
  "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
  // First `version = "..."` line — the [package] section sits at the top of Cargo.toml.
  "src-tauri/Cargo.toml": read("src-tauri/Cargo.toml").match(/^version\s*=\s*"([^"]+)"/m)?.[1],
  "ui/package.json": JSON.parse(read("ui/package.json")).version,
};

const tag = process.argv[2];
if (tag) sources["git tag"] = tag.replace(/^v/, "");

const versions = Object.values(sources);
if (versions.some((v) => !v)) {
  console.error("version-sync: could not read a version from every source:");
  for (const [name, v] of Object.entries(sources)) console.error(`  ${name}: ${v ?? "NOT FOUND"}`);
  process.exit(1);
}

if (new Set(versions).size !== 1) {
  console.error("version-sync: MISMATCH — these must all be identical:");
  for (const [name, v] of Object.entries(sources)) console.error(`  ${name}: ${v}`);
  process.exit(1);
}

console.log(`version-sync: OK (${versions[0]})`);
