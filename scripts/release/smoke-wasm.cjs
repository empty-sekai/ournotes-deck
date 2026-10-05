// Check extracted Node bindings, then load both web modules in an actual Chromium Worker.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const crypto = require('node:crypto');
const { chromium } = require('playwright');
const [recommendArg, replayArg, corpusArg] = process.argv.slice(2);
const recommendRoot = path.resolve(recommendArg), replayRoot = path.resolve(replayArg);
const corpus = path.resolve(corpusArg);
for (const root of [recommendRoot, replayRoot]) {
  const info = JSON.parse(fs.readFileSync(path.join(root, 'build-info.json')));
  for (const [file, digest] of Object.entries(info.files)) {
    assert.equal(crypto.createHash('sha256').update(fs.readFileSync(path.join(root, file))).digest('hex'), digest);
  }
}
const data = fs.readFileSync(path.join(corpus, 'data.json'), 'utf8');
const account = fs.readFileSync(path.join(corpus, 'account.json'), 'utf8');
const inputs = ['power', 'free'].map(name => ({ name,
  request: fs.readFileSync(path.join(corpus, `${name}.request.json`), 'utf8'),
  expected: JSON.parse(fs.readFileSync(path.join(corpus, `${name}.expected.json`))) }));
const untimed = answer => {
  const copy = structuredClone(answer);
  if (copy.result) { delete copy.result.elapsedMs; delete copy.result.telemetry; }
  return copy;
};
const { ReplaySession } = require(path.join(replayRoot, 'nodejs/ournotes_replay_wasm.js'));
const session = new ReplaySession(data);
const chart = JSON.parse(session.describeChart(1004));
assert(chart.notes.length > 0);
const template = session.template(1004, 100000, 60);
const replay = JSON.parse(session.run(template));
assert.equal(replay.format, 'ournotes.replay-result/1');
assert.equal(replay.complete, true);
assert(replay.score > 0 && replay.frameCount > 0);
session.free();
const worker = `import initRecommend,{DeckSolver} from '/recommend/ournotes_recommend_wasm.js';
import initReplay,{ReplaySession} from '/replay/ournotes_replay_wasm.js';
self.onmessage=async({data:input})=>{try{
await Promise.all([initRecommend(),initReplay()]);
const solver=new DeckSolver(new TextEncoder().encode(input.data));
const answers=input.inputs.map(({request})=>JSON.parse(solver.recommend(input.account,request)));
const session=new ReplaySession(input.data);
const chart=JSON.parse(session.describeChart(1004));
const result=JSON.parse(session.run(session.template(1004,100000,60)));
solver.free();session.free();postMessage({answers,chart,replay:result});
}catch(error){postMessage({error:String(error)});}};`;
const server = http.createServer((req, res) => {
  if (req.url === '/') { res.setHeader('Content-Type', 'text/html'); return res.end('<!doctype html><title>Release verification</title>'); }
  if (req.url === '/worker.js') { res.setHeader('Content-Type', 'application/javascript'); return res.end(worker); }
  const match = /^\/(recommend|replay)\/(ournotes_(?:recommend|replay)_wasm(?:_bg\.wasm|\.js))$/.exec(req.url);
  if (!match) { res.writeHead(404); return res.end(); }
  const root = match[1] === 'recommend' ? recommendRoot : replayRoot;
  res.setHeader('Content-Type', match[2].endsWith('.wasm') ? 'application/wasm' : 'application/javascript');
  res.end(fs.readFileSync(path.join(root, 'web', match[2])));
});
(async () => {
  let browser;
  try {
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const actual = await page.evaluate(input => new Promise((resolve, reject) => {
      const worker = new Worker('/worker.js', { type: 'module' });
      const timer = setTimeout(() => { worker.terminate(); reject(Error('Worker deadline')); }, 120000);
      worker.onerror = event => { clearTimeout(timer); worker.terminate(); reject(Error(event.message)); };
      worker.onmessage = ({ data }) => { clearTimeout(timer); worker.terminate(); resolve(data); };
      worker.postMessage(input);
    }), { data, account, inputs });
    assert.equal(actual.error, undefined, actual.error);
    inputs.forEach(({name, expected}, index) => assert.deepEqual(untimed(actual.answers[index]), untimed(expected), name));
    assert.deepEqual(actual.chart, chart);
    assert.deepEqual(actual.replay, replay);
    console.log(JSON.stringify({ status: 'passed', recommendationNativeCases: inputs.length,
      replayNodeWebEqual: true, chromium: browser.version() }));
  } finally {
    await browser?.close();
    server.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
