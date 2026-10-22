# Third-party notices

Talo's own distribution license has not been selected. Third-party code retains its original license. No upstream code is forked here. The generated runtime bundle includes complete discovered npm license/notice files in `THIRD-PARTY-LICENSES.json`, plus Bun's LICENSE beside its bundled binary and Chromium's bundled license resources.

| Dependency | Pinned version | License / source |
| --- | --- | --- |
| Tauri | 2.12.5 | MIT OR Apache-2.0; https://github.com/tauri-apps/tauri |
| Svelte | 5.57.2 | MIT; https://github.com/sveltejs/svelte |
| marked | 18.0.11 | MIT; https://github.com/markedjs/marked |
| Pi coding agent / AI SDK | 1.4.0 | MIT; https://github.com/earendil-works/pi |
| Pi MCP Adapter | 5.4.0 | MIT; https://github.com/nicobailon/pi-mcp-adapter |
| DonSeTch | 4.10.1 | AGPL-3.0-only; https://github.com/dondai44423/donsetch/tree/v4.10.1 |
| Stagehand | 4.3.0 | MIT; https://github.com/browserbase/stagehand |
| Playwright | 1.64.1 | Apache-2.0; https://github.com/microsoft/playwright |
| Chromium | Revision 1248 (156.0.8078.4) | BSD-style and third-party licenses bundled with browser; https://chromium.googlesource.com/chromium/src |
| Bun | 1.4.3 | MIT and bundled LGPL-2 notices for WebKit/JavaScriptCore; `LICENSE.md` ships beside the bundled binary; https://github.com/oven-sh/bun |
| rusqlite | 0.40.2 | MIT; https://github.com/rusqlite/rusqlite |
| SQLite | Bundled via libsqlite3-sys in Cargo.lock | Public domain; https://sqlite.org/copyright.html |

DonSeTch's AGPL terms must be preserved when redistributing its binary. Its exact upstream source tag is recorded above; the bundle preparation script includes the corresponding source archive and license. This notice does not substitute for the applicable licenses. See Cargo.lock and the generated npm inventory for transitive dependencies.

Slice 4 also packages the SHA-256-verified MiniLM reranking model/tokenizer (Apache-2.0), ONNX Runtime 1.24.2 (MIT and third-party notices), and PDFium chromium/7802 (BSD-style and third-party notices). Their notice files are included in `research-notices/`. Review [DonSeTch redistribution requirements](DONSETCH-REDISTRIBUTION.md) before publishing an installer.

