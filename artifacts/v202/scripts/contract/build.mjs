// V2.0.2 UNAPPROVED candidate reverse-topological build: REFUNDING -> SEALED -> OPEN with the pinned SilverScript binary.
//   node scripts/contract/build.mjs /absolute/silverc /absolute/bundle-directory
// Writes a bundle into a NEW directory (fails if it exists): *.sil, artifacts/, pins.json, profile.json, build-report.json.
// Compilation and link consistency only; never installs, tests, signs or broadcasts.
import fs from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import {parseCompiledFrame, COMPILER_COMMIT} from '../core/lib/artifacts.js';
import * as L from '../core/lib/state.js';
import {ORDER, DEPENDENCIES, COMPILER_SHA256, NETWORK_GENESIS, sha256, linkSource, checkTemplate, inspectBundle} from './bundle.mjs';

import {syncLayout} from './sync-layout.mjs';
const VERSION = 'V2.0.2';
const [exe, dest] = process.argv.slice(2);
if (!exe || !dest || process.argv.length !== 4 || !path.isAbsolute(exe) || !path.isAbsolute(dest)) throw Error('Usage: node build.mjs /absolute/silverc /absolute/bundle-directory');
assert.equal(sha256(await fs.readFile(exe)), COMPILER_SHA256, 'Compiler binary does not match the pinned SilverScript build');
const root = fileURLToPath(new URL('../../contracts/', import.meta.url));
await syncLayout(); // source projection consistency only, no automatic regeneration
const layout = await fs.readFile(path.join(root, 'ledger-layout.json'));
await fs.mkdir(dest);
await fs.mkdir(path.join(dest, 'artifacts'));
await fs.writeFile(path.join(dest, 'ledger-layout.json'), layout, {flag: 'wx'});
// Public test point, NOT a wallet key. Constructor values only instantiate the state span for compilation.
const owner = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
const config = {ticketPrice: 100000000n, ticketCap: 3, purchaseCap: 256, minTickets: 3, closeEligibleDaa: 500n};
const frames = {}, framePins = {};
for (const module of ORDER) {
  const template = await fs.readFile(path.join(root, `${module}.sil`), 'utf8');
  checkTemplate(module, template);
  const linked = linkSource(module, template, frames);
  let ledger = L.newOpen(owner, config);
  if (module !== 'open') ledger = {...L.appendPurchase(ledger, 3, owner), phase: module === 'sealed' ? L.Phase.SEALED : L.Phase.REFUNDING};
  const raw = L.encodeLedger(ledger), args = JSON.stringify([{kind: 'bytes', value: Array.from(raw)}]) + '\n';
  const sourceFile = path.join(dest, `artifacts/${module}-linked.sil`), argsFile = path.join(dest, `artifacts/${module}-linked.args.json`);
  const outputFile = path.join(dest, `artifacts/${module}-linked.json`);
  await fs.writeFile(path.join(dest, `${module}.sil`), template, {flag: 'wx'});
  await fs.writeFile(sourceFile, linked, {flag: 'wx'});
  await fs.writeFile(argsFile, args, {flag: 'wx'});
  const log = execFileSync(exe, [sourceFile, '--constructor-args', argsFile, '-o', outputFile], {encoding: 'utf8', timeout: 120000, maxBuffer: 16 * 1024 * 1024});
  await fs.writeFile(path.join(dest, `artifacts/${module}.log`), log, {flag: 'wx'});
  const artifact = await fs.readFile(outputFile), doc = JSON.parse(artifact.toString('utf8'));
  const frame = parseCompiledFrame(module, doc, sha256(linked));
  const name = {open: 'KaswinOpen', sealed: 'KaswinSealed', refunding: 'KaswinRefunding'}[module];
  const compiled = doc.contracts[name].compiled;
  const bytecode = typeof compiled.bytecode === 'string' ? Buffer.from(compiled.bytecode.replace(/^0x/, ''), 'hex') : Buffer.from(compiled.bytecode);
  assert.equal(compiled.state_span.len, L.pushBytes(raw).length, `${module}: compiler state push length mismatch`);
  assert.deepEqual(bytecode, Buffer.from(L.scriptFor(raw, frame.tail)), `${module}: compiler/codec state script mismatch`);
  assert.notEqual(frame.templateHash, '00'.repeat(32), `${module}: zero template hash`);
  frames[module] = frame;
  framePins[module] = {templateSourceSha256: sha256(template), sourceSha256: sha256(linked), artifactSha256: sha256(artifact), constructorArgsSha256: sha256(args),
    templateHash: frame.templateHash, tailBytes: frame.tail.length, dependencies: Object.fromEntries(DEPENDENCIES[module].map(m => [m, frames[m].templateHash]))};
}
// Candidate identity is NOT runtime approval. makeProfile rejects unapproved IDs.
// Preserved V2.0.1 lib supplies the unchanged KW21 constructor codec/hash algorithm, not approval.
const profile = {id: L.profileId(frames)};
const serialized = Object.fromEntries(L.MODULES.map(m => [m, {...frames[m], tail: Buffer.from(frames[m].tail).toString('hex')}]));
const pins = {contractVersion: VERSION, protocolVersion: 2, profileId: profile.id, networkGenesis: NETWORK_GENESIS, compilerCommit: COMPILER_COMMIT,
  compilerBinarySha256: COMPILER_SHA256, layoutSha256: sha256(layout), approvalStatus: 'UNAPPROVED_CANDIDATE', budgetProfileId: null, frames: framePins};
const report = {status: 'V2.0.2_COMPILED_UNAPPROVED_NOT_VM_OR_NETWORK_VERIFIED', contractVersion: VERSION, profileId: profile.id, compilerCommit: COMPILER_COMMIT,
  compilerBinarySha256: COMPILER_SHA256, layoutSha256: sha256(layout), compileOrder: ORDER, frames: serialized, budgetStatus: 'PENDING_VM_CALIBRATION', vm: 'NOT_RUN', network: 'NOT_RUN'};
for (const [name, value] of [['pins.json', pins], ['profile.json', {id: profile.id, contractVersion: VERSION, compilerCommit: COMPILER_COMMIT, frames: serialized}], ['artifacts/build-report.json', report]])
  await fs.writeFile(path.join(dest, name), JSON.stringify(value, null, 2) + '\n', {flag: 'wx'});
// Read back all emitted provenance without calling the runtime approval gate.
await inspectBundle(dest);
// Deliberately do not call the trusted runtime loader or modify APPROVED_PROFILE_ID.
console.log(`${VERSION} UNAPPROVED candidate: ${dest}\nProfile: ${profile.id}\n` + ORDER.map(m => `${m}: tail ${frames[m].tail.length} B, template ${frames[m].templateHash}`).join('\n'));
