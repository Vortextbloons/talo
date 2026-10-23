<div align="center">
  <img src="apps/desktop/icons/icon.png" alt="Talo app icon" width="112">
  <h1>Talo</h1>
  <p><strong>An AI agent that stays with your work.</strong></p>
  <p>A Windows-first desktop agent built to be there when you need it, with context that carries across conversations and projects.</p>
</div>

---

Talo is designed to feel less like opening a chatbot and more like working alongside an agent throughout your day. It stays available in the system tray, keeps your projects and conversations organized, and can continue active work while its window is closed.

Give it a task, and Talo can draw on your saved workspace knowledge, research the web, and work through an integrated browser. You can follow along, take over browser control, or return later to pick up where you left off.

Your data stays on your device, and saved provider keys are protected with Windows DPAPI. Connect a supported model provider to get started.

## An agent that can shape its own space

Talo can propose structure instead of only answering: draft a workspace with its own brief and pinned notes, hand you a task it cannot do itself, or ask a focused question with real options when a decision changes its next step. Those arrive in the **Inbox** (and the question card appears above the composer for the chat that asked it).

Proactivity stays bounded. Talo acts inside a conversation, never on a timer. A drafted workspace stays a draft until you accept it, Talo can only edit workspaces you explicitly let it edit, it may create five things a day, and it never deletes. Each workspace can also narrow what Talo may do there — web research, browser, or files — and carries a brief that applies to every chat in that space.

See [Slice 9 architecture and verification](docs/SLICE-9.md).

## Telegram

Your permanent primary chat stays above project chats. To reach it from Telegram,
choose a model in that chat, then open **Settings → Integrations → Telegram**.
Create a dedicated bot with BotFather, paste its token into Talo, open the pairing
link, and confirm your account in Talo. One private Telegram account can pair at a time.

Keep this PC awake and online with Talo running in the tray. Telegram supports text
requests, task progress, questions, and one-time approvals. `/status` shows progress;
`/stop` stops the current primary-chat reply. Other chats and child tasks keep running.
Messages and approval details pass through Telegram; the bot token stays protected
on your PC. Desktop-originated requests are not automatically forwarded.

## Google Workspace

Talo can read Gmail, work with Google Calendar and read or create text files in Google
Drive from a chat. For the person using Talo it is one button: **Settings → Integrations →
Google Workspace → Connect Google**, then choose the account and allow the permissions in
the browser that opens. Talo listens once on `127.0.0.1` for Google's answer, so no redirect
URI has to be registered.

The host owns the sign-in: it uses PKCE with a loopback redirect, seals tokens with Windows
DPAPI, refreshes them itself and revokes them when you disconnect. Tokens never reach the
UI, a chat, a log, or the model provider. Reading runs when you ask. Sending email, creating
or changing calendar events, and writing Drive files each stop and show the complete
approved action, recipient and body in a scrollable card that waits for a one-time approval;
a scheduled run cannot approve them on its own. Google content a chat uses is sent to your
configured model provider like any other tool result. A cancelled or interrupted write is
reported as uncertain rather than repeated, and Talo never retries a Google mutation by
itself.

Talo requests `gmail.readonly`, `gmail.compose`, `calendar.events`, `drive.readonly` and
`drive.file` separately, and reports any permission Google withholds. Drive writes only
touch files Talo created.

The Google Cloud app behind the shipped client is published as **External / In production**,
but it is **not Google-verified** and this repository does not claim otherwise. Until that
review finishes, the consent screen shows an unverified-app warning, Google caps use at 100
users, and `gmail.readonly` and `drive.readonly` are restricted scopes, so verification is
still required before an unrestricted public release. Verification needs the branding the
project registered: homepage <https://vortextbloons.github.io/talo/> and privacy policy
<https://vortextbloons.github.io/talo/privacy.html> (the same policy as
[PRIVACY.md](PRIVACY.md)). Two steps are still open: proving ownership of
`vortextbloons.github.io` in Google Search Console, and recording the scope-usage demo
video. Live behaviour is not exercised by this repository's tests: a person has to complete
the browser sign-in once to confirm it end to end.

### Building Talo with the Google app

The Google OAuth client belongs to the Talo build, not to the person using it. A build
without one still runs: the Google card then says it is unavailable instead of offering
sign-in.

1. In the Google Cloud project, enable the **Gmail**, **Google Calendar** and **Google
   Drive** APIs.
2. On the OAuth consent screen, set the app name, homepage and privacy links, choose
   **External**, and add the scopes `gmail.readonly`, `gmail.compose`, `calendar.events`,
   `drive.readonly` and `drive.file`. Testing status limits sign-in to listed test users;
   production works for any account but shows the unverified-app warning until Google
   verifies the app. The branding registered for this app uses
   <https://vortextbloons.github.io/talo/> and
   <https://vortextbloons.github.io/talo/privacy.html>.
3. Create an OAuth client of type **Desktop app** and download its JSON. Talo reads
   Google's standard installed-app file (`{"installed":{"client_id":…,"client_secret":…}}`)
   exactly as downloaded; the client secret is not treated as confidential by Google for
   desktop clients, but Talo still sends it to the token endpoint and keeps user tokens
   sealed.
4. Put that file where the build can embed it, then build as usual:
   * Keep it at `apps/desktop/google-oauth.local.json` (ignored by git), or
   * point `TALO_GOOGLE_OAUTH_CLIENT` at the downloaded file, or
   * pass its JSON text in `TALO_GOOGLE_OAUTH_JSON`.
   `apps/desktop/build.rs` embeds the client at compile time and rebuilds when any of these
   changes, so a released app needs no environment variable, file or setup form. Never
   commit the downloaded JSON. Adding the local file to a checkout that was already built
   needs one `cargo clean -p talo --manifest-path apps/desktop/Cargo.toml`; the environment
   variables always trigger a rebuild on their own.

## Learn more

[Providers](docs/PROVIDERS.md) · [Google Workspace](docs/GOOGLE.md) · [Architecture](docs/ARCHITECTURE.md) · [Troubleshooting](docs/TROUBLESHOOTING.md) · [Privacy](PRIVACY.md) · [Third-party notices](docs/THIRD-PARTY-NOTICES.md)
