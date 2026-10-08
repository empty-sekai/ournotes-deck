// Serial native or real Chromium Worker measurements over immutable requests.
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const os = require('node:os');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { spawn, execFileSync } = require('node:child_process');
const { projection } = require('./json-tokens.cjs');

const sha = value => crypto.createHash('sha256').update(value).digest('hex');
const read = file => fs.readFileSync(file, 'utf8');
function parseArguments(args) {
  const options = {}, positional = [];
  const valued = new Set(['--runtime', '--repeats', '--cases', '--baseline-source', '--candidate-source']);
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '--candidate-only') options[arg] = true;
    else if (valued.has(arg)) {
      assert(args[i + 1] && !args[i + 1].startsWith('--'), `${arg} requires a value`);
      assert(!Object.hasOwn(options, arg), `duplicate option ${arg}`);
      options[arg] = args[++i];
    } else {
      assert(!arg.startsWith('--'), `unknown option ${arg}`);
      positional.push(arg);
    }
  }
  const candidateOnly = options['--candidate-only'] === true;
  assert.equal(positional.length, candidateOnly ? 3 : 4,
    'benchmark.cjs MANIFEST BASELINE CANDIDATE OUTPUT [options]\n'
    + 'benchmark.cjs MANIFEST CANDIDATE OUTPUT --candidate-only [options]\n'
    + 'options: --runtime native|browser --repeats N --cases NAME,... --baseline-source ROOT --candidate-source ROOT');
  assert(!candidateOnly || !options['--baseline-source'], '--baseline-source requires paired mode');
  const [manifest, first, second, fourth] = positional;
  const runtime = options['--runtime'] ?? 'native';
  assert(['native', 'browser'].includes(runtime), 'runtime must be native or browser');
  const repeats = Number(options['--repeats'] ?? (candidateOnly ? 1 : 2));
  assert(Number.isSafeInteger(repeats) && repeats > 0, 'positive integer repeat count');
  return { manifest, baseline: candidateOnly ? null : first, candidate: candidateOnly ? first : second,
    output: candidateOnly ? second : fourth, candidateOnly, runtime, repeats,
    selected: options['--cases']?.split(','),
    sources: { baseline: options['--baseline-source'], candidate: options['--candidate-source'] } };
}
const runOrder = (candidateOnly, repeat, caseIndex) => candidateOnly ? ['candidate']
  : (repeat + caseIndex) % 2 ? ['candidate', 'baseline'] : ['baseline', 'candidate'];
const fractionCompare = (a, b) => {
  const difference = BigInt(a.numerator) * BigInt(b.denominator) - BigInt(b.numerator) * BigInt(a.denominator);
  return difference < 0n ? -1 : difference > 0n ? 1 : 0;
};
const span = (team, kind) => {
  const exact = team[kind === 'score' ? 'expectedScore' : 'expectedPayoff'];
  return exact ? { lower: exact, upper: exact } : team[`${kind}Interval`];
};
function teamKeys(text) {
  const field = (fields, key) => fields.find(([name]) => name === key)?.[1];
  const root = projection(text);
  const result = field(root, 'result');
  const teams = field(result ? result[1] : root, 'results');
  return (teams?.[1] || []).map(team => JSON.stringify(['members', 'snaps', 'power'].map(key => field(team[1], key))));
}
function compareAnswers(leftText, rightText) {
  const left = JSON.parse(leftText).result, right = JSON.parse(rightText).result;
  assert(left && right, 'both requests must return valid recommendation results');
  const field = (fields, key) => fields.find(([name]) => name === key)?.[1];
  const leftTokens = projection(leftText), rightTokens = projection(rightText);
  assert.deepEqual(field(leftTokens, 'datasetId'), field(rightTokens, 'datasetId'), 'dataset identity changed');
  const leftFields = field(leftTokens, 'result')[1], rightFields = field(rightTokens, 'result')[1];
  for (const name of ['metric', 'probabilityLaw', 'resultIdentity', 'resolvedContext', 'playerGoal', 'strategy', 'proofScope']) {
    assert.deepEqual(field(leftFields, name), field(rightFields, name), `${name} semantics changed`);
  }
  const leftKeys = teamKeys(leftText), rightKeys = teamKeys(rightText);
  const bothComplete = left.completion === 'Complete' && right.completion === 'Complete';
  if (bothComplete) assert.deepEqual(leftKeys, rightKeys, 'complete canonical Top-K changed');
  const index = new Map(leftKeys.map((key, i) => [key, i]));
  let sharedTeams = 0;
  for (let i = 0; i < rightKeys.length; i++) {
    if (!index.has(rightKeys[i])) continue;
    sharedTeams++;
    const a = left.results[index.get(rightKeys[i])], b = right.results[i];
    for (const kind of ['score', 'payoff']) {
      const x = span(a, kind), y = span(b, kind);
      if (x && y) {
        assert(fractionCompare(x.lower, y.upper) <= 0 && fractionCompare(y.lower, x.upper) <= 0,
          `disjoint ${kind} certificates for an identical team`);
      }
    }
  }
  return { bothComplete, canonicalTopKEqual: bothComplete ? true : null, sharedTeams, compatibleCertificates: true };
}
function distribution(values) {
  const sorted = values.filter(Number.isFinite).sort((a, b) => a - b);
  const q = p => sorted.length ? sorted[Math.min(sorted.length - 1, Math.ceil(p * sorted.length) - 1)] : null;
  return { count: sorted.length, min: sorted[0] ?? null, p50: q(0.5), p90: q(0.9), max: sorted.at(-1) ?? null };
}
function completionSummary(rows) {
  const complete = rows.filter(row => row.completion === 'Complete');
  const proven = complete.filter(row => row.optimality === 'proven');
  const withinBudget = row => Number.isFinite(row.searchWallMs) && Number.isFinite(row.timeLimitMs)
    && row.searchWallMs <= row.timeLimitMs;
  return { requests: rows.length, complete: complete.length, proven: proven.length,
    completeWithinBudget: complete.filter(withinBudget).length, provenWithinBudget: proven.filter(withinBudget).length,
    allComplete: rows.length > 0 && complete.length === rows.length,
    allProvenWithinBudget: rows.length > 0 && proven.filter(withinBudget).length === rows.length,
    completions: rows.reduce((counts, row) => {
      counts[row.completion] = (counts[row.completion] || 0) + 1;
      return counts;
    }, {}) };
}
function summarize(rows, labels = ['baseline', 'candidate']) {
  return Object.fromEntries(labels.map(label => {
    const selected = rows.filter(row => row.variant === label);
    const byFamily = {};
    for (const row of selected) {
      const family = byFamily[row.family] ||= { requests: 0, complete: 0, completions: {} };
      family.requests++;
      family.complete += row.completion === 'Complete' ? 1 : 0;
      family.completions[row.completion] = (family.completions[row.completion] || 0) + 1;
    }
    const byCase = {};
    for (const name of new Set(selected.map(row => row.name))) {
      const samples = selected.filter(row => row.name === name);
      byCase[name] = {
        ...completionSummary(samples),
        searchWallMs: distribution(samples.map(row => row.searchWallMs)), peakBytes: distribution(samples.map(row => row.peakBytes)),
        candidates: distribution(samples.map(row => row.leaves.visited)),
        simulations: distribution(samples.map(row => row.leaves.simulations)),
        refinementFrames: distribution(samples.map(row => row.refinement.frames)),
        workSamples: samples.map(row => ({ repeat: row.repeat, completion: row.completion, nodes: row.nodes,
          leaves: row.leaves, caches: row.caches, refinement: row.refinement })),
      };
    }
    return [label, {
      ...completionSummary(selected),
      searchWallMs: distribution(selected.map(row => row.searchWallMs)),
      completeWallMs: distribution(selected.filter(row => row.completion === 'Complete').map(row => row.searchWallMs)),
      peakBytes: distribution(selected.map(row => row.peakBytes)), byFamily, byCase,
    }];
  }));
}
function sourceIdentity(directory) {
  if (!directory) return null;
  const root = path.resolve(directory);
  const git = args => execFileSync('git', ['-C', root, ...args], { encoding: 'utf8' });
  const sourceManifest = {};
  for (const name of new Set(git(['ls-files', '--cached', '--others', '--exclude-standard', '-z']).split('\0').filter(Boolean))) {
    if (!(name.startsWith('crates/') || name.startsWith('wasm/recommend/') || name.startsWith('tools/search-harness/')
      || name === 'Cargo.toml' || name === 'Cargo.lock')) continue;
    if (name.split('/').some(part => part === 'target' || part === '__pycache__')) continue;
    const file = path.join(root, name);
    if (fs.existsSync(file) && fs.statSync(file).isFile()) sourceManifest[name] = sha(fs.readFileSync(file));
  }
  const sorted = Object.fromEntries(Object.entries(sourceManifest).sort(([a], [b]) => a.localeCompare(b)));
  return { root, head: git(['rev-parse', 'HEAD']).trim(), sourceManifest: sorted, manifestSha256: sha(JSON.stringify(sorted)) };
}
async function nativeRun(binary, files, output, timeout) {
  const begin = performance.now();
  let stdout = '', stderr = '';
  try {
    await new Promise((resolve, reject) => {
      const child = spawn(binary, [files.data, files.snapshot, files.request, output], { stdio: ['ignore', 'pipe', 'pipe'] });
      let failure;
      const timer = setTimeout(() => {
        failure = Error('native process external deadline');
        child.kill('SIGKILL');
      }, timeout);
      child.stdout.on('data', data => { stdout += data; });
      child.stderr.on('data', data => { stderr += data; });
      child.on('error', error => { failure ||= error; });
      // close follows process exit and drains both output streams, including after a watchdog kill.
      child.on('close', (code, signal) => {
        clearTimeout(timer);
        if (failure) reject(failure);
        else if (code === 0) resolve();
        else reject(Error(`native process exit ${code}, signal ${signal}: ${stderr}`));
      });
    });
  } finally {
    fs.writeFileSync(`${output}.log`, stdout + stderr);
  }
  const records = stdout.trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
  const done = [...records].reverse().find(row => Number.isFinite(row.elapsedMs));
  assert(done, 'native binary must emit an elapsedMs record (profile_case or benchmark_case)');
  return { result: read(output), searchWallMs: done.elapsedMs, processWallMs: performance.now() - begin, profiles: done };
}
function validateAnswer(text, requestText, datasetId) {
  const answer = JSON.parse(text), request = JSON.parse(requestText), outcome = answer.result;
  assert.equal(answer.status, 'ok', 'recommendation status must be ok');
  assert.deepEqual(answer.errors, [], 'recommendation errors must be empty');
  assert.deepEqual(answer.missing, [], 'recommendation missing facts must be empty');
  assert(outcome, 'recommendation result is required');
  assert.equal(answer.datasetId, datasetId, 'dataset identity');
  const field = (fields, key) => fields.find(([name]) => name === key)?.[1];
  const expected = projection(requestText), actual = field(projection(text), 'result')[1];
  assert(field(expected, 'metric'), 'request metric is required');
  assert.deepEqual(field(actual, 'metric'), field(expected, 'metric'), 'request metric changed');
  assert.deepEqual(field(actual, 'strategy'), field(expected, 'strategy')
    ?? ['object', projection('{"kind":"branchAndBound"}')], 'request strategy changed');
  assert(['Complete', 'TimedOut', 'RefinementRequired'].includes(outcome.completion), 'known completion state');
  assert(Array.isArray(outcome.results), 'recommendation results must be an array');
  if (outcome.completion === 'Complete') {
    assert.equal(outcome.optimality, 'proven', 'complete recommendation must be proven');
    assert.equal(outcome.telemetry?.proof?.complete, true, 'complete recommendation must carry a complete proof');
    assert.equal(outcome.exitReason, 'exhausted', 'complete recommendation must exhaust its domain');
    assert.equal(outcome.results.length, request.k ?? 5, 'complete recommendation must return the requested K');
    assert(outcome.results.every(team => team.rankCertified !== false), 'uncertified returned rank');
  } else {
    assert.notEqual(outcome.optimality, 'proven', 'incomplete recommendation cannot be proven');
  }
  return outcome;
}
async function browserRunner(variants) {
  const { chromium } = require('playwright');
  const worker = `import init,{DeckSolver} from '/pkg/VARIANT/ournotes_recommend_wasm.js';
self.onmessage=async({data})=>{let solver;try{const module=await init();
if(!(module.memory.buffer instanceof ArrayBuffer))throw Error('shared WASM memory');
const hashes=await Promise.all([data.data,data.snapshot,data.request].map(async text=>
Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(text))),x=>x.toString(16).padStart(2,'0')).join('')));
solver=new DeckSolver(data.data);const beforeBytes=module.memory.buffer.byteLength;
const begin=performance.now();const result=solver.recommend(data.snapshot,data.request);
postMessage({result,hashes,datasetId:solver.datasetId,searchWallMs:performance.now()-begin,
wasmBeforeBytes:beforeBytes,wasmAfterBytes:module.memory.buffer.byteLength,sharedMemory:false});
}catch(error){postMessage({error:String(error)});}finally{solver?.free();}};`;
  const allowed = new Set(['ournotes_recommend_wasm.js', 'ournotes_recommend_wasm_bg.wasm']);
  const server = http.createServer((req, res) => {
    if (req.url === '/') { res.setHeader('Content-Type', 'text/html'); return res.end('<!doctype html><title>Search benchmark</title>'); }
    const parts = req.url.split('/');
    if (parts[1] === 'worker' && Object.hasOwn(variants, parts[2])) {
      res.setHeader('Content-Type', 'application/javascript');
      return res.end(worker.replace('VARIANT', parts[2]));
    }
    if (parts[1] !== 'pkg' || !Object.hasOwn(variants, parts[2]) || !allowed.has(parts[3])) {
      res.writeHead(404); return res.end();
    }
    res.setHeader('Content-Type', parts[3].endsWith('.wasm') ? 'application/wasm' : 'application/javascript');
    res.end(fs.readFileSync(path.join(variants[parts[2]], parts[3])));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await chromium.launch({ headless: true, executablePath: process.env.OURNOTES_CHROMIUM, args: ['--no-sandbox'] });
    const page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    return {
      version: browser.version(),
      async run(variant, input, timeout) {
        const begin = performance.now();
        const result = await page.evaluate(({ variant, input, timeout }) => new Promise((resolve, reject) => {
          const worker = new Worker(`/worker/${variant}`, { type: 'module' });
          const timer = setTimeout(() => { worker.terminate(); reject(Error('Worker external deadline')); }, timeout);
          worker.onerror = event => { clearTimeout(timer); worker.terminate(); reject(Error(event.message)); };
          worker.onmessage = ({ data }) => { clearTimeout(timer); worker.terminate(); resolve(data); };
          worker.postMessage(input);
        }), { variant, input, timeout });
        assert.equal(result.error, undefined, result.error);
        assert.deepEqual(result.hashes, ['data', 'snapshot', 'request'].map(key => sha(input[key])), 'original UTF-8 input');
        assert.equal(result.datasetId, sha(input.data), 'dataset identity');
        return { ...result, processWallMs: performance.now() - begin };
      },
      async close() { await browser.close(); await new Promise(resolve => server.close(resolve)); },
    };
  } catch (error) {
    await browser?.close(); server.close(); throw error;
  }
}
async function main() {
  const options = parseArguments(process.argv.slice(2));
  const { candidateOnly, runtime, repeats, selected } = options;
  const manifestPath = path.resolve(options.manifest), root = path.dirname(manifestPath);
  const variants = candidateOnly ? { candidate: path.resolve(options.candidate) }
    : { baseline: path.resolve(options.baseline), candidate: path.resolve(options.candidate) };
  const out = path.resolve(options.output);
  fs.mkdirSync(out, { recursive: true });
  const report = {
    format: 'ournotes-deck.search-benchmark-report/1', runtime, repeats,
    mode: candidateOnly ? 'candidateOnly' : 'paired',
    order: candidateOnly ? 'candidate once per repeat and case' : 'AB/BA alternated by repeat and case',
    passed: false, passedScope: 'executionAndAnswerContracts', independentOracle: 'notRun',
    manifestSha256: null, runnerSha256: sha(read(__filename)),
    node: process.version, platform: process.platform, arch: process.arch,
    cpus: os.cpus().length, cpuModel: os.cpus()[0]?.model, totalMemory: os.totalmem(),
    variants: {}, rows: [], comparisons: [],
  };
  const save = () => {
    report.summary = summarize(report.rows, Object.keys(variants));
    fs.writeFileSync(path.join(out, 'report.json'), JSON.stringify(report, null, 2));
  };
  let browser;
  try {
    const manifestText = read(manifestPath), manifest = JSON.parse(manifestText);
    report.manifestSha256 = sha(manifestText);
    assert.equal(manifest.format, 'ournotes-deck.search-benchmark/1');
    const cases = manifest.cases.filter(entry => !selected || selected.includes(entry.name));
    assert(cases.length > 0, 'selected cases are empty');
    if (selected) for (const name of selected) assert(cases.some(entry => entry.name === name), `unknown case ${name}`);
    report.variants = Object.fromEntries(Object.entries(variants).map(([label, location]) => [label, {
      path: location, sha256: sha(fs.readFileSync(runtime === 'native' ? location : path.join(location, 'ournotes_recommend_wasm_bg.wasm'))),
      source: sourceIdentity(options.sources[label]),
    }]));
    browser = runtime === 'browser' ? await browserRunner(variants) : null;
    if (browser) report.chromium = browser.version;
    for (let caseIndex = 0; caseIndex < cases.length; caseIndex++) {
      const entry = cases[caseIndex];
      assert.match(entry.name, /^[A-Za-z0-9_-]+$/);
      const files = Object.fromEntries(['data', 'snapshot', 'request'].map(key => [key, path.resolve(root, entry[key])]));
      const input = Object.fromEntries(Object.entries(files).map(([key, file]) => [key, read(file)]));
      const request = JSON.parse(input.request);
      assert.equal(request.limits.timeLimitMs, manifest.requestTimeLimitMs, 'request time limit must equal the manifest');
      for (let repeat = 0; repeat < repeats; repeat++) {
        const order = runOrder(candidateOnly, repeat, caseIndex);
        const answers = {};
        for (const variant of order) {
          const runId = `${entry.name}-${repeat + 1}-${variant}`;
          const output = path.join(out, `${runId}.json`);
          console.log(JSON.stringify({ type: 'start', name: entry.name, repeat: repeat + 1, variant, runtime }));
          const result = browser ? await browser.run(variant, input, manifest.timeoutMs || 90000)
            : await nativeRun(variants[variant], files, output, manifest.timeoutMs || 90000);
          fs.writeFileSync(output, result.result);
          const outcome = validateAnswer(result.result, input.request, sha(input.data));
          const telemetry = outcome.telemetry;
          const row = {
            name: entry.name, family: entry.family, metric: entry.metric, repeat: repeat + 1, variant, runtime,
            inputSha256: ['data', 'snapshot', 'request'].map(key => sha(input[key])), k: request.k,
            timeLimitMs: request.limits.timeLimitMs, cacheEntries: request.limits.cacheEntries,
            completion: outcome.completion, exitReason: outcome.exitReason, optimality: outcome.optimality,
            teams: outcome.results.length, certifiedRanks: outcome.results.filter(team => team.rankCertified === true).length,
            searchWallMs: result.searchWallMs, processWallMs: result.processWallMs,
            peakBytes: telemetry.memory.peakBytes,
            wasmBeforeBytes: result.wasmBeforeBytes, wasmAfterBytes: result.wasmAfterBytes, sharedMemory: result.sharedMemory,
            phases: telemetry.phases, time: telemetry.time, nodes: telemetry.nodes, leaves: telemetry.leaves,
            caches: telemetry.caches, refinement: telemetry.lotteryRefinement, profiles: result.profiles,
            answerSha256: sha(result.result), semanticProjectionSha256: sha(JSON.stringify(projection(result.result))),
          };
          report.rows.push(row); answers[variant] = result.result; save();
          console.log(JSON.stringify({ type: 'done', name: entry.name, repeat: repeat + 1, variant,
            completion: row.completion, searchWallMs: row.searchWallMs, peakBytes: row.peakBytes,
            candidates: row.leaves.visited, simulations: row.leaves.simulations, refinement: row.refinement }));
        }
        if (!candidateOnly) {
          report.comparisons.push({ name: entry.name, repeat: repeat + 1,
            ...compareAnswers(answers.baseline, answers.candidate) });
        }
        save();
      }
    }
    report.passed = true;
  } catch (error) {
    report.passed = false;
    report.error = { name: error.name, message: error.message };
    throw error;
  } finally {
    try {
      await browser?.close();
    } catch (error) {
      report.passed = false;
      report.error ||= { name: error.name, message: error.message };
      report.cleanupError = { name: error.name, message: error.message };
      throw error;
    } finally {
      save();
    }
  }
  const summary = Object.fromEntries(Object.entries(report.summary).map(([label, value]) => {
    const { byCase, ...totals } = value;
    return [label, totals];
  }));
  console.log(JSON.stringify({ type: 'summary', ...summary }));
}
if (require.main === module) main().catch(error => { console.error(error.stack); process.exitCode = 1; });
module.exports = { compareAnswers, distribution, summarize, parseArguments, runOrder, validateAnswer, sourceIdentity };
