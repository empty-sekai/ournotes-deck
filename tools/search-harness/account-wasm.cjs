#!/usr/bin/env node
// Compare the actual account transport against an explicitly synthetic native corpus.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { createHash } = require('node:crypto');
const { performance } = require('node:perf_hooks');
const { projection } = require('./json-tokens.cjs');

function validateCorpus(read) {
  const names = JSON.parse(read('cases.json'));
  const coverage = JSON.parse(read('coverage.json')).cases;
  assert.equal(names.length, 76, 'complete synthetic transport corpus');
  assert.equal(new Set(names).size, names.length, 'unique case identities');
  assert.equal(coverage.length, names.length, 'complete coverage manifest');
  assert.equal(new Set(coverage.map(row => row.name)).size, coverage.length, 'unique coverage identities');
  assert.deepEqual(coverage.map(row => row.name).sort(), [...names].sort(), 'coverage names match case identities');
  const coverageByName = new Map(coverage.map(row => [row.name, row]));
  const scenes = {
    power: { kind: 'power' }, free: { kind: 'freeLive', musicId: 10 },
    mission: { kind: 'missionLive', musicId: 10 }, battle: { kind: 'battleLive', musicId: 10 },
    arena: { kind: 'arenaLive', arenaMusicId: 80 }, challenge: { kind: 'challengeLive', challengeMusicId: 70 },
    skip: { kind: 'skip', musicId: 10 }, challengeSkip: { kind: 'skip', challengeMusicId: 70 },
    freeLive: { kind: 'freeLive', musicId: 10 }, missionLive: { kind: 'missionLive', musicId: 10 },
  };
  const requests = new Map();
  const uniqueRequests = new Map();
  let expected = 0, maximum = 0;
  for (const name of names) {
    const raw = read(name + '.request.json');
    const value = JSON.parse(raw);
    const fields = projection(raw);
    const aggregation = value.aggregation ?? 'expected';
    assert(['expected', 'maximum'].includes(aggregation), `${name}: declared aggregation`);
    const row = coverageByName.get(name);
    assert.equal(row.scope, 'synthetic', `${name}: coverage scope`);
    assert.equal(row.aggregation ?? 'expected', aggregation, `${name}: coverage aggregation`);
    assert.equal(row.metric, value.goal.kind === 'power' ? 'power' : value.metric?.kind ?? 'score', `${name}: coverage metric`);
    const goal = scenes[row.scene];
    assert(goal, `${name}: supported coverage scene`);
    for (const [field, required] of Object.entries(goal)) assert.equal(value.goal[field], required, `${name}: coverage goal.${field}`);
    if (row.play === 'accuracy') assert(value.goal.accuracy, `${name}: coverage accuracy play`);
    else if (row.play) assert.equal(value.goal.play?.kind, row.play, `${name}: coverage play`);
    const condition = fields.filter(([key]) => key !== 'aggregation');
    const identity = JSON.stringify([...condition, ['aggregation', ['string', aggregation]]].sort(([a], [b]) => a.localeCompare(b)));
    assert(!uniqueRequests.has(identity), `${name}: duplicate request semantics of ${uniqueRequests.get(identity)}`);
    uniqueRequests.set(identity, name);
    requests.set(name, { raw, value, aggregation, condition });
    aggregation === 'maximum' ? maximum++ : expected++;
  }
  assert.equal(expected, 49, 'complete Expected coverage');
  assert.equal(maximum, 27, 'complete Maximum coverage');
  const gekisou = new Set(['missionLive', 'battleLive', 'arenaLive']);
  for (const [name, request] of requests) {
    if (request.aggregation === 'maximum') {
      assert(name.endsWith('-maximum'), `${name}: Maximum counterpart identity`);
      const base = requests.get(name.slice(0, -'-maximum'.length));
      assert(base && base.aggregation === 'expected', `${name}: Expected counterpart exists`);
      assert(!gekisou.has(request.value.goal.kind), `${name}: Maximum mode is admitted`);
      assert.deepEqual(request.condition, base.condition, `${name}: only aggregation differs from Expected counterpart`);
    } else {
      assert.equal(requests.has(name + '-maximum'), !gekisou.has(request.value.goal.kind), `${name}: exact Maximum counterpart coverage`);
    }
  }
  return { names, requests };
}

function fieldAt(fields, keys) {
  let node = ['object', fields];
  for (const key of keys) {
    node = node[0] === 'array' ? node[1][key] : node[1].find(([field]) => field === key)?.[1];
    assert(node, `missing field at ${keys.join('.')}`);
  }
  return node;
}

// Compare integer fields from their original JSON tokens, including payoffs outside the safe Number range.
function integerAt(fields, keys) {
  const node = fieldAt(fields, keys);
  assert.equal(node[0], 'number', `numeric integer at ${keys.join('.')}`);
  assert(/^-?\d+$/.test(node[1]), `integer token at ${keys.join('.')}`);
  return BigInt(node[1]);
}

const [pkg, directory, reportPath] = process.argv.slice(2);
if (!pkg || !directory) throw new Error('usage: node account-wasm.cjs PACKAGE_JS CORPUS_DIRECTORY [REPORT_JSON]');
const { DeckSolver } = require(path.resolve(pkg));
const root = path.resolve(directory);
const read = name => fs.readFileSync(path.join(root, name), 'utf8');
const bytes = fs.readFileSync(path.join(root, 'data.json'));
const datasetId = createHash('sha256').update(bytes).digest('hex');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const solver = new DeckSolver(new Uint8Array(bytes));
assert.equal(solver.datasetId, datasetId);
const textSolver = new DeckSolver(bytes.toString('utf8'));
assert.equal(textSolver.datasetId, datasetId);
textSolver.free();
// A view must hash only its own bytes, not the surrounding ArrayBuffer.
const padded = Buffer.concat([Buffer.from([0xff]), bytes, Buffer.from([0xff])]);
const viewSolver = new DeckSolver(new Uint8Array(padded.buffer, padded.byteOffset + 1, bytes.length));
assert.equal(viewSolver.datasetId, datasetId);
viewSolver.free();
assert.throws(() => new DeckSolver(new Uint8Array([0xff])), /UTF-8/);
assert.throws(() => new DeckSolver(JSON.parse(bytes.toString('utf8'))), /Uint8Array.*JSON text/);
const capabilities = JSON.parse(solver.capabilities());
assert.equal(capabilities.accountFormat, 'ournotes.account/1');
assert.equal(capabilities.requestFormat, 'ournotes-deck.recommendation-request/2');
assert.equal(capabilities.answerFormat, 'ournotes-deck.account-recommendation/1');
assert.equal(capabilities.support.freeLive.score, 'proven');
assert.equal(capabilities.support.battleLive.score, 'proven');
assert.equal(capabilities.support.freeLive.challengePoints, 'proven');
assert.equal(capabilities.eventMusicRanking.goal, 'challengeLive');
assert.equal(capabilities.eventMusicRanking.metric, 'score');
assert.equal(capabilities.luckMissions, true);
assert.equal(capabilities.support.freeLive.scoreAndLife, 'proven');
assert.equal(capabilities.scoreAndLife.requiresCompleteJudgementStream, true);
assert.equal(capabilities.skip.musicIdOrChallengeMusicId, true);
assert.equal(capabilities.defaultAggregation, 'expected');
assert.deepEqual(capabilities.aggregations.expected, capabilities.metrics);
const maximumGoals = ['power', 'freeLive', 'challengeLive', 'skip'];
assert.deepEqual(Object.keys(capabilities.aggregations.maximum).sort(), maximumGoals.sort());
for (const goal of maximumGoals) assert.deepEqual(capabilities.aggregations.maximum[goal], capabilities.metrics[goal]);
const liveMaximumModel = {
  kind: 'independentNativeDrawSupport', performanceOrders: 120,
  ordinarySkillDraw: 'binary32OfSystemRandomNextDouble', ordinarySkillComparison: 'strictLessThan',
  rootSeedRealizability: 'notEstablished', bestOrderCertificate: 'performanceOrderAndTerminalValuesOnly',
  streamBaseSeedRole: 'notAConstraint',
};
const deterministicMaximumModel = {
  kind: 'deterministic', performanceOrders: 1,
  rootSeedRealizability: 'notApplicable', bestOrderCertificate: 'notApplicable',
};
assert.deepEqual(capabilities.maximumModel, liveMaximumModel);
assert.deepEqual(capabilities.maximumModels, {
  power: deterministicMaximumModel, freeLive: liveMaximumModel,
  challengeLive: liveMaximumModel, skip: deterministicMaximumModel,
});
const assertModel = (result, request, aggregation) => {
  const model = aggregation === 'maximum'
    ? (['power', 'skip'].includes(request.goal.kind) ? deterministicMaximumModel : liveMaximumModel)
    : undefined;
  assert.deepEqual(result.maximumModel, model);
};
const privateValues = ['PRIVATE_NAME_SENTINEL', '9007199254740993', '9223372036854775806',
  '9007199254740992', '9223372036854776000', '"_name"', '"_accountid"', '"_profileId"'];
const assertPrivate = raw => {
  for (const value of privateValues) assert(!raw.includes(value), `private field leaked: ${value}`);
};
const account = read('account.json');
let reports = 0;
const { names, requests } = validateCorpus(read);
let expectedCases = 0, maximumCases = 0;
const records = [];
for (const name of names) {
  const { raw: request, value: requestValue, aggregation } = requests.get(name);
  const expectedRaw = read(name + '.expected.json');
  const expected = JSON.parse(expectedRaw);
  const expectedProjection = projection(expectedRaw);
  aggregation === 'maximum' ? maximumCases++ : expectedCases++;
  assert.equal(expected.status, 'ok', `${name}: native status`);
  assert.equal(expected.final, true, `${name}: native final`);
  assert.equal(expected.result.optimality.proven, true, `${name}: native proof`);
  assert.equal(expected.result.exitReason, 'exhausted', `${name}: native exhausted`);
  assert.equal(expected.result.teams.length, 5, `${name}: native K=5`);
  assertModel(expected.result, requestValue, aggregation);
  let caseReports = 0;
  let progressFailure;
  const started = performance.now();
  const raw = solver.recommend(account, request, raw => {
    reports++;
    caseReports++;
    // The transport deliberately ignores callback exceptions. Retain any
    // assertion failure and rethrow after the synchronous call has returned.
    try {
      const answer = JSON.parse(raw);
      assert.equal(answer.format, 'ournotes-deck.account-recommendation/1');
      assert.equal(answer.datasetId, datasetId);
      assert.equal(answer.final, false);
      assert.equal(answer.result.optimality.proven, false);
      assert.equal(answer.result.aggregation, aggregation);
      assert.deepEqual(fieldAt(projection(raw), ['result', 'goal']),
        fieldAt(expectedProjection, ['result', 'goal']), `${name}: progress resolved goal`);
      assertModel(answer.result, requestValue, aggregation);
      assert(answer.result.teams.every(team => team.orders === null));
      assertPrivate(raw);
    } catch (error) { progressFailure ??= error; }
  }, 0);
  const recommendWallMs = performance.now() - started;
  if (progressFailure) throw progressFailure;
  assertPrivate(raw);
  const actual = JSON.parse(raw);
  const actualProjection = projection(raw);
  assert.equal(actual.status, 'ok');
  assert.equal(actual.final, true);
  assert.equal(actual.result.optimality.proven, true);
  assert.equal(actual.result.exitReason, 'exhausted', `${name}: WASM exhausted`);
  assert.equal(actual.result.phase, 'done');
  assert.equal(actual.result.aggregation, aggregation);
  assertModel(actual.result, requestValue, aggregation);
  assert.equal(actual.result.teams.length, 5, `${name}: complete K=5`);
  assert(actual.result.teams.some(team => team.layout.snaps.some(snap => snap !== null)), `${name}: nonempty Snap`);
  if (['freeLive', 'challengeLive'].includes(requestValue.goal.kind)) {
    assert(caseReports > 0, `${name}: progress reports`);
    for (const [index, team] of actual.result.teams.entries()) {
      if (aggregation === 'maximum') {
        assert.equal(team.orders, null);
        assert.equal(BigInt(team.value.exact.denominator), 1n);
        assert.equal(BigInt(team.value.exact.numerator), BigInt(team.value.score));
        assert.deepEqual([...team.bestOrder.order].sort((a, b) => a - b), [...team.layout.members].sort((a, b) => a - b));
        const prefix = ['result', 'teams', index];
        const score = integerAt(actualProjection, [...prefix, 'value', 'score']);
        const bestScore = integerAt(actualProjection, [...prefix, 'bestOrder', 'score']);
        assert(bestScore <= score, `${name}: bestOrder score cannot exceed maximum score`);
        if ((requestValue.metric?.kind ?? 'score') === 'score') {
          assert.equal(bestScore, score, `${name}: score-optimal order attains maximum score`);
          assert.equal(team.value.payoff, null, `${name}: score metric has no separate payoff`);
          assert.equal(team.bestOrder.payoff, null, `${name}: score metric order has no separate payoff`);
        } else {
          assert(team.value.payoff, `${name}: selected payoff is present`);
          assert.equal(BigInt(team.value.payoff.exact.denominator), 1n, `${name}: exact maximum payoff denominator`);
          assert.equal(integerAt(actualProjection, [...prefix, 'bestOrder', 'payoff']),
            BigInt(team.value.payoff.exact.numerator), `${name}: selected order attains maximum payoff`);
        }
        continue;
      }
      assert.equal(team.orders.count, 120);
      assert.equal(team.orders.values.length, 120);
      assert.equal(team.orders.values.reduce((sum, value) => sum + BigInt(value), 0n), BigInt(team.value.exact.numerator));
      assert.equal(BigInt(team.value.exact.denominator), 120n);
      if (requestValue.metric?.kind === 'challengePoints') {
        assert.equal(team.orders.payoffValues.length, 120);
        assert.equal(team.orders.payoffValues.reduce((sum, value) => sum + BigInt(value), 0n), BigInt(team.value.payoff.exact.numerator));
        assert.equal(BigInt(team.value.payoff.exact.denominator), 120n);
        assert.equal(actual.result.metric.rewardProjection, true);
      }
    }
  }
  assert.deepEqual(actualProjection, expectedProjection, `${name}: exact native/WASM semantics`);
  records.push({ name, aggregation, requestSha256: hash(request), referenceSha256: hash(expectedRaw),
    answerSha256: hash(raw), semanticSha256: hash(JSON.stringify(actualProjection)),
    recommendWallMs, progressReports: caseReports,
    status: actual.status, exitReason: actual.result.exitReason, proven: actual.result.optimality.proven,
    teams: actual.result.teams.length });
}
assert.equal(expectedCases, 49);
assert.equal(maximumCases, 27);
assert(reports > 0);
for (const name of ['mission-score', 'battle-score', 'arena-score']) {
  const request = JSON.parse(read(name + '.request.json'));
  request.aggregation = 'maximum';
  let rejectedProgress = 0;
  const answer = JSON.parse(solver.recommend(account, JSON.stringify(request), () => rejectedProgress++, 0));
  assert.equal(answer.status, 'failed', `${name}: unsupported Maximum`);
  assert.equal(answer.final, true);
  assert.equal(answer.result, null);
  assert(answer.errors.some(issue => issue.path === 'aggregation' && issue.code === 'unsupported'));
  assert.equal(rejectedProgress, 0, `${name}: no unsupported search`);
}
for (const [name, change, issuePath] of [
  ['life-stream', q => { delete q.goal.play; }, 'goal.play'],
  ['life-stream', q => { q.goal.accuracy = { greatFraction: 0.1 }; }, 'goal.accuracy'],
  ['life-stream', q => { q.goal.play.stream.judged.pop(); }, 'goal.play.stream'],
  ['challenge-skip-points', q => { q.goal.musicId = 10; }, 'goal.challengeMusicId'],
  ['challenge-skip-items', q => { delete q.eventContext.selectedRewards; }, 'eventContext.selectedRewards'],
  ['free-maximum', q => { q.aggregation = 'median'; }, 'aggregation'],
]) {
  const request = JSON.parse(read(name + '.request.json'));
  change(request);
  const answer = JSON.parse(solver.recommend(account, JSON.stringify(request)));
  assert.equal(answer.status, 'invalid');
  assert.equal(answer.result, null);
  assert(answer.errors.some(issue => issue.path === issuePath), issuePath);
}
for (const [keys, issuePath, requestName] of [
  [['declared'], 'declared._vip._rank', 'power'],
  [['account', '_player', '_memberCards', 0, '_exp'], '_player._memberCards[0]._exp', 'power'],
  [['account', '_player', '_memberCards', 0, '_awakeCount'], '_player._memberCards[0]._awakeCount', 'power'],
  [['account', '_player', '_memberCards', 0, '_rank'], '_player._memberCards[0]._rank', 'power'],
  [['account', '_player', '_memberCards', 0, '_liveSkillLevel'], '_player._memberCards[0]._liveSkillLevel', 'free'],
  [['account', '_player', '_supportCards', 0, '_exp'], '_player._supportCards[0]._exp', 'power'],
  [['account', '_player', '_supportCards', 0, '_rank'], '_player._supportCards[0]._rank', 'power'],
]) {
  for (const absent of [true, false]) {
    const input = JSON.parse(account);
    const parent = keys.slice(0, -1).reduce((value, key) => value[key], input);
    if (absent) delete parent[keys.at(-1)]; else parent[keys.at(-1)] = null;
    const raw = solver.recommend(JSON.stringify(input), read(requestName + '.request.json'));
    assertPrivate(raw);
    const answer = JSON.parse(raw);
    assert.equal(answer.status, 'incomplete', `${issuePath} absent=${absent}`);
    assert.equal(answer.final, true);
    assert.equal(answer.result, null);
    assert.deepEqual(answer.errors, []);
    assert(answer.missing.some(issue => issue.path === issuePath && issue.code === 'missing'));
  }
}
for (const field of ['format', 'datasetId', 'server', 'revision', 'coverage', 'assumptions', 'account']) {
  const input = JSON.parse(account);
  delete input[field];
  const answer = JSON.parse(solver.recommend(JSON.stringify(input), read('power.request.json')));
  assert.equal(answer.status, 'invalid', `missing envelope ${field}`);
  assert.equal(answer.result, null);
  assert(answer.errors.some(issue => issue.path === 'account' && issue.code === 'parse'));
}
// The UTF-8 BOM participates in the identity even though the JSON parser accepts it.
const bomBytes = Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), bytes]);
const bomId = createHash('sha256').update(bomBytes).digest('hex');
assert.notEqual(bomId, datasetId);
for (const input of [new Uint8Array(bomBytes), bomBytes.toString('utf8')]) {
  const bomSolver = new DeckSolver(input);
  assert.equal(bomSolver.datasetId, bomId);
  const mismatch = JSON.parse(bomSolver.recommend(account, read('power.request.json')));
  assert.equal(mismatch.status, 'invalid');
  assert(mismatch.errors.some(issue => issue.path === 'datasetId' && issue.code === 'dataset_mismatch'));
  const matchingAccount = account.replace(datasetId, bomId);
  const raw = bomSolver.recommend(matchingAccount, read('power.request.json'));
  assertPrivate(raw);
  const answer = JSON.parse(raw);
  assert.equal(answer.status, 'ok');
  assert.equal(answer.datasetId, bomId);
  assert.equal(answer.result.teams.length, 5);
  assert.deepEqual(answer.result.teams, JSON.parse(read('power.expected.json')).result.teams);
  bomSolver.free();
}
solver.free();
const report = { cases: names.length, expectedCases, maximumCases, status: 'passed', progressReports: reports,
  runtime: process.version, datasetId, accountSha256: hash(account), casesSha256: hash(read('cases.json')),
  coverageSha256: hash(read('coverage.json')),
  runnerSha256: hash(fs.readFileSync(__filename)), projectionSha256: hash(fs.readFileSync(require.resolve('./json-tokens.cjs'))),
  glueSha256: hash(fs.readFileSync(path.resolve(pkg))),
  wasmSha256: hash(fs.readFileSync(path.resolve(pkg).replace(/\.js$/, '_bg.wasm'))), records };
if (reportPath) fs.writeFileSync(reportPath, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ cases: names.length, expectedCases, maximumCases, status: 'passed', progressReports: reports }));
