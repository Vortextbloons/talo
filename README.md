# Talo

A Windows-first, local desktop AI assistant foundation. Tauri 2 owns the tray, persistence, permissions, and service lifecycle. Svelte 5 provides the interface. The unmodified, published Pi SDK runs in a bundled Bun process.

Slice 4 adds built-in, API-key-free public web research through pinned DonSeTch 4.10.1: search, fetch, crawl and screenshots. The Windows supervisor starts research on demand, keeps active work alive, and removes the worker and renderers after two idle minutes. Chat includes expandable evidence, source links, cancellation and retries. See [Slice 4 architecture and verification](docs/SLICE-4.md) and [redistribution requirements](docs/DONSETCH-REDISTRIBUTION.md).

Slice 5 adds a persistent Chromium browser inside Talo, eight Pi browser tools, local Stagehand v4 intelligence, automatic per-thread actions, exclusive input ownership and crash recovery. Use the Browser chip in the composer to open a side preview, then click the preview for a larger view. Opening the view watches Pi; Take Over explicitly enables manual input. Turn activity is bundled into one collapsed group. Open **Browser** to watch, pause, take over, return control or stop. Closing the Talo interface and Pi hibernation leave Chromium running; **Quit Talo** stops all services. See [Slice 5 architecture and verification](docs/SLICE-5.md).

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

Closing the window destroys its WebView2 tree and leaves Talo in the tray; Open Talo rebuilds the window and Quit Talo shuts down services and exits. Active generation continues while the window is closed. Pi rests after approximately two minutes without work and restarts on demand. API keys are protected with Windows DPAPI before storage; they are never saved in settings or Pi authentication files. Chromium retains a dedicated primary profile separate from Pi transcripts and application memory.

See [architecture](docs/ARCHITECTURE.md), [model providers](docs/PROVIDERS.md), [feature boundaries](docs/PLACEHOLDERS.md), [runtime setup](docs/SETUP.md), [design](docs/DESIGN.md), and [third-party notices](docs/THIRD-PARTY-NOTICES.md).

Slice 8 prepares the release candidate: setup is skippable, `Ctrl+1`–`Ctrl+6` switch pages, and `Ctrl+N` starts a conversation. Licensing now covers every bundled runtime (Bun's MIT license ships beside its binary), and a release-gate test keeps installer inputs, notice coverage and wired scripts honest. Release documentation lives in [Slice 8](docs/SLICE-8.md), [packaging and signing](docs/RELEASING.md), [release notes](docs/RELEASE-NOTES.md) and [troubleshooting](docs/TROUBLESHOOTING.md); measure idle footprint with `scripts/measure-idle.ps1`.

The chat companion panel keeps the animated agent, current activity and browser on the right. Click the agent name to rename it; the saved identity is applied to the system prompt on the next turn, including resumed conversations and retries, independently of memory settings. The visible chat gets an automatic live browser thumbnail. Each chat keeps separate tabs and control state, while website logins are shared. Browser actions run without approval prompts; manual interaction requires Take Over. Agent status appears beside the animated logo instead of a separate Pi badge.
