// Contract V2.0.1 plugin (Profile 78c11314…): 80B ledger (KW21), in-transaction draw, 4 / 0.2 / 0.02 economics.
// Requires an explicitly pinned LOCAL publication snapshot containing contracts/V2.0.1/ and scripts/V2.0.1/lib/;
// never loads templates/cores from the network. Coexists with the V2 plugin (kaswin-v2.mjs): different plugin id,
// Profile-bound Genesis prefix and registration amount, so neither can claim the other's rounds.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath, pathToFileURL} from 'node:url';
import {createV2RoundAdapter} from './v2-rounds.mjs';
import {wireSpk} from '../src/normalize.mjs';
import {registryScope, registrationOf, DEFAULT_REGISTRY_ADDRESS} from '../src/adapter/registry.mjs';
import {actionOf} from './kaswin-v2.mjs';

const HASH = /^[0-9a-f]{64}$/;
const TN10_GENESIS = 'f896a3034873be1739fc4359236899fd3d65d2bc94f9780df0d0da3eb1cc4370';
export const PROFILE_ID = '78c11314f95531345fb6f8614d4d628e2dbfac3429b7c9e37f4161a3a40907f7';
export const NETWORK_GENESIS = TN10_GENESIS;
export const REGISTRATION_SOMPI = '20000000';          // 0.2 TKAS, fixed by the V2.0.1 upgrade decision (V2 keeps 0.05)
export const CONTRACT_DIR = 'contracts/V2.0.1', CORE_DIR = 'scripts/V2.0.1/lib';
export {actionOf};

const hasBundle = d => fs.existsSync(path.join(d, CONTRACT_DIR, 'pins.json')) && fs.existsSync(path.join(d, CORE_DIR, 'state.js'));
function resolveRepo(repoDir) {
  if (repoDir && typeof repoDir === 'string') return path.resolve(repoDir);
  if (process.env.KASWIN_V201_REPO) return path.resolve(process.env.KASWIN_V201_REPO);
  const candidates = [
    path.resolve(fileURLToPath(new URL('../artifacts/v201', import.meta.url))),
    path.resolve(fileURLToPath(new URL('../../../references/kaswin-v201', import.meta.url))),
    '/opt/kaspa-covenant-indexer/artifacts/v201',
    '/opt/kaswin-event-indexer/references/kaswin-v201',
    '/root/kaswin-publish-20261005'
  ];
  return candidates.find(c => { try { return hasBundle(c); } catch { return false; } }) ?? null;
}
/** sha256 over sorted "name\0sha256(file)\n" of every lib/*.js the plugin can import (transitively the whole core). */
export function coreDigest(repoDir) {
  const dir = path.join(repoDir, CORE_DIR), h = crypto.createHash('sha256');
  for (const name of fs.readdirSync(dir).filter(n => n.endsWith('.js')).sort())
    h.update(`${name}\0${crypto.createHash('sha256').update(fs.readFileSync(path.join(dir, name))).digest('hex')}\n`);
  return h.digest('hex');
}

export async function create(options = {}, {sdk} = {}) {
  const o = {registryAddresses: [DEFAULT_REGISTRY_ADDRESS], registrationSompi: REGISTRATION_SOMPI, ...options, repoDir: resolveRepo(options.repoDir)};
  if (!HASH.test(o.profileId ?? '') || o.profileId === '00'.repeat(32)) throw Error('V201_PROFILE_PIN_REQUIRED');
  if (o.networkGenesis !== TN10_GENESIS) throw Error('V201_TN10_PIN_REQUIRED');
  if (typeof o.repoDir !== 'string' || !path.isAbsolute(o.repoDir) || !hasBundle(o.repoDir)) throw Error('V201_LOCAL_REPOSITORY_REQUIRED');
  if (String(o.registrationSompi) !== REGISTRATION_SOMPI) throw Error('V201_REGISTRATION_AMOUNT_PIN');
  if (!Array.isArray(o.registryAddresses)) throw Error('REGISTRY_ADDRESSES');
  const digest = coreDigest(o.repoDir);
  if (o.coreSha256 !== undefined && o.coreSha256 !== digest) throw Error(`V201_CORE_HASH_MISMATCH expected ${o.coreSha256} got ${digest}`);
  const load = relative => import(pathToFileURL(path.join(o.repoDir, relative)).href);
  // loadBundle re-verifies template/linked sources, artifacts, constructor args, dependency hashes and the Profile id.
  const {loadBundle} = await load(`${CONTRACT_DIR}/tools/bundle.mjs`);
  const {pins, profile} = await loadBundle(path.join(o.repoDir, CONTRACT_DIR));
  if (profile.id !== o.profileId || pins.networkGenesis !== o.networkGenesis || pins.contractVersion !== 'V2.0.1') throw Error('V201_UNPINNED_PROFILE');
  const [S, P, G, accepted, R] = await Promise.all(['state', 'protocol', 'genesis-discovery', 'accepted', 'registry'].map(m => load(`${CORE_DIR}/${m}.js`)));
  if (S.CONTRACT_VERSION !== 'V2.0.1' || S.HEADER !== 80 || String(R.REGISTRATION_SOMPI) !== REGISTRATION_SOMPI) throw Error('V201_CORE_MISMATCH');
  const scope = sdk && o.registryAddresses.length ? registryScope(sdk, o.registryAddresses) : [];
  const registration = (tx, sc, owner) => registrationOf(tx, sc, owner, REGISTRATION_SOMPI);
  const adapter = createV2RoundAdapter({S, G, accepted, profile, networkGenesis: o.networkGenesis, registryScope: scope, registrationOf: registration});
  const watch = new Set(scope.map(r => r.address)), fee = BigInt(REGISTRATION_SOMPI);
  const prefix = Buffer.from('KASWIN_GENESIS_V2').toString('hex') + profile.id;
  // Routing/template check only. The engine establishes selected-chain acceptance and hydrates funding before genesis().
  const inspectGenesis = tx => {
    const payload = tx?.payload, output = tx?.outputs?.[0], binding = output?.covenant;
    if (typeof payload !== 'string' || !/^[0-9a-f]*$/.test(payload)) return {match: false, reason: 'GENESIS_PAYLOAD_FORMAT'};
    if (payload.length !== prefix.length + S.HEADER * 2 || !payload.startsWith(prefix)) return {match: false, reason: 'V201_TAG_PROFILE_OR_LENGTH'};
    if (!binding || binding.authorizingInput !== 0) return {match: false, reason: 'GENESIS_BINDING'};
    try {
      G.verifyGenesisAnnouncement({accepted: true, payload, authorizingOutpoint: tx.inputs?.[0]?.previousOutpoint,
        outputIndex: 0, value: BigInt(output.value), spk: wireSpk(output.scriptPublicKey), covenantId: binding.covenantId,
        authorizingInput: binding.authorizingInput, covenantOutputIndices: tx.outputs.flatMap((v, i) => v.covenant?.covenantId === binding.covenantId ? [i] : [])}, profile);
      return {match: true};
    } catch (e) { return {match: false, reason: String(e?.message ?? e)}; }
  };
  return {id: 'kaswin-v2.0.1', version: profile.id.slice(0, 16), profileId: profile.id, contractVersion: 'V2.0.1', coreSha256: digest,
    watchAddresses: () => [...watch],
    discover: u => u.address && watch.has(u.address) && u.amount === fee && u.outpoint.index === 1 && (!u.covenantId || u.covenantId === S.ZERO) ? u.outpoint.transactionId : null,
    candidateGenesis: tx => typeof tx?.payload === 'string' && tx.payload.startsWith(prefix),
    inspectGenesis, matchesGenesis: tx => inspectGenesis(tx).match,
    genesis: adapter.genesis, advance: adapter.advance, kindOf: actionOf,
    internals: {S, P, G, R, accepted, profile, adapter, repoDir: o.repoDir}};
}
