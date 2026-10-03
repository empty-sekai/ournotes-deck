// Semantic projection of a solver JSON answer that keeps every number token as written, so integers above
// JavaScript's safe range never compare equal by rounding. Object keys are sorted; `telemetry` and `elapsedMs` are
// dropped at the top level and inside `result` (an owned-snapshot answer), since they hold diagnostics and time.
const assert = require('node:assert/strict');

function projection(text) {
  JSON.parse(text); // Syntax only. The recursive comparison keeps number tokens.
  let i = 0;
  const space = () => { while (/\s/.test(text[i] || '')) i++; };
  const string = () => {
    const start = i++;
    let escaped = false;
    while (i < text.length) {
      const c = text[i++];
      if (escaped) escaped = false;
      else if (c === '\\') escaped = true;
      else if (c === '"') return JSON.parse(text.slice(start, i));
    }
    throw Error('unterminated string');
  };
  const value = () => {
    space();
    if (text[i] === '"') return ['string', string()];
    if (text[i] === '{') {
      i++; space(); const fields = [];
      while (text[i] !== '}') {
        const key = string(); space(); assert.equal(text[i++], ':');
        fields.push([key, value()]); space();
        if (text[i] !== ',') break;
        i++; space();
      }
      assert.equal(text[i++], '}');
      return ['object', fields.sort(([a], [b]) => a.localeCompare(b))];
    }
    if (text[i] === '[') {
      i++; space(); const items = [];
      while (text[i] !== ']') {
        items.push(value()); space();
        if (text[i] !== ',') break;
        i++; space();
      }
      assert.equal(text[i++], ']'); return ['array', items];
    }
    const start = i;
    while (i < text.length && !/[\s,\]}]/.test(text[i])) i++;
    const token = text.slice(start, i);
    return [/^(true|false|null)$/.test(token) ? 'literal' : 'number', token];
  };
  const strip = fields => fields.filter(([key]) => key !== 'telemetry' && key !== 'elapsedMs')
    .map(([key, v]) => (key === 'result' && v[0] === 'object' ? [key, ['object', strip(v[1])]] : [key, v]));
  const root = value(); assert.equal(root[0], 'object');
  return strip(root[1]);
}
assert.notDeepEqual(projection('{"n":9007199254740992}'), projection('{"n":9007199254740993}'));
assert.deepEqual(projection('{"a":[{"s":"a,}\\\"","n":1}],"telemetry":{},"elapsedMs":1}'),
  projection('{ "a": [ { "n": 1, "s": "a,}\\\"" } ] }'));
assert.deepEqual(projection('{"result":{"telemetry":{"nodes":1},"elapsedMs":2,"n":3}}'), projection('{"result":{"n":3}}'));

module.exports = { projection };
