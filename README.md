# Talo

A Windows-first, local desktop AI assistant foundation. Tauri 2 owns the tray, persistence, permissions, and service lifecycle. Svelte 5 provides the interface. The unmodified, published Pi SDK runs in a bundled Node process.

## Develop (Windows x64)

Prerequisites for contributors: Node 24.22.1+, npm, stable Rust with the MSVC target, Visual Studio C++ Build Tools and Windows SDK, and WebView2. End users receive runtimes in the installer.

```powershell
npm ci
npm run vendor
npm run prepare:bundle
npm run dev
```

`npm run dev:ui` previews the interface without native functionality. `npm run check`, `npm test`, and `cargo test --manifest-path apps/desktop/Cargo.toml` validate the foundation. `npm run package` produces a Windows x64 NSIS installer in `apps/desktop/target/release/bundle/nsis`. Packaging includes the offline WebView2 installer and can be large. Installers are unsigned in this phase.

Close hides the window. Open it from the tray; Quit Talo shuts down services and exits. Settings exposes Pi and Chromium start/stop controls. Pi exits after five idle minutes; restarting it restores the latest persistent session when opened. Chromium retains a separate profile across restarts.

See [architecture](docs/ARCHITECTURE.md), [feature boundaries](docs/PLACEHOLDERS.md), [runtime setup](docs/SETUP.md), [design](docs/DESIGN.md), and [third-party notices](docs/THIRD-PARTY-NOTICES.md).
