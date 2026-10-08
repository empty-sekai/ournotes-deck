// Portable inputs and serial native/Chromium Worker checks for the declared completion corpora.
// Input JSON is copied and transported as bytes/text; parsing never supplies replacement request bytes.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { spawn, execFileSync } = require('node:child_process');
const { compareAnswers, sourceIdentity } = require('./benchmark.cjs');

const sha = value => crypto.createHash('sha256').update(value).digest('hex');
const read = file => fs.readFileSync(file, 'utf8');
const json = file => JSON.parse(read(file));
const write = (file, value) => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(`${file}.tmp`, JSON.stringify(value, null, 2) + '\n');
  fs.renameSync(`${file}.tmp`, file);
};
const safeName = name => {
  assert.equal(typeof name, 'string');
  assert.match(name, /^[A-Za-z0-9_-]+$/);
  return name;
};
const requestContract = text => {
  const request = JSON.parse(text);
  assert.equal(request.format, 'ournotes-deck.search-request/1');
  assert.equal(request.execution?.kind, 'live', 'played Live request required');
  assert.equal(request.k, 3, 'the declared K must remain 3');
  assert.equal(request.limits?.timeLimitMs, 60000, 'the declared search budget must remain 60,000 ms');
  assert.equal(request.limits?.maxCandidates, null, 'no candidate limit may be introduced');
  assert.deepEqual(request.constraints, {}, 'the complete declared candidate domain must remain available');
  return request;
};

function plan(mode) {
  assert(['smoke', 'all', 'synthetic', 'full48'].includes(mode), 'unknown CI corpus selection');
  const include = mode === 'smoke' ? [
    { name: 'synthetic-smoke', suite: 'synthetic', cases: 'short-score,short-probability,short-capped,short-life,short-free,short-no-luck',
      expectedCases: 6, requireComplete: 'short-score,short-free,short-no-luck' },
    { name: 'real-smoke', suite: 'full48', cases: 'short-newcomer-score', expectedCases: 1, requireComplete: 'none' },
  ] : [
    ...(mode !== 'full48' ? [{ name: 'synthetic', suite: 'synthetic', cases: 'all', expectedCases: 16, requireComplete: 'all' }] : []),
    ...(mode !== 'synthetic' ? ['newcomer', 'midcore', 'veteran'].map(profile => ({
      name: `full48-${profile}`, suite: 'full48', cases: `profile:${profile}`, expectedCases: 16, requireComplete: 'all',
    })) : []),
  ];
  return { mode, needSynthetic: include.some(row => row.suite === 'synthetic'),
    needReal: include.some(row => row.suite === 'full48'), matrix: { include } };
}

function selectCases(manifest, selection) {
  assert.equal(manifest.format, 'ournotes-deck.search-benchmark/1');
  assert.equal(manifest.requestTimeLimitMs, 60000);
  assert.equal(manifest.timeoutMs, 90000, 'the declared external Worker watchdog must remain 90,000 ms');
  assert(Array.isArray(manifest.cases) && manifest.cases.length > 0, 'empty corpus');
  const names = manifest.cases.map(entry => safeName(entry.name));
  assert.equal(new Set(names).size, names.length, 'duplicate case names');
  if (selection === 'all') return manifest.cases;
  if (selection.startsWith('profile:')) {
    const profile = safeName(selection.slice('profile:'.length));
    const selected = manifest.cases.filter(entry => entry.profile === profile);
    assert(selected.length > 0, `unknown profile ${profile}`);
    return selected;
  }
  const selected = selection.split(',').map(safeName);
  assert.equal(new Set(selected).size, selected.length, 'duplicate selected case names');
  for (const name of selected) assert(names.includes(name), `unknown case ${name}`);
  return manifest.cases.filter(entry => selected.includes(entry.name));
}

function bundle(manifestFile, outputDirectory) {
  const source = path.resolve(manifestFile), root = path.dirname(source), manifest = json(source);
  const cases = selectCases(manifest, 'all');
  const suite = manifest.synthetic === true ? 'synthetic' : manifest.matrixId;
  assert(['synthetic', 'full48'].includes(suite), 'only the declared synthetic and full48 corpora are accepted');
  assert.equal(cases.length, suite === 'synthetic' ? 16 : 48, 'the full corpus must be exported before selection');
  const output = path.resolve(outputDirectory), directory = path.join(output, 'inputs', suite);
  assert(!fs.existsSync(directory), 'input bundle already exists');
  fs.mkdirSync(directory, { recursive: true });
  const entries = [], receipts = [];
  for (const entry of cases) {
    const copied = { ...entry }, hashes = {};
    for (const field of ['data', 'snapshot', 'request', 'roster']) {
      if (field === 'roster' && !entry[field]) continue;
      assert.equal(typeof entry[field], 'string', `${entry.name}: missing ${field}`);
      const bytes = fs.readFileSync(path.resolve(root, entry[field]));
      const digest = sha(bytes), destination = path.join(output, 'inputs', 'blobs', `${digest}.json`);
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      if (fs.existsSync(destination)) assert.equal(sha(fs.readFileSync(destination)), digest);
      else fs.writeFileSync(destination, bytes);
      copied[field] = `../blobs/${digest}.json`;
      hashes[field] = digest;
    }
    requestContract(read(path.resolve(directory, copied.request)));
    assert.equal(json(path.resolve(directory, copied.snapshot)).datasetId, hashes.data, 'owned snapshot dataset identity');
    entries.push(copied);
    receipts.push({ name: entry.name, sha256: hashes });
  }
  const manifestPath = path.join(directory, 'benchmark.json');
  write(manifestPath, { ...manifest, cases: entries });
  fs.copyFileSync(source, path.join(directory, 'source-manifest.json'));
  write(path.join(directory, 'inputs-receipt.json'), {
    format: 'ournotes-deck.browser-input-bundle/1', suite, sourceManifestSha256: sha(fs.readFileSync(source)),
    manifestSha256: sha(fs.readFileSync(manifestPath)), cases: receipts,
  });
  // Preserve native materialization evidence when the corpus has it.
  for (const name of ['preparation-receipt.json', 'preparation-specs.json', 'preparation.json']) {
    const file = path.join(root, name);
    if (fs.existsSync(file)) fs.copyFileSync(file, path.join(directory, name));
  }
  return manifestPath;
}

function buildReceipt(sourceDirectory, bundleDirectory) {
  const source = path.resolve(sourceDirectory), directory = path.resolve(bundleDirectory);
  const identity = sourceIdentity(source);
  assert.equal(execFileSync('git', ['-C', source, 'status', '--porcelain', '--untracked-files=no'], { encoding: 'utf8' }).trim(), '',
    'CI binaries must be built from an unmodified checkout');
  const artifacts = ['bin/profile_case', 'pkg/ournotes_recommend_wasm.js', 'pkg/ournotes_recommend_wasm_bg.wasm'];
  const receipt = {
    format: 'ournotes-deck.browser-build/1', source: identity,
    artifacts: Object.fromEntries(artifacts.map(file => [file, sha(fs.readFileSync(path.join(directory, file)))])),
    rustc: execFileSync('rustc', ['-Vv'], { encoding: 'utf8' }),
    wasmBindgen: execFileSync('wasm-bindgen', ['--version'], { encoding: 'utf8' }).trim(), node: process.version,
    buildCommands: [
      'cargo build --release --locked --manifest-path tools/search-harness/Cargo.toml --bin benchmark_prepare --bin profile_case',
      'cargo build --release --locked --target wasm32-unknown-unknown --manifest-path wasm/recommend/Cargo.toml',
      'wasm-bindgen --target web --out-dir work/browser-bundle/pkg wasm/recommend/target/wasm32-unknown-unknown/release/ournotes_recommend_wasm.wasm',
    ],
  };
  write(path.join(directory, 'build-receipt.json'), receipt);
  return receipt;
}

function verifyBundle(manifestFile, bundleDirectory, sourceDirectory) {
  const manifestPath = path.resolve(manifestFile), manifest = json(manifestPath), root = path.dirname(manifestPath);
  const inputs = json(path.join(root, 'inputs-receipt.json'));
  assert.equal(inputs.format, 'ournotes-deck.browser-input-bundle/1');
  assert.equal(inputs.manifestSha256, sha(fs.readFileSync(manifestPath)), 'portable manifest identity');
  assert.deepEqual(inputs.cases.map(entry => entry.name), manifest.cases.map(entry => entry.name), 'complete input cover');
  for (const [index, entry] of manifest.cases.entries()) {
    for (const [field, expected] of Object.entries(inputs.cases[index].sha256)) {
      assert.equal(sha(fs.readFileSync(path.resolve(root, entry[field]))), expected, `${entry.name}: ${field} bytes changed`);
    }
    requestContract(read(path.resolve(root, entry.request)));
  }
  const build = json(path.join(bundleDirectory, 'build-receipt.json'));
  assert.equal(build.format, 'ournotes-deck.browser-build/1');
  const current = sourceIdentity(sourceDirectory);
  assert.equal(current.head, build.source.head, 'runner and binaries must have the same source commit');
  assert.equal(current.manifestSha256, build.source.manifestSha256, 'runner and binary source trees differ');
  for (const [file, expected] of Object.entries(build.artifacts)) {
    assert.equal(sha(fs.readFileSync(path.join(bundleDirectory, file))), expected, `${file}: binary identity changed`);
  }
  return { manifest, build, inputs };
}

const provenInBudget = row => row?.completion === 'Complete' && row.optimality === 'proven'
  && Number.isFinite(row.searchWallMs) && row.searchWallMs >= 0 && row.searchWallMs <= 60000 && row.timeLimitMs === 60000;
const provenIn20sEndToEnd = row => row?.completion === 'Complete' && row.optimality === 'proven'
  && Number.isFinite(row.processWallMs) && row.processWallMs >= 0 && row.processWallMs <= 20000 && row.timeLimitMs === 60000;
const endToEndScope = {
  browser: 'processWallMs: Node page.evaluate invocation through the final Worker response; includes input transport, Worker creation, WASM initialization, dataset/DeckSolver construction and recommendation. Excludes host fixture reads, HTTP server/browser startup and browser installation.',
  native: 'processWallMs: native process spawn through process exit; includes process startup, input reads, dataset construction, recommendation and output persistence. Excludes benchmark runner startup and host-side result validation.',
};

function summarizeRuns(runs, planned, requiredNames) {
  const completed = runs.filter(run => run.finished);
  const valid = runtime => completed.filter(run => run[runtime]?.passed === true);
  const native = valid('native'), browser = valid('browser');
  const compared = completed.filter(run => run.comparison?.compatibleCertificates === true);
  const required = runs.filter(run => requiredNames.includes(run.name));
  const counts = rows => rows.reduce((out, run) => {
    const completion = run.browser.row.completion;
    out[completion] = (out[completion] || 0) + 1;
    return out;
  }, {});
  return {
    planned, finished: completed.length, nativeValid: native.length, browserValid: browser.length,
    compatibleComparisons: compared.length, canonicalTopKCompared: compared.filter(run => run.comparison.bothComplete).length,
    browserProvenWithinBudget60s: browser.filter(run => provenInBudget(run.browser.row)).length,
    nativeProvenWithinBudget60s: native.filter(run => provenInBudget(run.native.row)).length,
    browserProvenWithin20sEndToEnd: browser.filter(run => provenIn20sEndToEnd(run.browser.row)).length,
    nativeProvenWithin20sEndToEnd: native.filter(run => provenIn20sEndToEnd(run.native.row)).length,
    browserCompletions: counts(browser),
    allAccountedFor: completed.length === planned,
    allExecuted: planned > 0 && native.length === planned && browser.length === planned,
    contractsPassed: planned > 0 && native.length === planned && browser.length === planned && compared.length === planned,
    requiredCompletionPassed: required.every(run => run.finished && run.browser?.passed && provenInBudget(run.browser.row)),
    targetMet: planned > 0 && browser.length === planned && browser.every(run => provenIn20sEndToEnd(run.browser.row)),
  };
}

function summaryMarkdown(report) {
  const s = report.summary;
  return [
    '## Chromium Worker search verification', '',
    `Source: \`${report.sourceCommit}\`. Cases accounted for: ${s.finished}/${s.planned}.`, '',
    `Execution and cross-runtime contracts: **${s.contractsPassed ? 'passed' : 'failed or unfinished'}**. `
      + `Chromium proven completion within 20 seconds end to end: **${s.browserProvenWithin20sEndToEnd}/${s.planned}**.`, '',
    `Native proven completion within 20 seconds end to end: **${s.nativeProvenWithin20sEndToEnd}/${s.planned}**. `
      + `Within the separate 60-second search budget: Chromium ${s.browserProvenWithinBudget60s}/${s.planned}, native ${s.nativeProvenWithinBudget60s}/${s.planned}.`, '',
    `Browser timing: ${endToEndScope.browser}`, '',
    `Native timing: ${endToEndScope.native}`, '',
    'Every case keeps K=3, no candidate limit, the complete declared ownership domain and all 120 uniform performance orders. '
      + 'Complete pairs compare the canonical Top-K; incomplete pairs compare common returned certificates and retain their incomplete status.', '',
    '| Case | Native status | Native end-to-end ms | Chromium status | Chromium search ms | Chromium end-to-end ms | Canonical Top-K compared |',
    '| --- | --- | ---: | --- | ---: | ---: | --- |',
    ...report.runs.map(run => {
      const status = runtime => run[runtime]?.passed ? run[runtime].row.completion : run[runtime]?.error ? 'runner error' : 'not finished';
      const milliseconds = (runtime, field) => Number.isFinite(run[runtime]?.row?.[field]) ? run[runtime].row[field].toFixed(1) : '—';
      return `| ${run.name} (${run.repeat}) | ${status('native')} | ${milliseconds('native', 'processWallMs')} | ${status('browser')} | ${milliseconds('browser', 'searchWallMs')} | ${milliseconds('browser', 'processWallMs')} | ${run.comparison?.bothComplete ? 'yes' : 'no'} |`;
    }), '',
    'Full requests, byte hashes, build receipt, runtime reports, stderr/stdout and complete answer JSON are available in the run artifacts.', '',
  ].join('\n');
}

function childRun(args, logFile, timeout) {
  return new Promise(resolve => {
    const log = fs.openSync(logFile, 'w');
    let child;
    try {
      child = spawn(process.execPath, args, { detached: process.platform !== 'win32', stdio: ['ignore', log, log] });
    } catch (error) {
      fs.closeSync(log);
      resolve({ passed: false, error: error.message });
      return;
    }
    fs.closeSync(log);
    let error;
    const timer = setTimeout(() => {
      error = 'benchmark runner external deadline';
      try {
        // Kill the whole local CI process group, including a browser stuck before Worker startup.
        if (process.platform !== 'win32') process.kill(-child.pid, 'SIGKILL');
        else child.kill('SIGKILL');
      } catch (failure) { if (failure.code !== 'ESRCH') error += `: ${failure.message}`; }
    }, timeout);
    child.on('error', failure => { error = failure.message; });
    child.on('close', (code, signal) => {
      clearTimeout(timer);
      resolve({ passed: code === 0 && !error, code, signal, ...(error ? { error } : {}) });
    });
  });
}

async function run(manifestFile, bundleDirectory, outputDirectory, options) {
  const source = path.resolve(options.source || path.join(__dirname, '../..'));
  const directory = path.resolve(bundleDirectory), out = path.resolve(outputDirectory), manifestPath = path.resolve(manifestFile);
  const { manifest, build } = verifyBundle(manifestPath, directory, source);
  const cases = selectCases(manifest, options.cases || 'all');
  const repeats = Number(options.repeats || 1);
  assert(Number.isSafeInteger(repeats) && repeats >= 1 && repeats <= 3, 'repeats must be 1, 2 or 3');
  assert.equal(cases.length, Number(options['expected-cases']), 'selected shard does not cover its declared case count');
  const requiredSelection = options['require-complete'] || 'all';
  const required = requiredSelection === 'all' ? cases.map(entry => entry.name) : requiredSelection === 'none' ? [] : requiredSelection.split(',');
  const requireTarget = options['require-target'] || 'false';
  assert(['true', 'false'].includes(requireTarget), '--require-target must be true or false');
  for (const name of required) assert(cases.some(entry => entry.name === name), `required completion is outside the selected cases: ${name}`);
  assert(!fs.existsSync(out), 'result directory already exists');
  fs.mkdirSync(out, { recursive: true });
  const report = {
    format: 'ournotes-deck.browser-ci/1', sourceCommit: build.source.head,
    buildReceiptSha256: sha(fs.readFileSync(path.join(directory, 'build-receipt.json'))),
    inputReceiptSha256: sha(fs.readFileSync(path.join(path.dirname(manifestPath), 'inputs-receipt.json'))),
    manifestSha256: sha(fs.readFileSync(manifestPath)),
    runtime: 'actualChromiumWorker', requestTimeLimitMs: 60000, externalWorkerWatchdogMs: manifest.timeoutMs,
    targetEndToEndMs: 20000, endToEndScope, requireTarget: requireTarget === 'true',
    requiredCompleteCases: required, finished: false, passed: false,
    independentOracle: 'notRun', scope: 'Same-source native/Chromium consistency and declared completion targets; not game parity.',
    runs: cases.flatMap(entry => Array.from({ length: repeats }, (_, index) => ({ name: entry.name, repeat: index + 1, finished: false }))),
  };
  const save = () => {
    report.summary = summarizeRuns(report.runs, cases.length * repeats, required);
    write(path.join(out, 'report.json'), report);
    fs.writeFileSync(path.join(out, 'summary.md'), summaryMarkdown(report));
  };
  save();
  for (const [index, current] of report.runs.entries()) {
    const caseDirectory = path.join(out, current.name, String(current.repeat));
    fs.mkdirSync(caseDirectory, { recursive: true });
    for (const runtime of index % 2 ? ['browser', 'native'] : ['native', 'browser']) {
      const destination = path.join(caseDirectory, runtime), logFile = path.join(caseDirectory, `${runtime}.log`);
      const binary = runtime === 'native' ? path.join(directory, 'bin/profile_case') : path.join(directory, 'pkg');
      console.log(JSON.stringify({ type: 'start', name: current.name, repeat: current.repeat, runtime }));
      current[runtime] = { passed: false, started: true };
      save();
      const execution = await childRun([path.join(__dirname, 'benchmark.cjs'), manifestPath, binary, destination,
        '--candidate-only', '--runtime', runtime, '--cases', current.name, '--candidate-source', source],
      logFile, manifest.timeoutMs + 60000);
      current[runtime] = { ...execution, report: path.relative(out, path.join(destination, 'report.json')), log: path.relative(out, logFile) };
      try {
        assert(execution.passed, execution.error || `benchmark exited ${execution.code}, signal ${execution.signal}`);
        const result = json(path.join(destination, 'report.json'));
        assert.equal(result.passed, true, 'benchmark execution or answer contract failed');
        assert.equal(result.rows.length, 1, 'each process must evaluate exactly one declared request');
        const row = result.rows[0];
        assert.equal(row.name, current.name);
        assert.equal(row.variant, 'candidate');
        assert.equal(row.runtime, runtime);
        assert.equal(row.timeLimitMs, 60000);
        assert.equal(row.k, 3);
        const outcome = json(path.join(destination, `${current.name}-1-candidate.json`)).result;
        assert.equal(outcome.probabilityLaw?.kind, 'uniformMemberOrder');
        assert.equal(outcome.probabilityLaw?.orders, 120, 'all original uniform labels are required');
        assert.equal(outcome.resultIdentity, 'team');
        if (runtime === 'browser') {
          assert.equal(typeof result.chromium, 'string', 'actual Chromium version required');
          assert.equal(row.sharedMemory, false, 'Worker must use unshared WASM memory');
          current[runtime].chromium = result.chromium;
        }
        current[runtime].row = row;
      } catch (error) {
        current[runtime].passed = false;
        current[runtime].error = error.message;
      }
      save();
      console.log(JSON.stringify({ type: 'done', name: current.name, repeat: current.repeat, runtime,
        passed: current[runtime].passed, completion: current[runtime].row?.completion, error: current[runtime].error }));
    }
    if (current.native.passed && current.browser.passed) {
      try {
        const answer = runtime => read(path.join(caseDirectory, runtime, `${current.name}-1-candidate.json`));
        current.comparison = compareAnswers(answer('native'), answer('browser'));
      } catch (error) { current.comparisonError = error.message; }
    }
    current.finished = true;
    save();
  }
  report.finished = true;
  save();
  report.passed = report.summary.contractsPassed && report.summary.requiredCompletionPassed
    && (!report.requireTarget || report.summary.targetMet);
  save();
  if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summaryMarkdown(report));
  return report;
}

function optionsFrom(args) {
  const result = {};
  assert(args.length % 2 === 0, 'options require --name value pairs');
  for (let index = 0; index < args.length; index += 2) {
    assert(['--source', '--cases', '--repeats', '--expected-cases', '--require-complete', '--require-target'].includes(args[index]), 'unknown option');
    const name = args[index].slice(2);
    assert(!Object.hasOwn(result, name), `duplicate option ${name}`);
    result[name] = args[index + 1];
  }
  return result;
}

function collect(resultDirectory, outputDirectory, matrixText, repeats, sourceCommit) {
  const matrix = JSON.parse(matrixText), runs = [], shards = [], required = new Set();
  const count = Number(repeats);
  assert(Number.isSafeInteger(count) && count > 0);
  assert(Array.isArray(matrix.include) && matrix.include.length > 0);
  for (const shard of matrix.include) {
    const file = path.join(resultDirectory, `browser-results-${safeName(shard.name)}`, 'report.json');
    try {
      const report = json(file);
      assert.equal(report.format, 'ournotes-deck.browser-ci/1');
      assert.equal(report.sourceCommit, sourceCommit, 'shards must use the same source commit');
      assert.equal(report.summary.planned, shard.expectedCases * count, 'shard coverage changed');
      runs.push(...report.runs);
      report.requiredCompleteCases.forEach(name => required.add(name));
      shards.push({ name: shard.name, sha256: sha(fs.readFileSync(file)), passed: report.passed, finished: report.finished });
    } catch (error) { shards.push({ name: shard.name, passed: false, error: error.message }); }
  }
  const identities = runs.map(run => `${run.name}/${run.repeat}`);
  assert.equal(new Set(identities).size, identities.length, 'shards must not duplicate requests');
  const planned = matrix.include.reduce((sum, shard) => sum + shard.expectedCases * count, 0);
  const report = {
    format: 'ournotes-deck.browser-ci-collection/1', sourceCommit, shards, runs,
    targetEndToEndMs: 20000, requestTimeLimitMs: 60000, endToEndScope,
    summary: summarizeRuns(runs, planned, [...required]),
  };
  report.passed = shards.every(shard => shard.passed) && report.summary.contractsPassed && report.summary.requiredCompletionPassed;
  write(path.join(outputDirectory, 'report.json'), report);
  fs.writeFileSync(path.join(outputDirectory, 'summary.md'), summaryMarkdown(report));
  if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summaryMarkdown(report));
  return report;
}

async function main() {
  const [command, ...args] = process.argv.slice(2);
  if (command === 'plan') { assert.equal(args.length, 1); console.log(JSON.stringify(plan(args[0]))); }
  else if (command === 'bundle') { assert.equal(args.length, 2); console.log(bundle(...args)); }
  else if (command === 'receipt') { assert.equal(args.length, 2); buildReceipt(...args); }
  else if (command === 'collect') {
    assert.equal(args.length, 5, 'collect RESULTS OUTPUT MATRIX_JSON REPEATS SOURCE_COMMIT');
    if (!collect(...args).passed) process.exitCode = 1;
  }
  else if (command === 'run') {
    assert(args.length >= 3, 'run MANIFEST BUNDLE OUTPUT --expected-cases N [--cases SELECTION --require-complete all|none|NAMES --require-target true|false --repeats N --source ROOT]');
    const result = await run(...args.slice(0, 3), optionsFrom(args.slice(3)));
    if (!result.passed) process.exitCode = 1;
  } else throw Error('browser-ci.cjs plan|bundle|receipt|run|collect');
}

if (require.main === module) main().catch(error => { console.error(error.stack); process.exitCode = 1; });
module.exports = { plan, requestContract, selectCases, bundle, provenInBudget, provenIn20sEndToEnd, summarizeRuns, summaryMarkdown, collect };
