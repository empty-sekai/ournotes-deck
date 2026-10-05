# Release artifacts

GitHub Releases include the following packages, built from the immutable commit
referenced by the version tag. Each package has a top-level directory, licenses,
usage notes, and `build-info.json` with the source commit, compiler version and
SHA-256 hashes of its contents. `SHA256SUMS` covers all seven archives.

| Package | Contents / requirements |
| --- | --- |
| `ournotes-deck-vVERSION-x86_64-unknown-linux-musl.tar.gz` | Statically linked Linux x64 CLI binaries |
| `ournotes-deck-vVERSION-aarch64-unknown-linux-musl.tar.gz` | Statically linked Linux ARM64 CLI binaries |
| `ournotes-deck-vVERSION-x86_64-pc-windows-msvc.zip` | Windows x64 CLI binaries |
| `ournotes-deck-vVERSION-x86_64-apple-darwin.tar.gz` | macOS Intel CLI binaries; macOS 11 or later |
| `ournotes-deck-vVERSION-aarch64-apple-darwin.tar.gz` | macOS Apple Silicon CLI binaries; macOS 11 or later |
| `ournotes-replay-wasm-vVERSION.tar.gz` | Replay WASM with `web/` and `nodejs/` bindings |
| `ournotes-recommend-wasm-vVERSION.tar.gz` | Recommendation WASM with `web/` and `nodejs/` bindings |

Native packages contain one executable, `ournotes-deck` (`ournotes-deck.exe` on
Windows). Its `recommend` subcommand accepts a JSON request plus a roster or
owned snapshot; `power`, `skip`, `live`, and `chart-stats` provide the task-specific
interfaces. Use `ournotes-deck --help` and `ournotes-deck recommend --help`.
Rust library consumers use the tagged Git dependencies described in the README.

Both WASM packages include JavaScript, TypeScript declarations, and the matching
`_bg.wasm` files. Keep each binding directory together. The web directory uses ES
modules; the Node directory uses CommonJS. Serve the web package over HTTP, with
`application/wasm` for `.wasm` files. Put synchronous recommendation execution in
a dedicated Worker, which can be terminated to cancel a search.

No game data or accounts are bundled. Supply your own matching-version deck
data and the required player inputs.

## Building and publishing

`.github/workflows/release.yml` runs on version tags and can also be dispatched
from the default branch with an existing tag, for example `v0.0.1`. The workflow
resolves the tag to a commit and checks its workspace version. It checks out that
commit for every build, even when the publishing scripts are newer than the tag.
It does not move tags or rebuild release binaries from the default branch.

All native targets build on matching host hardware. Linux uses musl; the macOS
deployment target is 11.0. WASM uses the two independent locked package manifests
and wasm-bindgen CLI 0.2.127, matching their pinned dependency version.

Before uploading, the workflow extracts and runs the native binaries on six
synthetic cases per platform (Power, Skip, Free score/PT and Mission score/PT).
For WASM it compares the extracted Node recommendation package with the native
49-case synthetic account corpus, then exercises both web modules in a Chromium
Worker. The worker checks Power and Free recommendation results against native
references and replay output against the Node binding. These package checks do
not establish additional native-game coverage.

Only after all builds and checks pass does the workflow upload the archives and
`SHA256SUMS`, download the published assets again, and verify every archive hash.
The workflow can add artifacts to an already published source-only Release.
