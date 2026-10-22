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

`npm run dev:ui` previews the interface without native functionality. `npm run check`, `npm test`, and `cargo test --manifest-path apps/desktop/Cargo.toml` validate the foundation. `npm run package` produces a Windows x64 NSIS installer in `apps/desktop/target/release/bundle/nsis`. The installer embeds a WebView2 bootstrapper rather than the offline runtime, so it stays near 250 MB; `npm run package:offline` swaps in the ~200 MB offline WebView2 installer for machines that cannot reach Microsoft at install time. Installers are unsigned in this phase.

Connect a provider in Settings, select a model in Chat, and send a message. Responses stream through the bundled Pi worker; Stop preserves partial output and Retry repeats a failed or stopped turn. Each conversation resumes its own Pi session after reopening Talo. Connection checks send a small authenticated model request, which may consume provider credits.

Use Workspaces to create, rename, link a project folder, archive, or delete a workspace. General is the default. The chat sidebar switches workspaces, searches titles and message history, and manages chats. Archived chats can be restored. Deleting chat listings retains their original Pi JSONL files on disk.

Use Memory to save, search, edit, and delete global or workspace knowledge. Personal information requires explicit consent. Edits retain superseded versions; deletion erases older versions too. The memory switch controls context retrieval. Pi keeps its native automatic compaction; bounded memory and relevant archive excerpts are refreshed for each turn without loading whole old conversations. SQLite migrations and the database writer belong to the native host, independent of Pi's lifetime. Rebuild search reconstructs the message cache and FTS index from original session files, preserving cached results when an original is unavailable.

Closing the window destroys its WebView2 tree and leaves Talo in the tray; Open Talo rebuilds the window and Quit Talo shuts down services and exits. Active generation continues while the window is closed. Pi rests after approximately two minutes without work and restarts on demand. API keys are protected with Windows DPAPI before storage; they are never saved in settings or Pi authentication files. Chromium retains the separate profile established in Slice 1; browser automation remains outside Slice 2.

See [architecture](docs/ARCHITECTURE.md), [model providers](docs/PROVIDERS.md), [feature boundaries](docs/PLACEHOLDERS.md), [runtime setup](docs/SETUP.md), [design](docs/DESIGN.md), and [third-party notices](docs/THIRD-PARTY-NOTICES.md).
