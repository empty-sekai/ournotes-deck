// Numeric cross-architecture equivalence is portability evidence, not ARM64 game proof.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
const [wasmFile, nativeExe, inputDirectory, reportFile] = process.argv.slice(2);
const { instance } = await WebAssembly.instantiate(fs.readFileSync(wasmFile), {});
const cases = [];
for (const file of fs.readdirSync(inputDirectory).filter(f => f.endsWith('.json')).sort()) {
    const inputPath = path.join(inputDirectory, file);
    const bytes = fs.readFileSync(inputPath);
    const ptr = instance.exports.alloc(bytes.length);
    new Uint8Array(instance.exports.memory.buffer, ptr, bytes.length).set(bytes);
    const packed = instance.exports.evaluate(ptr, bytes.length);
    const outPtr = Number(packed & 0xffffffffn), outLength = Number(packed >> 32n);
    const actual = JSON.parse(Buffer.from(new Uint8Array(instance.exports.memory.buffer, outPtr, outLength)).toString());
    instance.exports.release(ptr, bytes.length);
    instance.exports.release(outPtr, outLength);
    const expected = JSON.parse(nativeExe === '-'
        ? fs.readFileSync(path.join(inputDirectory, '..', `${file}.x64-output.json`), 'utf8')
        : execFileSync(nativeExe, [inputPath], { maxBuffer: 20 * 1024 * 1024 }).toString());
    assert.ok(!actual.error, `${file}: ${actual.error}`);
    assert.deepEqual(actual, expected, `numeric mismatch: ${file}`);
    fs.writeFileSync(path.join(inputDirectory, '..', `${file}.x64-output.json`), JSON.stringify(expected));
    fs.writeFileSync(path.join(inputDirectory, '..', `${file}.wasm-output.json`), JSON.stringify(actual));
    cases.push({ file, atoms: actual.atoms.length, scores: actual.atoms.map(a => a.score), exact: true });
}
fs.writeFileSync(reportFile, JSON.stringify({ format: 'ournotes-deck.numeric-portability/1', passed: true,
    architectures: ['x86_64-pc-windows-msvc', 'wasm32-unknown-unknown'],
    evaluator: 'production recommendation::evaluate_declared_context', cases,
    scope: 'Same source numeric outputs/entire score traces/RNG draws/Gekisou ranges; not native ARM64 game proof or WASM search performance' }, null, 2));
console.log(`${cases.length} cases exact on x64 and WASM`);
