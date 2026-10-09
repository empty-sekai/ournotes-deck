"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const [bindingPath, corpusPath] = process.argv.slice(2);
if (!bindingPath || !corpusPath) {
  throw new Error("Usage: node replay-rank.cjs <nodejs-binding> <native-corpus.json>");
}
const { ReplaySession } = require(path.resolve(bindingPath));
const corpus = JSON.parse(fs.readFileSync(corpusPath, "utf8"));
const session = new ReplaySession(JSON.stringify(corpus.data));
let completed = 0;
try {
  for (const { request, expected } of corpus.cases) {
    for (const step of [1, 7, 120]) {
      const job = session.startRankAnalysis(JSON.stringify(request));
      try {
        assert.throws(() => job.advance(0));
        assert.throws(() => job.advance(121));
        let status = JSON.parse(job.status());
        let calls = 0;
        while (status.status === "running") {
          assert.equal(status.result, null);
          const previous = status.completedOrders;
          status = JSON.parse(job.advance(step));
          assert.ok(status.completedOrders <= Math.min(120, previous + step));
          assert.ok(++calls <= 120);
        }
        assert.deepEqual(status, expected);
        assert.deepEqual(JSON.parse(job.advance(step)), expected);
        completed++;
      } finally {
        job.free();
      }
    }
  }
  const cancelled = session.startRankAnalysis(JSON.stringify(corpus.cases[0].request));
  assert.equal(JSON.parse(cancelled.advance(1)).status, "running");
  cancelled.free();
} finally {
  session.free();
}

// A job owns its parsed inputs independently of the JavaScript session handle.
const owner = new ReplaySession(JSON.stringify(corpus.data));
const retained = owner.startRankAnalysis(JSON.stringify(corpus.cases[0].request));
owner.free();
try {
  assert.deepEqual(JSON.parse(retained.advance(120)), corpus.cases[0].expected);
} finally {
  retained.free();
}
console.log(`Replay rank WASM: ${completed} native comparisons, cancellation and session ownership passed`);
