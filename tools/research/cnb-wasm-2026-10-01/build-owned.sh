set -euo pipefail
TASK=/workspace/cache/member-owned-current4e-20261001
CONTAINER=draft-recommend-rust-j4
trap 'code=$?; if [ "$code" != 0 ]; then printf "{\"phase\":\"owned-check-failed\",\"exitCode\":%s}\n" "$code" > "$TASK/status.json"; fi' EXIT
printf '{"phase":"owned-format-source-freeze"}\n' > "$TASK/status.json"
docker exec -w "$TASK/source" "$CONTAINER" bash -ec 'cargo fmt --all; cargo fmt --manifest-path wasm/recommend/Cargo.toml' </dev/null
python "$TASK/freeze-source.py" before
printf '{"phase":"owned-clippy-core-tests","jobs":4}\n' > "$TASK/status.json"
docker exec -w "$TASK/source" -e CARGO_TARGET_DIR="$TASK/target-j4" "$CONTAINER" bash -ec 'cargo fmt --all -- --check; cargo fmt --manifest-path wasm/recommend/Cargo.toml -- --check; cargo clippy --all-targets --locked -j4 -- -D warnings -A clippy::nonminimal_bool -A clippy::uninlined_format_args; cargo test --locked -j4; BDON_FIXTURE_OUT=/workspace/cache/member-owned-current4e-20261001/corpus cargo test --test adapter_fixture_export --locked -j4 -- --ignored --test-threads=1' </dev/null
printf '{"phase":"owned-release-cli-wasm","jobs":4}\n' > "$TASK/status.json"
docker exec -w "$TASK/source" -e CARGO_TARGET_DIR="$TASK/target-j4" "$CONTAINER" bash -ec 'cargo build --bin ournotes-recommend --release --locked -j4; cargo build --manifest-path wasm/recommend/Cargo.toml --target wasm32-unknown-unknown --release --locked -j4' </dev/null
mkdir -p "$TASK/pkg"
docker exec "$CONTAINER" /workspace/cache/deck-wasm-build/tools/bin/wasm-bindgen "$TASK/target-j4/wasm32-unknown-unknown/release/ournotes_recommend_wasm.wasm" --target web --out-dir "$TASK/pkg" </dev/null
python "$TASK/freeze-source.py" after
printf '{"phase":"owned-cli-corpus"}\n' > "$TASK/status.json"
docker exec draft-recommend-browser python3 "$TASK/make-corpus.py" </dev/null
printf '{"phase":"owned-browser-corpus"}\n' > "$TASK/status.json"
docker exec draft-recommend-browser node "$TASK/browser-accept.cjs" </dev/null
python "$TASK/freeze-source.py" after
printf '{"phase":"owned-acceptance-pass","sourceStable":true}\n' > "$TASK/status.json"