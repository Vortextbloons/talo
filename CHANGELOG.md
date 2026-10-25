# Changelog

All notable changes to Talo are recorded here. Versions follow semantic versioning; the
installer, the update manifest and this file are bumped together by `bun run version:set`.

## 0.4.2 ? 2026-10-24

- Fixed release packaging by synchronizing Cargo.lock with the app version.
- Version bumps now update Cargo.lock automatically to keep locked release builds working.
- Includes all changes documented in 0.4.1 below.

## 0.4.1 — 2026-10-24

Patch release fixing approvals, delegation, scheduler, MCP cancellation, and Google
sign-in packaging.

### Approvals and integrations

- Protected-tool "allow for this scope" grants are now saved before the approval is
  reserved, so scope grants persist correctly.
- Worker startup now stops the worker and records the failure when MCP tool
  configuration fails, instead of leaving it half-started.

### Delegation and scheduler

- Opening a delegated child transcript is guarded against stale requests and
  conversation switches.
- Updating a deleted scheduled task now reports that it no longer exists instead of
  silently succeeding.

### Tools and providers

- MCP connection setup respects cancellation instead of leaking close attempts.
- Provider preparation is shared between validation and theme drafting.
- Tool output truncation is byte-safe for multibyte text.
- Removed dead contract types (`MemoryRecord`, `MemoryStore`, `ScheduledTask`).

### Release bundle

- The release workflow validates the `TALO_GOOGLE_OAUTH_JSON` secret and embeds the
  Google OAuth client in the installer, so Google sign-in works in released builds.

### Upgrading

Install over an existing build. Chats, memory, projects, skills, schedules, and integrations are
kept; the SQLite schema migrates in place. Installed copies update themselves through
Settings → Advanced → Updates.

## 0.4.0 — 2026-10-23

This release improves tray behavior, approval routing, theming, and the first-run
experience.

### Window and tray

- Closing to tray now suspends and resumes the WebView2 webview, with a
  `keepWebviewInTray` setting and updated window close handling.
- The native window background color follows the active theme.

### Approvals and onboarding

- Browser and tool approvals now carry conversation and generation IDs, so prompts
  route to the conversation that triggered them.
- Reworked onboarding and permission dialogs.
- Structured questions use a single-question wizard with progress pips.

### Theming and branding

- New theme store with a companion color picker; the AgentMark component replaces
  the static mascot SVG, and agent avatars plus busy glow effects use the
  companion color.
- The assistant can draft a theme from a description through the new `theme.draft`
  worker method.
- Chat session system prompts now include the current date and current model.

### Delegation

- Delegated tasks carry an `agent_name`, persisted in the host database and shown
  in the child transcript and delegation views.

### Memory and navigation

- Added a Memory page to primary navigation.

### Tool errors and release bundle

- MCP schema validation errors now surface field details.
- Bundle assets pruned; DonSeTch source ships as a release asset.

### Upgrading

Install over an existing build. Chats, memory, projects, skills, schedules, and integrations are
kept; the SQLite schema migrates in place. Installed copies update themselves through
Settings → Advanced → Updates.

## 0.3.0 — 2026-10-23

This release adds Google Workspace integrations and improves startup and long-running chat
responsiveness.

### Google Workspace

- Connect a Google account from Settings to read Gmail, work with Calendar, and read or create
  text files in Drive.
- Sending email, changing calendar events, and writing Drive files require a one-time approval
  that shows the action details before it runs.
- Google sign-in and token handling stay in the Windows host, with tokens protected using DPAPI.
- The Google app is not yet Google-verified; its consent screen may show an unverified-app
  warning and Google currently limits use to 100 users.

### Chat and scheduled tasks

- Startup now loads a snapshot of the primary conversation, library, and integrations, reducing
  the round trips needed before the first chat view appears.
- Chat session operations release the host lock between worker requests, and large transcripts
  load in pages.
- The assistant can manage scheduled tasks through its managed task operation.

### Build and privacy

- The build and Windows release workflow now use Bun, with the dependency lockfile updated to
  `bun.lock`.
- Added a published privacy policy and included the notices required by the release bundle.

### Upgrading

Install over an existing build. Chats, memory, projects, skills, schedules, and integrations are
kept; the SQLite schema migrates in place. Installed copies update themselves through
Settings → Advanced → Updates.

## 0.2.0 — 2026-10-22

Second Windows release. The release adds delegated child tasks, saved skills, a second model
provider family with sign-in, Telegram access to a permanent primary chat, and a faster chat
surface.

### Delegated child tasks

- Talo can split a request into child tasks. Each one gets a self-contained brief and a short
  title, runs independently of the parent chat, and returns its result to that chat
  automatically.
- Child transcripts open in the same window, read-only, with a bounded list of the plain steps
  the child took, so progress can be inspected without reading tool arguments.
- A subagent call now appears inside the parent message that started it, as a compact row that
  tracks `Starting → Running → Done` (or failed, interrupted, stopped) and opens the child
  transcript. Finished calls stay in the chat as a record.
- Active children also appear in a compact list in the right-hand panel with a one-click open
  and a cancel button; the list clears as work finishes so nothing stale accumulates.
- Cancellation is scoped: stopping one child no longer risks tearing down the shared worker,
  and a scheduled run's cancellation stays isolated from interactive work.
- Child sessions cannot gain broader capabilities than the chat that created them, and a
  delegated child cannot propose skills — the parent does, so the Inbox shows one card.

### Skills

- A finished repeating workflow can be saved as a skill. Talo proposes it with a title,
  description and Markdown instructions; the proposal waits in the Inbox for approval, editing
  or dismissal, and is never active before that.
- Approved skills are stored as portable `SKILL.md` files, scoped to one project or to every
  project, revisioned, and listed with their name and description so a chat can read one before
  it starts.
- Proposals can also revise an approved skill; the approved text stays active until the
  revision is accepted. Disabling and deleting happen only from the Skills page.

### Providers

- Added **ChatGPT** as a provider through OpenAI sign-in, with token refresh and revoke from
  Settings. Keys and tokens continue to be protected by Windows DPAPI and never enter the UI
  state or chat transcripts.
- Provider and model selection is remembered per conversation, and the model picker shows
  per-provider routing tiers when a chat is set to choose the model automatically.

### Telegram

- Pair one private Telegram account with a dedicated bot, then talk to Talo from a phone.
- Requests arrive in the permanent **primary chat**: text prompts, progress, structured
  questions and one-time approvals. `/status` reports progress and `/stop` stops the current
  primary-chat reply only.
- Browser actions requested through Telegram stay protected by the same approval broker and
  expire when stale. Desktop-originated requests are not forwarded to Telegram.

### Primary chat

- Every install has a primary chat above project chats. It cannot be renamed or deleted, so
  Telegram and future remote surfaces always have a stable place to deliver work.

### Chat and history

- Concurrent turns now queue behind the running one with their own pending state instead of
  failing the whole send, and the queue is visible and cancellable.
- Message history loads in pages with before/after cursors, and long transcripts are
  virtualized, so a conversation with hundreds of messages no longer floods the DOM.
- Streamed deltas are journaled, so a restart replays completed work instead of losing it.
- Image attachments open in a native preview, served through the host rather than embedded in
  chat state.
- The thread rail no longer collapses into a strip of initials; the conversation list is a
  fixed column, and the right-hand companion is smaller with less chrome.

### Fixes

- The tools router treated an unknown or missing operation as a call with no server, which
  surfaced as "enable and trust this integration in Settings" — a message that looked like a
  permissions problem while nothing had been invoked. Unknown operations now report the valid
  names, and a missing server says which list to check.
- Read-only agent reads (`state`, `space_list`, `task_list`, `skill_list`) no longer emit
  `agent-changed`, so opening the Inbox stops re-rendering unrelated panels.
- Browser frame fields are owned rather than cloned, and the parsed message tree uses
  `$state.raw`, cutting per-token reactivity work.
- The companion's browser preview dropped its resting-status and per-chat tabs caption, and the
  model/scope footer under it, keeping the panel to the mascot, the activity line and work in
  progress.

### Upgrading

Install over an existing build. Chats, memory, projects, skills, schedules and Telegram pairing
are kept; the SQLite schema migrates in place. Installed copies update themselves through
Settings → Advanced → Updates.

### Known limitations

- Installers and update artifacts are unsigned, so Windows SmartScreen may warn.
- Telegram needs this PC awake and online with Talo running in the tray.
- Delegated child tasks run inside the same app with the same provider connection; they are not
  a separate machine or a cheaper model tier unless you choose one.

## 0.1.0 — 2026-10-22

First Windows release candidate for nontechnical users, with everything running locally.

- Chat with streaming replies, stop and retry, image attachments, reasoning display,
  per-conversation model choice, and threads that survive restarts.
- Providers: OpenCode Zen, OpenCode Go, OpenRouter, NVIDIA NIM and custom OpenAI-compatible
  endpoints through the bundled Pi worker, with DPAPI-protected keys.
- Web research with permission-gated search, fetch, crawl and screenshots, plus expandable
  evidence and source links.
- A persistent Chromium inside Talo driven by the agent, with manual takeover, crash recovery
  and per-chat tabs.
- MCP tools behind a permission broker with exact-operation grants.
- Scheduled tasks with one-time, daily and weekly runs, runtime/cost/token/RAM limits, native
  notifications, and pause or resume from the tray.
- Memory with project and global scopes, explicit consent for personal information, versioned
  edits and rebuildable search.
- Workspace bundles the agent may draft, tasks for the user, and structured questions, all
  reviewed in the Inbox.
- A companion panel whose mascot reflects real activity.
