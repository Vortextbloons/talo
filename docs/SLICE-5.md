# Slice 5 — Dedicated browser

The native supervisor owns a separate Node Browser Manager with its own Windows Job Object. Pi's stdin/stdout transport only requests operations; it never launches Chromium. Closing the interface leaves the native tray host and browser running. Pi hibernation, restart and generation cancellation do not terminate the browser service. Explicit Stop closes Chromium without deleting its profile. Quit Talo shuts down services, including Chromium, and the Job Object removes descendants if graceful shutdown fails or the host crashes.

Playwright is the sole Chromium launcher. The existing packaging pipeline bundles the Chromium revision pinned by Playwright 1.64.1 and records it in `vendor/runtime-manifest.json`. Stagehand 4.3.0 attaches to that browser; it never launches another one or uses Browserbase. Chromium's sandbox is explicitly enabled. Stagehand's extension is shipped inside its npm package. Only that extension's computed origin is allowed on the loopback CDP connection. The manager API is the host-owned stdio pipe, not a public HTTP server. As with other desktop browser automation, processes running as the same Windows user remain inside the local trust boundary.

The persistent primary profile is `browser/profiles/primary` under Talo's application data directory. An existing Slice 1–4 `browser-profile` is migrated once. A process lock prevents two managers from using the same profile. Crash recovery reaps the service's complete process tree before relaunching it. Profile directories are independent of Pi sessions and memory. Session identifiers and descriptors reserve isolated profiles for later work, but V1 accepts only `primary` and does not execute sub-agents.

## Tools and permissions

Pi registers `browser_open`, `browser_tabs`, `browser_snapshot`, `browser_interact`, `browser_ai`, `browser_screenshot`, `browser_download` and `browser_handoff`. DonSeTch's four `web_*` research tools remain separate. Browser activity appears in chat without typed values, extracted page data or credentials in activity previews.

All deterministic interactions, intelligent browser requests and downloads conservatively require one-use approval. The host strips caller-supplied authorization, binds approval to the exact arguments and document revision, audits the decision, and expires it after 90 seconds. A changed document requires fresh approval. Purchases, messages and deletions consequently cannot bypass approval by being disguised as ordinary clicks. Uploads and password entry use manual takeover. Website content is explicitly untrusted; there is no automatic write into permanent memory.

Supported native Stagehand providers use the current connection's key and model. OpenAI-compatible providers, including OpenCode, OpenRouter and configured compatible custom endpoints, use Stagehand's client-side model callback with the same connection. Incompatible custom APIs fail explicitly; there is no cloud fallback. Logging and caching are disabled for Stagehand. Credentials are only passed over private host pipes and kept in process memory; errors exclude provider response bodies and keys. No cookie, storage or arbitrary JavaScript tools are exposed. Accessible snapshots omit form values; screenshots remain private artifacts and should not be requested on credential screens.

All browser HTTP(S) traffic uses a local egress proxy that resolves and validates destination addresses before connecting to the selected IP. Private, loopback and custom-port destinations are denied, including redirects and subresources. Page requests to loopback are additionally blocked by Playwright routes; the Stagehand extension alone needs the loopback CDP exception. Page WebSockets and service workers are blocked in V1; some sites that depend on them may require future compatibility work. QUIC and non-proxied WebRTC UDP are disabled. Agent navigation only accepts public HTTP(S) URLs.

Chromium's automatic downloads are denied. `browser_download` requires approval for the exact public URL, uses matching profile cookies privately, pins the resolved address, rejects redirects, limits files to 25 MiB and writes a unique non-executable `.download` file into `browser/downloads`. Neither websites nor the model choose an arbitrary output path. Files are never automatically opened or executed.

## Ownership and cancellation

The Browser page and chat controls show the active page and owner. Open Browser enters human control. Return Control explicitly hands ownership to Pi. During agent ownership Windows disables hardware input to Chromium windows while CDP remains usable. Take Over cancels active commands before enabling manual input. Agents cannot return control to themselves. Watch focuses the window without changing ownership; Pause retains the lock and blocks automation. Stop closes Chromium.

Operations are serialized in the primary session, have cancellation and deadlines, and are bound to the worker epoch. A cancelled or timed-out in-flight operation closes its active tab so queued Playwright or Stagehand commands cannot continue after takeover. This is deliberately conservative and may discard unsaved work in that tab; cookies and the profile remain. Other tabs are retained. Browser crashes leave an explicit error state and can be recovered with Open Browser.

## Verification

The existing check/build commands cover the UI, TypeScript workers and native host. Browser integration coverage exercises bundled sandboxed Chromium, profile persistence, deterministic interaction, stale approvals, ownership, cancellation, crash recovery and local Stagehand extraction through a fixture provider. The optional native service test uses `TALO_BROWSER_TEST_HOST` pointing to a freshly built Talo executable and checks the real window ownership helper plus Pi sleep/wake independence. It does not use a live paid provider.

Completed during implementation: worker compilation, UI production build, browser integration checks, local Stagehand extraction, bundled runtime verification and the native service sleep/wake check. A real paid-provider call and a signed installer were not exercised.
