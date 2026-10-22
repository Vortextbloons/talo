# Bundled runtimes

Run `npm run vendor` then `npm run prepare:bundle` on Windows x64. Generated binaries are ignored by Git. Node is downloaded from nodejs.org with SHA-256 verification. Chromium is the exact revision in the locked Playwright package. DonSeTch's installer verifies its release checksum. No runtime downloads or npm installation occur on an end user's device.

`runtime/` contains Node and Chromium. `bundle/` contains compiled workers, production node_modules, and license inventory. Tauri maps both into the installed resource directory. `runtime-manifest.json` records runtime versions; do not edit generated files manually.
