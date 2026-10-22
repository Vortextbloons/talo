# Slice 4 — built-in public web research

Pi registers exactly `web_search`, `web_fetch`, `web_crawl`, and `web_screenshot` through `workers/pi/src/web.ts`. The native DonSeTch Pi extension and general MCP loader are disabled; the general adapter rejects DonSeTch registration. Tool construction performs no network or process work.

The adapter emits private `web` MCP frames. The Windows supervisor lazily starts the unmodified, bundled DonSeTch 4.10.1 worker, performs the MCP initialize handshake, and relays correlated responses through `web_response` frames. This connection is asynchronous; host commands and cancellation remain responsive while research runs. Pi and DonSeTch are sibling services, not parent/child runtimes. The public tool names and results remain behind one replaceable adapter.

The worker stays alive during active calls and sleeps after 120 seconds without pending work. Cancellation sends the MCP cancellation notification. A 45-second operation budget, 65-second adapter deadline and 70-second host recovery deadline bound stalled work. A worker crash fails pending calls with a retryable error; the next request creates a fresh worker and handshake. Each worker has a nested Windows Job Object, so its renderers are removed even if their parent crashes. App exit closes all managed processes.

Research uses an allowlisted environment and an isolated cache/profile; provider credentials, personal DonSeTch config, proxy credentials, imported logins and browser cookies are not inherited. The adapter rejects non-HTTP(S), credential-bearing, custom-port, localhost and private-IP URLs, including IPv4 alternate notation and mapped IPv6. DonSeTch's own egress guards validate resolved targets, redirects and browser subrequests. Crawls stay on the seed host, respect robots.txt/rate limits, and have five-page, depth-two and 40,000-character budgets. Direct host requests are paced at least one second apart; DonSeTch applies crawl pacing and throttling internally.

Rendering relies on upstream CDP request guards. Upstream documents a residual DNS validation/connect race in Chromium; this is not an OS network sandbox. Preserve that limitation in security reviews of hostile sites. The HTTP transport uses upstream connect-time validation.

Model context receives at most 10,000 characters of page text, six compact search rows, and twelve source URLs. Responses above the inline threshold are stored as JSON artifacts; screenshot PNGs are written to local artifacts rather than sent as base64 to the model. Artifact opening permits UUID `.json`/`.png` files only inside the research artifact directory. Chat details show source cards, expandable evidence, running activity, cancellation and retry controls. Research details are a bounded in-memory UI projection; authoritative Pi tool results remain in the native session log. Web content has no automatic path into permanent memory.

`scripts/donsetch.mjs` pins the archive, extracted executable/DLLs, source and model hashes. Bundle verification rechecks shipped hashes. OCR and external browser downloads are disabled; the reranker is explicitly packaged because upstream has no complete runtime disable switch. Missing packaged model files fail startup instead of requesting downloads.

Verification:

```powershell
npm run prepare:bundle
npm run check
npm test
cargo test --manifest-path apps/desktop/Cargo.toml
cargo test --manifest-path apps/desktop/Cargo.toml bundled_donsetch_live_four_tools_and_redirect_guard -- --ignored --nocapture
```

The explicit live test exercises all four tools and the public-to-loopback redirect guard using the bundled Windows executable and Chromium. Deterministic tests cover private URL variants, transport cancellation/deadlines, evidence bounds/artifacts, actual Pi tool invocation, tool deduplication, active/idle lifecycle, crash recovery, model isolation, and renderer cleanup. Existing provider/session/memory tests remain part of the normal suite. Public search engines can refuse or rate-limit requests; degraded/weak evidence remains labeled rather than fabricated.

See [redistribution review](DONSETCH-REDISTRIBUTION.md) before publishing an installer.
