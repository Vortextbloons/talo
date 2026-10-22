# Releasing Talo

Everything below is Windows-only today, because the packaging pipeline and the vendored
runtimes are Windows x64.

## One-time setup

1. **Updater keypair.** Already generated on this machine:

   ```
   ~/.tauri/talo.key        private key — never commit, never lose
   ~/.tauri/talo.key.pub    public key — embedded in apps/desktop/tauri.conf.json
   ```

   The public key is `plugins.updater.pubkey` in `apps/desktop/tauri.conf.json`. If it is lost,
   installed copies can never be updated again; they would have to reinstall by hand. Back the
   private key up outside the repo.

   The key was generated without a password. If you want one, regenerate **before the first
   public release** (`npm run tauri signer generate -- -w ~/.tauri/talo.key -f`), then paste the
   new `.pub` into the config — after a release has shipped, changing the key strands users.

2. **Repository secrets** (`Settings → Secrets and variables → Actions`):

   | Secret | Value |
   | --- | --- |
   | `TAURI_SIGNING_PRIVATE_KEY` | full contents of `~/.tauri/talo.key` (not a path) |
   | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | the key's password, or empty |

3. **Updater endpoint.** `apps/desktop/tauri.conf.json` ships with
   `https://github.com/OWNER/talo/releases/latest/download/latest.json`. Release builds rewrite
   it from `GITHUB_REPOSITORY` via `scripts/updater-endpoint.mjs`, so the checked-in placeholder
   is never what users poll. Local builds keep the placeholder and simply fail to find updates.

4. **Workflow permissions.** `Settings → Actions → General → Workflow permissions` must allow
   read and write, or the release upload fails with *Resource not accessible by integration*.

## Cutting a release

```bash
npm run version:set 0.1.1     # package.json, apps/ui, tauri.conf.json, Cargo.toml
git commit -am "0.1.1"
git tag v0.1.1
git push origin main --tags
```

The `release` workflow then:

1. runs `npm ci`, `npm run vendor` (downloads Bun 1.4.3 + the pinned Chromium revision),
2. builds the workers and the shipped npm graph, and boots the bundled worker once as a check,
3. points the updater at the real repository,
4. builds with `tauri.release.conf.json` merged in, which sets `createUpdaterArtifacts`
   (the plain `npm run package` deliberately leaves it off so local builds need no key),
5. signs the installer, publishes the GitHub release and uploads `latest.json`.

Expect the run to take 15–25 minutes and the installer to be ~240 MB: the app ships Bun,
Chromium and the whole npm graph, and Tauri's updater has no delta support — every update
downloads the full installer.

## What users see

Settings → Advanced → Updates shows the installed version. Talo checks quietly on launch and
whenever Settings is opened.

- Update available → **Update now** → the worker, browser and research processes are stopped,
  the signed installer downloads with a progress bar, and Windows runs it.
- On Windows the plugin exits the app and the NSIS installer relaunches it (`passive` mode plus
  `/R`), so the app closes and comes back on the new version. `process.relaunch` is only needed
  on macOS/Linux.
- Chats, settings, connections and memory are stored in the app data directory and are untouched.

`prepare_update` (Rust) exists because the bundled `bun.exe`, Chromium and the research worker
hold files the installer must replace; without it Windows can fail on a locked file.

## Signing the installer (optional, not the updater signature)

The updater signature only proves the artifact came from you. Windows SmartScreen is separate
and will warn on an unsigned installer. A code-signing certificate wired through
`bundle.windows.certificateThumbprint` (or `signCommand`) removes that warning; it is not
required for updates to work.

## Rollback

`latest.json` from `releases/latest` is authoritative. Deleting or unpublishing a bad release
makes the previous one latest again, but clients that already installed the bad build will not
downgrade — the updater only moves forward unless you install a `versionComparator`.
