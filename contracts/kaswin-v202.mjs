// Contract V2.0.2 plugin (Profile ed0387c9…): 80B ledger (KW21 named layout), in-transaction draw, 4 / 0.2 / 0.02 economics.
// Requires an explicitly pinned LOCAL publication snapshot containing contracts/ and scripts/core/lib/;
// never loads templates/cores from the network.
//
// NETWORK NEUTRAL: the Profile, templates, ABI and CID derivation do not depend on the network (verified in
// artifacts/v202/scripts/core/lib/state.js profileId(), which hashes only sourceSha256/templateHash/dispatchTag).
// The only per-network fact is the discovery registry address, which is injected as a network context.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath, pathToFileURL} from 'node:url';
import {createV2RoundAdapter} from './v2-rounds.mjs';
import {wireSpk} from '../src/normalize.mjs';
import {registryScope, registrationOf} from '../src/adapter/registry.mjs';
import {networkConfig, networkOfGenesis} from '../src/networks.mjs';

const ACTIONS = {0x51: 'BUY', 0x52: 'CLOSE', 0x53: 'DRAW_AND_PAY', 0x54: 'TIMEOUT_REFUND', 0x55: 'REFUND', 81: 'BUY', 82: 'CLOSE', 84: 'DRAW_AND_PAY', 89: 'TIMEOUT_REFUND', 90: 'REFUND'};
export function actionOf(tx) {
  const script = tx?.inputs?.[0]?.signatureScript;
  return typeof script === 'string' && /^[0-9a-f]{2}/i.test(script) ? ACTIONS[parseInt(script.slice(0, 2), 16)] ?? 'UNKNOWN' : 'UNKNOWN';
}

const HASH = /^[0-9a-f]{64}$/;
export const PROFILE_ID = 'ed0387c9e77bb4e0a8f1f4654a326c316d83663f455283ec6a046a095c94856d';
export const CONTRACT_VERSION = 'V2.0.2';
export const REGISTRATION_SOMPI = '20000000';          // 0.2 KAS
/** Default network when the caller does not pin one (TN10). Kept as an export for tooling/tests. */
export const NETWORK_GENESIS = networkConfig('tn10').genesis;
export const CONTRACT_DIR = 'contracts', CORE_DIR = 'scripts/core/lib', BUNDLE_TOOL = 'scripts/contract/bundle.mjs';

const hasBundle = d => fs.existsSync(path.join(d, CONTRACT_DIR, 'pins.json')) && fs.existsSync(path.join(d, CORE_DIR, 'state.js'));
function resolveRepo(repoDir) {
  if (repoDir && typeof repoDir === 'string') return path.resolve(repoDir);
  if (process.env.KASWIN_V202_REPO) return path.resolve(process.env.KASWIN_V202_REPO);
  const candidates = [
    path.resolve(fileURLToPath(new URL('../artifacts/v202', import.meta.url))),
    path.resolve(fileURLToPath(new URL('../../../references/kaswin-v202', import.meta.url))),
    '/opt/kaspa-covenant-indexer/artifacts/v202',
    '/opt/kaswin-event-indexer/references/kaswin-v202',
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

export async function create(options = {}, {sdk, network} = {}) {
  const o = {...options, repoDir: resolveRepo(options.repoDir)};
  if (!HASH.test(o.profileId ?? '') || o.profileId === '00'.repeat(32)) throw Error('V202_PROFILE_PIN_REQUIRED');
  // Network context resolution (fail closed, never falls back to another network):
  //  1. an explicit `network` key/networkId, else 2. the network that owns the pinned genesis.
  const net = network != null ? networkConfig(network)
    : o.networkGenesis != null ? networkConfig(networkOfGenesis(o.networkGenesis) ?? o.networkGenesis)
    : networkConfig('tn10');
  o.networkGenesis = net.genesis;
  o.network = net.key;
  if (o.registryAddresses === undefined) o.registryAddresses = [net.registry];
  o.registrationSompi = o.registrationSompi ?? REGISTRATION_SOMPI;
  if (typeof o.repoDir !== 'string' || !path.isAbsolute(o.repoDir) || !hasBundle(o.repoDir)) throw Error('V202_LOCAL_REPOSITORY_REQUIRED');
  if (String(o.registrationSompi) !== REGISTRATION_SOMPI) throw Error('V202_REGISTRATION_AMOUNT_PIN');
  if (!Array.isArray(o.registryAddresses)) throw Error('REGISTRY_ADDRESSES');
  const digest = coreDigest(o.repoDir);
  if (o.coreSha256 !== undefined && o.coreSha256 !== digest) throw Error(`V202_CORE_HASH_MISMATCH expected ${o.coreSha256} got ${digest}`);
  const load = relative => import(pathToFileURL(path.join(o.repoDir, relative)).href);
  // loadBundle re-verifies template/linked sources, artifacts, constructor args, dependency hashes and the Profile id.
  const {loadBundle} = await load(BUNDLE_TOOL);
  const {pins, profile} = await loadBundle(path.join(o.repoDir, CONTRACT_DIR));
  if (profile.id !== o.profileId || pins.contractVersion !== CONTRACT_VERSION) throw Error('V202_UNPINNED_PROFILE');
  const [S, P, G, accepted, R] = await Promise.all(['state', 'protocol', 'genesis-discovery', 'accepted', 'registry'].map(m => load(`${CORE_DIR}/${m}.js`)));
  const headerBytes = S.HEADER_BYTES ?? S.HEADER ?? 80;
  if (S.CONTRACT_VERSION !== CONTRACT_VERSION || headerBytes !== 80 || String(R.REGISTRATION_SOMPI) !== REGISTRATION_SOMPI) throw Error('V202_CORE_MISMATCH');
  // Pure-JS registry scope: no SDK required (the address codec is local). The network's prefix is enforced.
  const scope = o.registryAddresses.length ? registryScope(sdk, o.registryAddresses, o.network) : [];
  // Bind the V2.0.2 registration amount (0.2 KAS). registry.mjs defaults to the legacy 0.05, so the amount MUST be
  // pinned here rather than relying on the adapter's 3-argument call.
  const registration = (tx, sc, owner) => registrationOf(tx, sc, owner, REGISTRATION_SOMPI);
  const adapter = createV2RoundAdapter({S, G, accepted, profile, networkGenesis: o.networkGenesis, registryScope: scope, registrationOf: registration});
  const watch = new Set(scope.map(r => r.address)), fee = BigInt(REGISTRATION_SOMPI);
  const prefix = Buffer.from('KASWIN_GENESIS_V2').toString('hex') + profile.id;
  // Routing/template check only. The engine establishes selected-chain acceptance and hydrates funding before genesis().
  const inspectGenesis = tx => {
    const payload = tx?.payload, output = tx?.outputs?.[0], binding = output?.covenant;
    if (typeof payload !== 'string' || !/^[0-9a-f]*$/.test(payload)) return {match: false, reason: 'GENESIS_PAYLOAD_FORMAT'};
    if (payload.length !== prefix.length + headerBytes * 2 || !payload.startsWith(prefix)) return {match: false, reason: 'V202_TAG_PROFILE_OR_LENGTH'};
    if (!binding || binding.authorizingInput !== 0) return {match: false, reason: 'GENESIS_BINDING'};
    try {
      G.verifyGenesisAnnouncement({accepted: true, payload, authorizingOutpoint: tx.inputs?.[0]?.previousOutpoint,
        outputIndex: 0, value: BigInt(output.value), spk: wireSpk(output.scriptPublicKey), covenantId: binding.covenantId,
        authorizingInput: binding.authorizingInput, covenantOutputIndices: tx.outputs.flatMap((v, i) => v.covenant?.covenantId === binding.covenantId ? [i] : [])}, profile);
      return {match: true};
    } catch (e) { return {match: false, reason: String(e?.message ?? e)}; }
  };
  return {id: 'kaswin-v2.0.2', version: profile.id.slice(0, 16), profileId: profile.id, contractVersion: CONTRACT_VERSION,
    network: o.network, networkGenesis: o.networkGenesis, coreSha256: digest,
    watchAddresses: () => [...watch],
    discover: u => u.address && watch.has(u.address) && u.amount === fee && u.outpoint.index === 1 && (!u.covenantId || u.covenantId === S.ZERO) ? u.outpoint.transactionId : null,
    candidateGenesis: tx => typeof tx?.payload === 'string' && tx.payload.startsWith(prefix),
    inspectGenesis, matchesGenesis: tx => inspectGenesis(tx).match,
    genesis: adapter.genesis, advance: adapter.advance, kindOf: actionOf,
    internals: {S, P, G, R, accepted, profile, adapter, repoDir: o.repoDir}};
}
