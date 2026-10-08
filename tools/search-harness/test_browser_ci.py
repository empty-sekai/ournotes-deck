"""CI coverage, immutable request transport and completion accounting; no browser or solver required."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


HERE = Path(__file__).resolve().parent


class BrowserCIContracts(unittest.TestCase):
    def node(self, script, arguments=None):
        environment = dict(os.environ)
        environment.pop("GITHUB_STEP_SUMMARY", None)
        result = subprocess.run(
            ["node", "-e", "const ci = require('./browser-ci.cjs');\n"
             "const assert = require('node:assert/strict');\n" + script],
            input=json.dumps(arguments), text=True, capture_output=True, cwd=HERE, check=False, env=environment,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def test_full_plan_keeps_all_48_real_and_16_synthetic_cases(self):
        self.node(r"""
const fs = require('node:fs');
const matrix = JSON.parse(fs.readFileSync('fixtures/full48/matrix.json', 'utf8'));
const full = ci.plan('all');
assert.equal(full.matrix.include.reduce((n, shard) => n + shard.expectedCases, 0), 64);
const real = full.matrix.include.filter(shard => shard.suite === 'full48');
assert.equal(real.length, 3);
const seen = [];
for (const shard of real) {
  const profile = shard.cases.slice('profile:'.length);
  const names = matrix.cases.filter(row => row.profile === profile).map(row => row.name);
  assert.equal(names.length, shard.expectedCases);
  seen.push(...names);
}
assert.equal(new Set(seen).size, 48);
assert.equal(ci.plan('synthetic').needReal, false);
assert.equal(ci.plan('full48').needSynthetic, false);
assert.equal(ci.plan('smoke').matrix.include.length, 2);
assert.throws(() => ci.plan('unknown'));
for (const row of matrix.cases) ci.requestContract(JSON.stringify(row.request));
""")

    def test_budget_k_and_unrestricted_domain_are_not_rewritten(self):
        self.node(r"""
const request = {format:'ournotes-deck.search-request/1', execution:{kind:'live'}, k:3,
  limits:{timeLimitMs:60000,maxCandidates:null,cacheEntries:0}, constraints:{}};
const text = JSON.stringify(request);
assert.deepEqual(ci.requestContract(text), request);
assert.equal(JSON.stringify(request), text);
for (const mutate of [r=>r.k=1, r=>r.limits.timeLimitMs=60001, r=>r.limits.maxCandidates=1,
  r=>r.constraints={leader:1}, r=>r.execution.kind='skip']) {
  const changed = structuredClone(request); mutate(changed);
  assert.throws(() => ci.requestContract(JSON.stringify(changed)));
}
const manifest = {format:'ournotes-deck.search-benchmark/1',requestTimeLimitMs:60000,timeoutMs:90000,
  cases:[{name:'one',profile:'newcomer'},{name:'two',profile:'veteran'}]};
assert.deepEqual(ci.selectCases(manifest,'all'), manifest.cases);
assert.deepEqual(ci.selectCases(manifest,'profile:newcomer'), [manifest.cases[0]]);
assert.throws(() => ci.selectCases(manifest,'one,one'));
assert.throws(() => ci.selectCases(manifest,'missing'));
assert.throws(() => ci.selectCases(manifest,'profile:missing'));
""")

    def test_portable_bundle_preserves_original_large_integer_and_whitespace_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            self.node(r"""
const fs = require('node:fs'), path = require('node:path'), crypto = require('node:crypto');
const root = JSON.parse(fs.readFileSync(0,'utf8'));
const digest = raw => crypto.createHash('sha256').update(raw).digest('hex');
const data = '{ "format": "nnnotes.deck-data/1", "large": 9007199254740993 }\n';
const snapshot = JSON.stringify({datasetId:digest(data),eligible:{members:[],snaps:[]}});
const request = '{ "format":"ournotes-deck.search-request/1", "execution":{"kind":"live"},'
  + '"k":3,"limits":{"timeLimitMs":60000,"maxCandidates":null,"cacheEntries":1024},'
  + '"constraints":{},"context":{"ticks":638976432000000001} }\n';
for (const [name, value] of Object.entries({data,snapshot,request})) fs.writeFileSync(path.join(root,`${name}.json`),value);
const manifest = {format:'ournotes-deck.search-benchmark/1',synthetic:true,requestTimeLimitMs:60000,timeoutMs:90000,
  cases:Array.from({length:16},(_,i)=>({name:`case-${i}`,data:'data.json',snapshot:'snapshot.json',request:'request.json'}))};
const source = path.join(root,'benchmark.json'); fs.writeFileSync(source,JSON.stringify(manifest));
const output = ci.bundle(source,path.join(root,'bundle'));
const packed = JSON.parse(fs.readFileSync(output,'utf8'));
for (const entry of packed.cases) {
  for (const [key, text] of Object.entries({data,snapshot,request})) {
    const bytes = fs.readFileSync(path.resolve(path.dirname(output),entry[key]));
    assert.equal(bytes.toString('utf8'),text);
  }
}
assert.equal(fs.readdirSync(path.join(root,'bundle','inputs','blobs')).length,3);
assert.throws(()=>ci.bundle(source,path.join(root,'bundle')));
""", directory)

    def test_partial_failures_and_matching_incomplete_answers_never_meet_target(self):
        self.node(r"""
const row = {completion:'Complete',optimality:'proven',searchWallMs:19000,processWallMs:20000,timeLimitMs:60000};
const success = {name:'one',repeat:1,finished:true,native:{passed:true,row},browser:{passed:true,row},
  comparison:{bothComplete:true,compatibleCertificates:true}};
let result = ci.summarizeRuns([success],1,['one']);
assert.equal(result.targetMet,true);
assert.equal(result.requiredCompletionPassed,true);
assert.equal(ci.provenInBudget({...row,searchWallMs:60000.01}),false);
assert.equal(ci.provenIn20sEndToEnd({...row,processWallMs:20000.01}),false);
const lateResponse = structuredClone(success);
lateResponse.browser.row = {...lateResponse.browser.row,processWallMs:21000};
result = ci.summarizeRuns([lateResponse],1,['one']);
assert.equal(result.browserProvenWithinBudget60s,1);
assert.equal(result.browserProvenWithin20sEndToEnd,0);
assert.equal(result.nativeProvenWithin20sEndToEnd,1);
assert.equal(result.requiredCompletionPassed,true);
assert.equal(result.targetMet,false);
const pending = {name:'two',repeat:1,finished:false};
result = ci.summarizeRuns([success,pending],2,['one','two']);
assert.equal(result.contractsPassed,false);
assert.equal(result.targetMet,false);
assert.equal(result.requiredCompletionPassed,false);
const incomplete = structuredClone(success);
incomplete.browser.row.completion='RefinementRequired'; incomplete.browser.row.optimality='unproven';
incomplete.comparison.bothComplete=false;
result = ci.summarizeRuns([incomplete],1,[]);
assert.equal(result.contractsPassed,true);
assert.equal(result.canonicalTopKCompared,0);
assert.equal(result.targetMet,false);
assert.equal(result.requiredCompletionPassed,true);
const failed = {name:'one',repeat:1,finished:true,native:success.native,browser:{passed:false,error:'Worker external deadline'}};
result = ci.summarizeRuns([failed],1,[]);
assert.equal(result.contractsPassed,false);
assert.equal(result.browserValid,0);
assert.deepEqual(result.browserCompletions,{});
""")

    def test_missing_shard_is_counted_and_duplicate_case_coverage_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            self.node(r"""
const fs = require('node:fs'), path = require('node:path');
const root = JSON.parse(fs.readFileSync(0,'utf8'));
const row = {completion:'Complete',optimality:'proven',searchWallMs:10,processWallMs:15,timeLimitMs:60000};
const run = {name:'one',repeat:1,finished:true,native:{passed:true,row},browser:{passed:true,row},
  comparison:{bothComplete:true,compatibleCertificates:true}};
const report = {format:'ournotes-deck.browser-ci/1',sourceCommit:'abc',finished:true,passed:true,
  requiredCompleteCases:['one'],runs:[run],summary:{planned:1}};
const first = path.join(root,'results','browser-results-first'); fs.mkdirSync(first,{recursive:true});
fs.writeFileSync(path.join(first,'report.json'),JSON.stringify(report));
const matrix = JSON.stringify({include:[{name:'first',expectedCases:1},{name:'missing',expectedCases:1}]});
const combined = ci.collect(path.join(root,'results'),path.join(root,'summary'),matrix,'1','abc');
assert.equal(combined.summary.planned,2);
assert.equal(combined.summary.finished,1);
assert.equal(combined.summary.targetMet,false);
assert.equal(combined.passed,false);
const second = path.join(root,'results','browser-results-missing'); fs.mkdirSync(second,{recursive:true});
fs.writeFileSync(path.join(second,'report.json'),JSON.stringify(report));
assert.throws(()=>ci.collect(path.join(root,'results'),path.join(root,'duplicate'),matrix,'1','abc'),/duplicate requests/);
""", directory)


if __name__ == '__main__':
    unittest.main()
