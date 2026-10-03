// Compare complete native owned-snapshot answers with the same Rust core in Chromium Workers.
// Keep JSON number tokens intact, including integers above JavaScript's safe range.
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const { chromium } = require('playwright');
const { projection } = require('./json-tokens.cjs');
const sha = value => crypto.createHash('sha256').update(value).digest('hex');

async function main() {
  const [manifestFile, pkgDirectory, outputDirectory] = process.argv.slice(2);
  if (!outputDirectory) throw Error('usage: node browser.cjs MANIFEST.json WASM_PACKAGE OUTPUT_DIRECTORY');
  const manifestPath = path.resolve(manifestFile), root = path.dirname(manifestPath);
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  const pkg = path.resolve(pkgDirectory), out = path.resolve(outputDirectory);
  fs.mkdirSync(out, { recursive: true });
  const worker = `import init,{DeckSolver} from '/pkg/ournotes_recommend_wasm.js';
self.onmessage=async({data})=>{let solver;try{await init();
const hashes=await Promise.all([data.data,data.snapshot,data.request].map(async text=>
Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(text))),x=>x.toString(16).padStart(2,'0')).join('')));
solver=new DeckSolver(data.data);
const begin=performance.now();const result=solver.recommend(data.snapshot,data.request);
postMessage({result,hashes,datasetId:solver.datasetId,searchWallMs:performance.now()-begin});
}catch(e){postMessage({error:String(e)});}finally{solver?.free();}};`;
  const files = new Set(['ournotes_recommend_wasm.js', 'ournotes_recommend_wasm_bg.wasm']);
  const server = http.createServer((req, res) => {
    if (req.url === '/') { res.setHeader('Content-Type', 'text/html'); return res.end('<!doctype html><title>Search verification</title>'); }
    if (req.url === '/worker.js') { res.setHeader('Content-Type', 'application/javascript'); return res.end(worker); }
    const name = req.url.startsWith('/pkg/') ? req.url.slice(5) : '';
    if (!files.has(name)) { res.writeHead(404); return res.end(); }
    res.setHeader('Content-Type', name.endsWith('.wasm') ? 'application/wasm' : 'application/javascript');
    res.end(fs.readFileSync(path.join(pkg, name)));
  });
  let browser;
  try {
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    browser = await chromium.launch({ headless: true, args: ['--no-sandbox'] });
    const page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const cases = [];
    for (const entry of manifest.cases) {
      assert.match(entry.name, /^[A-Za-z0-9_-]+$/);
      const input = Object.fromEntries(['data', 'snapshot', 'request'].map(k => [k, fs.readFileSync(path.resolve(root, entry[k]), 'utf8')]));
      const reference = fs.readFileSync(path.resolve(root, entry.reference), 'utf8');
      assert.equal(JSON.parse(reference).result?.completion, 'Complete', `${entry.name}: reference must be complete`);
      const begin = performance.now();
      const actual = await page.evaluate(({ input, timeout }) => new Promise((resolve, reject) => {
        const worker = new Worker('/worker.js', { type: 'module' });
        const timer = setTimeout(() => { worker.terminate(); reject(Error('Worker deadline')); }, timeout);
        worker.onerror = event => { clearTimeout(timer); worker.terminate(); reject(Error(event.message)); };
        worker.onmessage = ({ data }) => { clearTimeout(timer); worker.terminate(); resolve(data); };
        worker.postMessage(input);
      }), { input, timeout: manifest.timeoutMs || 300000 });
      assert.equal(actual.error, undefined, `${entry.name}: ${actual.error}`);
      assert.deepEqual(actual.hashes, ['data', 'snapshot', 'request'].map(k => sha(input[k])), `${entry.name}: original UTF-8 transport`);
      assert.equal(actual.datasetId, sha(input.data), `${entry.name}: dataset identity`);
      assert.deepEqual(projection(actual.result), projection(reference), `${entry.name}: exact semantic JSON tokens`);
      fs.writeFileSync(path.join(out, `${entry.name}.json`), actual.result);
      const row = { name: entry.name, complete: true, exactSemanticTokensMatch: true, inputSha256: actual.hashes,
        referenceSha256: sha(reference), semanticProjectionSha256: sha(JSON.stringify(projection(actual.result))),
        searchWallMs: actual.searchWallMs, workerWallMs: performance.now() - begin };
      cases.push(row);
      console.log(JSON.stringify(row));
    }
    fs.writeFileSync(path.join(out, 'report.json'), JSON.stringify({ format: 'ournotes-deck.browser-search-check/1',
      passed: true, scope: 'Declared inputs in actual Chromium Workers; shared-model native/WASM equivalence, not game parity.',
      node: process.version, chromium: browser.version(), manifestSha256: sha(fs.readFileSync(manifestPath)),
      runnerSha256: sha(fs.readFileSync(__filename)), wasmSha256: sha(fs.readFileSync(path.join(pkg, 'ournotes_recommend_wasm_bg.wasm'))),
      cases }, null, 2));
  } finally {
    await browser?.close();
    server.close();
  }
}
main().catch(error => { console.error(error.stack); process.exitCode = 1; });
