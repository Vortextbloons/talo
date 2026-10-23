# DonSeTch redistribution review — Slice 4

Reviewed against the unmodified DonSeTch v4.10.1 LICENSE (AGPL-3.0-only), especially sections 1, 4–6, 10 and 13. Upstream: https://github.com/dondai44423/donsetch/tree/3169d2d3e8e426ec0c0eca5cac15c05d83dcdc6f . License text: https://www.gnu.org/licenses/agpl-3.0.html .

Talo launches the unmodified Windows executable as a separate stdio MCP process. It does not link DonSeTch into Pi or the Tauri host, load its native Pi extension, or modify upstream source. Process separation documents the architecture; it does not by itself decide whether a distribution is an aggregate or a combined/derived work.

The bundle includes:

- `DONSETCH-LICENSE.txt`, copyright/license notices, exact version, commit and pinned SHA-256 values in `donsetch-manifest.json`.
- `DONSETCH-SOURCE.txt`: exact version, commit and SHA-256 plus download instructions for the Corresponding Source, which is published as a release asset next to the installer rather than embedded in it.
- `research-notices/`: ONNX Runtime 1.24.2 license and third-party notices; PDFium chromium/7802 license set; reranking model card and Apache-2.0 license.
- Unmodified Windows executable, ONNX Runtime and PDFium DLLs from the checksum-pinned official release archive. No automatic self-updates.

Before publishing an installer, the distributor must resolve and record these release requirements:

1. Preserve upstream copyright, AGPL license, warranty notices and recipients' rights to redistribute and modify. Do not apply incompatible restrictions to covered code.
2. Provide equivalent, free access to the complete Corresponding Source alongside the installer under section 6(d), with clear download instructions. The release workflow uploads the checksum-pinned `donsetch-v4.10.1-source.tar.gz` as a release asset next to the installer and fails the run if the upload fails; `DONSETCH-SOURCE.txt` in the installed bundle records the exact URL, commit and SHA-256. A GitHub tag link alone is insufficient. Verify that the offered source includes any required non-system linked dependency source and the scripts/configuration needed to build it; the upstream repository archive alone does not establish completeness of those dependencies.
3. Review whether the final distribution is an aggregate or a combined/derived work and select Talo's own distribution license accordingly. Talo's own license has not yet been selected. Do not describe this integration as an exemption from AGPL.
4. If DonSeTch is modified, mark changes and dates, provide the modified Corresponding Source and preserve applicable interactive notices. If users interact remotely with a modified version over a network, section 13 requires a prominent source-access offer. Talo currently uses only a local stdio connection to unmodified DonSeTch.
5. Assess section 6 Installation Information if distributing with a covered User Product, and keep the source offer/download available for the applicable distribution method.
6. Retain and review the DLL/model dependency notices and licenses as well as the npm/Rust inventories. A successful build or test is not legal approval to publish.

Rebuild reference: follow the pinned source's README and `.github/workflows/release.yml`; the Windows release uses `cargo build --release --locked --features ocr,rerank,http --target x86_64-pc-windows-msvc`. build.rs records verified PDFium/ONNX dependencies. Talo disables OCR, packages the verified reranking files locally, and keeps renderer downloads disabled.

No installer has been published as part of Slice 4. The release review above remains a distributor responsibility.
