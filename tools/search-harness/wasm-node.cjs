// Compare complete native owned-snapshot answers with the same Rust core compiled to WebAssembly for Node
// (`wasm-bindgen --target nodejs`), and check the progress callback. Keeps JSON number tokens intact.
// Usage: node wasm-node.cjs MANIFEST.json WASM_PACKAGE OUTPUT_DIRECTORY
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const { projection } = require('./json-tokens.cjs');
const sha = value => crypto.createHash('sha256').update(value).digest('hex');
// Search work is stable across progress callbacks. Millisecond fields and process-wide linear-memory
// high-water marks can change: serializing a report may grow WASM memory, which remains allocated.
const untimed = value => Array.isArray(value) ? value.map(untimed)
  : value && typeof value === 'object'
    ? Object.fromEntries(Object.entries(value).filter(([key]) => !key.endsWith('Ms')).map(([key, v]) => [key, untimed(v)]))
    : value;
const telemetry = answer => {
  const work = untimed(JSON.parse(answer).result?.telemetry ?? null);
  if (work) delete work.memory;
  return work;
};
const best = result => result.results[0]?.expectedPayoff;

function main() {
  const [manifestFile, pkgDirectory, outputDirectory] = process.argv.slice(2);
  if (!outputDirectory) throw Error('usage: node wasm-node.cjs MANIFEST.json WASM_PACKAGE OUTPUT_DIRECTORY');
  const manifestPath = path.resolve(manifestFile), root = path.dirname(manifestPath);
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  const pkg = path.resolve(pkgDirectory), out = path.resolve(outputDirectory);
  const { DeckSolver } = require(path.join(pkg, 'ournotes_recommend_wasm.js'));
  fs.mkdirSync(out, { recursive: true });
  const interval = manifest.progressIntervalMs ?? 250;
  const solvers = new Map();
  const cases = [];
  for (const entry of manifest.cases) {
    assert.match(entry.name, /^[A-Za-z0-9_-]+$/);
    const input = Object.fromEntries(['data', 'snapshot', 'request'].map(k => [k, fs.readFileSync(path.resolve(root, entry[k]), 'utf8')]));
    const reference = fs.readFileSync(path.resolve(root, entry.reference), 'utf8');
    assert.equal(JSON.parse(reference).result?.completion, 'Complete', `${entry.name}: reference must be complete`);
    if (!solvers.has(entry.data)) solvers.set(entry.data, new DeckSolver(input.data));
    const solver = solvers.get(entry.data);
    assert.equal(solver.datasetId, sha(input.data), `${entry.name}: dataset identity`);
    const begin = performance.now();
    const plain = solver.recommend(input.snapshot, input.request);
    const plainMs = performance.now() - begin;
    assert.deepEqual(projection(plain), projection(reference), `${entry.name}: exact semantic JSON tokens`);
    const reports = [];
    const hooked = solver.recommend(input.snapshot, input.request, text => reports.push(text), interval);
    assert.deepEqual(projection(hooked), projection(plain), `${entry.name}: progress leaves the answer unchanged`);
    assert.deepEqual(telemetry(hooked), telemetry(plain), `${entry.name}: progress leaves the search unchanged`);
    const final = JSON.parse(hooked).result;
    let previous;
    for (const text of reports) {
      const report = JSON.parse(text);
      assert.equal(report.completion, 'TimedOut', `${entry.name}: a report is the result if stopped then`);
      assert.deepEqual(report.resolvedContext, final.resolvedContext, `${entry.name}: report context`);
      if (previous) {
        assert.ok(previous.telemetry.nodes <= report.telemetry.nodes, `${entry.name}: nodes never decrease`);
        assert.ok(BigInt(best(previous)?.numerator ?? -1) <= BigInt(best(report)?.numerator ?? -1), `${entry.name}: best never decreases`);
      }
      previous = report;
    }
    if (previous && best(previous)) {
      assert.ok(BigInt(best(previous).numerator) <= BigInt(best(final).numerator), `${entry.name}: final best`);
    }
    fs.writeFileSync(path.join(out, `${entry.name}.json`), hooked);
    const row = { name: entry.name, complete: true, exactSemanticTokensMatch: true, reports: reports.length, inputSha256: ['data', 'snapshot', 'request'].map(k => sha(input[k])),
      referenceSha256: sha(reference), wasmWallMs: plainMs };
    cases.push(row);
    console.log(JSON.stringify(row));
  }
  fs.writeFileSync(path.join(out, 'report.json'), JSON.stringify({ format: 'ournotes-deck.wasm-node-check/1', passed: true,
    scope: 'Declared inputs in Node WebAssembly; shared-model native/WASM equivalence, not game parity.',
    node: process.version, progressIntervalMs: interval, manifestSha256: sha(fs.readFileSync(manifestPath)),
    runnerSha256: sha(fs.readFileSync(__filename)), wasmSha256: sha(fs.readFileSync(path.join(pkg, 'ournotes_recommend_wasm_bg.wasm'))),
    cases }, null, 2));
}
main();
