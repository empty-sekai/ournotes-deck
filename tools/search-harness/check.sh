#!/usr/bin/env bash
# Local gate: formatting, Clippy, tests, the recommendation WASM build and the synthetic harness suite.
# Run from anywhere inside the repository. WORK (default: work) holds the exported corpus and reports.
set -euo pipefail
cd "$(dirname "$0")/../.."
work=${WORK:-work}
harness_target=${CARGO_TARGET_DIR:-tools/search-harness/target}

cargo fmt --all -- --check
cargo fmt --manifest-path tools/search-harness/Cargo.toml -- --check
cargo fmt --manifest-path wasm/recommend/Cargo.toml -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --all-targets --features search-diagnostics -- -D warnings
cargo clippy --locked --all-targets --features native-fixtures -- -D warnings
cargo clippy --locked --all-targets --manifest-path tools/search-harness/Cargo.toml -- -D warnings
cargo test --release --locked --features search-diagnostics
cargo test --release --locked --manifest-path tools/search-harness/Cargo.toml
python3 -m unittest discover -s tools/search-harness -p 'test_*.py'
python3 -m unittest discover -s tools/native-validation -p 'test_*.py'
cargo build --release --locked --target wasm32-unknown-unknown --manifest-path wasm/recommend/Cargo.toml
cargo build --release --locked --manifest-path tools/search-harness/Cargo.toml
BDON_HARNESS_OUT="$work/corpus" cargo test --release --locked --test adapter_fixture_export \
  export_search_harness_inputs -- --ignored
python3 tools/search-harness/check-errors.py "$harness_target/release/ournotes-search-harness" "$work/corpus" \
  "$work/oracle-cap-refusal"
python3 tools/search-harness/run.py run --binary "$harness_target/release/ournotes-search-harness" \
  --suite "$work/corpus/suite.json" --out "$work/reports"
