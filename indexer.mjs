#!/usr/bin/env node
// Covenant event indexer daemon (contract rules from contracts/*.json plugins). Read-only: connects via kaspa-resolver,
// never signs or submits.
//
//   node indexer.mjs run [--networks tn10,mainnet] [--db PATH] [--pool] [--api-port 8788] [--relay URL]
//   node indexer.mjs run --url wss://.. | --urls PRIMARY,STANDBY | --resolver URL,..   (single network, explicit nodes)
//   node indexer.mjs track GENESIS_TXID START_CHAIN_HASH [--network tn10] [--db PATH]
//   node indexer.mjs contracts [--contracts FILE] [--network tn10]    (load + print, no network)
//   node indexer.mjs status [--db PATH]
//
// MULTI-NETWORK: one process runs ONE engine per network, each with its own dual-node pool (primary + hot standby),
// so `--networks tn10,mainnet` opens 4 wRPC connections. Each network keeps its own SQLite file (derived from --db)
// and its own contract registry. Networks never share state: measured 2026-10-16, the same x-only key yields the SAME
// scriptPublicKey on both networks, so a shared cache would silently look valid.
//
// --pool: the two members of a network are chosen at start from that network's seed + kaspa-resolver answers
// (healthy, different backends by p2pId, lowest RTT); a member that keeps failing is replaced from a fresh probe.
import {parseArgs} from 'node:util';
import path from 'node:path';
import {Store} from './src/store.mjs';
import {Engine} from './src/engine.mjs';
import {loadContracts} from './src/contracts.mjs';
import {openNode, addressOfFactory} from './src/node.mjs';
import {openDualNode} from './src/dual-node.mjs';
import {seedFor, RESOLVER_HOSTS, probePool, choosePair, chooseReplacement, discoverFromResolvers} from './src/node-pool.mjs';
import {parseNetworks, DEFAULT_NETWORK} from './src/networks.mjs';
import {makeRelay, flushOutbox} from './src/relay.mjs';
import {createApi} from './src/api.mjs';
import {runtimeOptions} from './src/config.mjs';

const {values: cli, positionals: [cmd = 'run', ...rest]} = parseArgs({allowPositionals: true, options: {
  db: {type: 'string', default: 'kaswin-events.sqlite'}, url: {type: 'string'}, urls: {type: 'string'}, resolver: {type: 'string'},
  pool: {type: 'boolean', default: false}, 'pool-urls': {type: 'string'},
  networks: {type: 'string'}, network: {type: 'string'},
  relay: {type: 'string'}, 'relay-file': {type: 'string'},
  contracts: {type: 'string', default: new URL('./contracts/v202.json', import.meta.url).pathname}, contract: {type: 'string'},
  'legacy-contract': {type: 'string'}, 'registry-addresses': {type: 'string'},
  'admin-token': {type: 'string'},
  'api-port': {type: 'string'}, 'api-host': {type: 'string', default: '127.0.0.1'}, 'cors-origin': {type: 'string', default: '*'},
  'checkpoint-ms': {type: 'string', default: '3000'}, 'reconcile-ms': {type: 'string', default: '60000'},
  'finality-blue-score': {type: 'string', default: '600'},
  'rest-url': {type: 'string'},
}});
const o = runtimeOptions(cli);
const log = (kind, ...a) => console.log(JSON.stringify({at: new Date().toISOString(), kind, detail: a.length === 1 ? a[0] : a}, (_, v) => typeof v === 'bigint' ? v.toString() : v));
if (o['api-port'] && (!/^[0-9]+$/.test(o['api-port']) || Number(o['api-port']) < 1 || Number(o['api-port']) > 65535)) throw Error('INVALID_API_PORT');

/** Per-network DB file: `<base>` for the only network, `<base>.<key>` when several are monitored in one process. */
function dbPathFor(base, net, multi) {
  if (!multi) return base;
  const ext = path.extname(base), stem = ext ? base.slice(0, -ext.length) : base;
  return `${stem}.${net.key}${ext}`;
}

/** Open the node set (1 or 2 connections) of ONE network and return {node, engine, store, contracts, network}. */
async function openNetwork(net, {multi}) {
  const network = net.networkId;
  const dbPath = dbPathFor(o.db, net, multi);
  const store = new Store(dbPath, {defaultContract: o['legacy-contract'] ?? null, network: net.key});
  const contracts = await loadContracts(o.contracts, {
    network: net.key,
    networkGenesis: net.genesis,
    registryAddresses: o.registryAddresses ? o.registryAddresses.split(',').map(s => s.trim()) : undefined,
  });
  const openMember = url => openNode({network, url, timeoutMs: 6000, connectDeadlineMs: 8000});
  const probe = async url => {
    const n = await openMember(url);
    try {
      const rtt = [];
      let blue = 0n;
      for (let i = 0; i < 3; i++) { const t = performance.now(); blue = BigInt((await n.getSinkBlueScore()).blueScore); rtt.push(performance.now() - t); }
      return {p2pId: n.identity.p2pId, blueScore: blue, rttMs: Math.round(rtt.sort((a, b) => a - b)[1])};
    } finally { try { await n.close(); } catch {} }
  };
  const extra = o['pool-urls'] ? o['pool-urls'].split(',').map(s => s.trim()).filter(u => u.includes('/' + net.resolverPath + '/')) : [];
  // Operator-managed pool nodes (added from the Admin UI, persisted in this network's DB). Probed like any other
  // candidate: being listed is not trust, it only means "consider this URL".
  const managed = () => store.poolNodes().map(n => n.url).filter(u => typeof u === 'string' && /^wss?:\/\//i.test(u));
  const candidates = async () => {
    const found = await discoverFromResolvers(RESOLVER_HOSTS, {network: net.key, rounds: 2});
    return [...new Set([...managed(), ...extra, ...seedFor(net.key), ...found])];
  };
  let node;
  if (o.pool) {
    const pool = await probePool(await candidates(), probe);
    log('POOL_PROBED', {network: net.key, healthy: pool.healthy.map(c => ({url: c.url, p2pId: c.p2pId, rttMs: c.rttMs, lag: String(c.lag)})), rejected: pool.rejected});
    const pair = choosePair(pool.healthy);
    if (pair.length < 2) log('POOL_ONE_BACKEND_ONLY', {network: net.key, chosen: pair.map(c => c.url)});
    if (!pair.length) throw Error('POOL_NO_HEALTHY_NODE ' + net.key);
    const urls = pair.length === 2 ? pair.map(c => c.url)
      : [pair[0].url, pool.healthy.find(c => c.url !== pair[0].url)?.url ?? seedFor(net.key).find(u => u !== pair[0].url)];
    log('POOL_CHOSEN', {network: net.key, urls});
    node = await openDualNode({urls, log, open: openMember, pickReplacement: async ({avoidP2pIds, avoidUrls}) => {
      const again = await probePool((await candidates()).filter(u => !avoidUrls.includes(u)), probe);
      const next = chooseReplacement(again.healthy, avoidP2pIds);
      log('POOL_REPROBED', {network: net.key, chosen: next?.url ?? null, healthy: again.healthy.length, rejected: again.rejected.length});
      return next?.url ?? null;
    }});
  } else if (o.urls) node = await openDualNode({urls: o.urls.split(',').map(s => s.trim()), log, open: openMember});
  else node = await openNode({network, url: o.url ?? null, log});
  const engine = new Engine({store, contracts, node, addressOf: addressOfFactory(null, network), log,
    config: {network: net.key, finalityBlueScore: BigInt(o['finality-blue-score']),
      // An explicit --rest-url (or KASWIN_REST_URL, including the disable value) wins for every network;
      // otherwise each network uses its OWN REST endpoint (never another network's).
      restUrl: o.restUrlExplicit ? o.restUrl : net.rest}});
  node.on('utxos', data => engine.onUtxosChanged(data));
  node.on('chain', removed => engine.onChainChanged(removed));
  return {net, store, contracts, node, engine, dbPath};
}

// ---------------------------------------------------------------- status
if (cmd === 'status') {
  const nets = parseNetworks(o.networks ?? o.network, DEFAULT_NETWORK);
  const safe = x => JSON.parse(JSON.stringify(x, (_, v) => typeof v === 'bigint' ? v.toString() : v));
  const out = {};
  for (const net of nets) {
    const dbPath = dbPathFor(o.db, net, nets.length > 1);
    let store;
    try { store = new Store(dbPath, {network: net.key}); } catch (e) { out[net.key] = {db: dbPath, error: String(e?.message ?? e)}; continue; }
    out[net.key] = {db: dbPath, rounds: store.rounds().map(r => ({id: r.id, contract: r.contract, status: r.status, address: r.address, tip: r.data.tip, terminal: r.data.terminal,
      phase: r.data.state?.phase, sold: r.data.state?.sold, transitions: store.transitions(r.id).map(t => ({seq: t.seq, kind: t.kind, txid: t.txid, status: t.status}))})),
      outboxPending: store.pending(1000).length, journal: store.journal(15)};
    store.close();
  }
  console.log(JSON.stringify(safe(nets.length === 1 ? out[nets[0].key] : out), null, 1));
  process.exit(0);
}

// ---------------------------------------------------------------- contracts
if (cmd === 'contracts') {
  const nets = parseNetworks(o.networks ?? o.network, DEFAULT_NETWORK);
  const out = {};
  for (const net of nets) {
    const contracts = await loadContracts(o.contracts, {
      network: net.key, networkGenesis: net.genesis,
      registryAddresses: o.registryAddresses ? o.registryAddresses.split(',').map(s => s.trim()) : undefined,
    });
    out[net.key] = {networkId: net.networkId, genesis: net.genesis, registry: net.registry,
      contracts: contracts.describe(), watch: contracts.watchAddresses()};
  }
  console.log(JSON.stringify({config: o.contracts, networks: nets.length === 1 ? out[nets[0].key] : out}, null, 1));
  process.exit(0);
}

// ---------------------------------------------------------------- run / track
const nets = parseNetworks(o.networks ?? o.network, DEFAULT_NETWORK);
if ([o.url, o.urls, o.resolver, o.pool].filter(Boolean).length > 1) throw Error('USE_ONE_OF_URL_URLS_RESOLVER_POOL');
if ((o.url || o.urls) && nets.length > 1) throw Error('EXPLICIT_NODES_ARE_SINGLE_NETWORK');

const live = [];
for (const net of nets) {
  const ctx = await openNetwork(net, {multi: nets.length > 1});
  live.push(ctx);
  log('NETWORK_READY', {network: net.key, db: ctx.dbPath, node: ctx.node.identity, contracts: ctx.contracts.describe()});
}

if (cmd === 'track') {
  const [txid, start] = rest;
  if (!/^[0-9a-f]{64}$/.test(txid ?? '') || !/^[0-9a-f]{64}$/.test(start ?? '')) throw Error('usage: track GENESIS_TXID START_CHAIN_HASH');
  if (live.length !== 1) throw Error('TRACK_TAKES_ONE_NETWORK');
  const {engine, node, store} = live[0];
  await engine.start();
  // Serialize with retryPending()/event work queued by start(): never call engine work outside Engine.run.
  let result = 'NOT_RUN', failure = null;
  await engine.run('track:' + txid, async () => {
    try { result = await engine.discoverGenesis(txid, {starts: [start], contract: o.contract ?? null}); } catch (e) { failure = e; throw e; }
  });
  await engine.idle();
  log('TRACK', failure ? 'FAILED ' + String(failure?.message ?? failure) : result);
  await node.close(); store.close();
  process.exit(failure ? 1 : 0);
}

// ---------------------------------------------------------------- run (long-lived)
const relay = makeRelay({url: o.relay ?? null, token: process.env.KASWIN_RELAY_TOKEN ?? null, file: o['relay-file'] ?? null});
let stopping = false;
const timers = [];
for (const ctx of live) {
  const {net, store, node, engine} = ctx;
  let degraded = false;
  node.on('disconnect', () => { degraded = true; log('DISCONNECTED', {network: net.key, node: node.identity}); });
  node.on('connect', () => engine.run('reconnect', async () => {
    if (!degraded) return;
    await node.verify();                       // resolver may have handed us a different node
    await engine.resubscribeAll();
    degraded = false; log('RESUBSCRIBED', {network: net.key, node: node.identity});
  }));
  node.on('failover', info => { log('FAILOVER_RECONCILE', {network: net.key, ...info}); engine.run('reconcile', () => engine.reconcile()); engine.retryPending(); });
  await engine.start();
  if (node.subscribeVirtualChainChanged) {
    try { await node.subscribeVirtualChainChanged(); engine.chainSubscribed = true; log('CHAIN_SUBSCRIBED', {network: net.key}); }
    catch (e) { log('CHAIN_SUBSCRIBE_FAILED', {network: net.key, message: String(e?.message ?? e)}); }
  }
  log('STARTED', {network: net.key, node: node.identity, live: store.rounds({liveOnly: true}).length, contracts: ctx.contracts.describe()});
  const every = (ms, fn) => { const t = setInterval(() => { if (!stopping && !degraded) fn(); }, ms); t.unref?.(); return t; };
  timers.push(
    every(Number(o['checkpoint-ms']), () => engine.run('checkpoint', () => engine.checkpoint())),
    every(Number(o['reconcile-ms']), () => { engine.run('reconcile', () => engine.reconcile()); engine.run('finality', () => engine.checkFinality()); }),
    every(5000, () => engine.retryPending()),
    every(2000, () => flushOutbox(store, relay).catch(e => log('RELAY_RETRY', {network: net.key, message: String(e.message)}))),
  );
}

let api = null;
if (o['api-port']) {
  // The admin token is shared: it lives in the first network's DB (single-writer), so /admin works on either view.
  const primary = live[0];
  let adminToken = o['admin-token'] ?? process.env.ADMIN_TOKEN ?? process.env.KASPA_INDEXER_ADMIN_TOKEN;
  if (!adminToken) {
    adminToken = primary.store.getMeta('adminToken');
    if (!adminToken) {
      const crypto = await import('node:crypto');
      adminToken = crypto.randomBytes(16).toString('hex');
      primary.store.setMeta('adminToken', adminToken);
      log('ADMIN_TOKEN_INITIALIZED', {token: adminToken, note: 'Saved in the primary network database meta. Use this token to access /admin'});
    }
  } else {
    primary.store.setMeta('adminToken', adminToken);
  }
  api = createApi({
    store: primary.store,
    corsOrigin: o['cors-origin'] ?? '*',
    engine: primary.engine,
    contracts: primary.contracts,
    node: primary.node,
    network: primary.net.networkId,
    adminToken,
    contractsPath: o.contracts,
    openMember: url => openNode({network: primary.net.networkId, url, timeoutMs: 6000, connectDeadlineMs: 8000}),
    // Multi-network: every network gets its own read-only view, keyed by the network table key.
    networks: live.map(c => ({key: c.net.key, label: c.net.label, networkId: c.net.networkId, genesis: c.net.genesis,
      rest: c.net.rest, registry: c.net.registry, store: c.store, engine: c.engine, contracts: c.contracts, node: c.node})),
    log
  });
  await new Promise((resolve, reject) => { api.once('error', reject); api.listen(Number(o['api-port']), o['api-host'], resolve); });
  log('API_LISTENING', api.address());
  log('ADMIN_DASHBOARD', {url: `http://${o['api-host']}:${o['api-port']}/admin`});
}

const stop = async () => {
  stopping = true; timers.forEach(clearInterval);
  if (api) await new Promise(resolve => api.close(resolve));
  for (const {engine, node, store} of live) { await engine.idle(); try { await node.close(); } catch {} store.close(); }
  process.exit(0);
};
process.on('SIGINT', stop); process.on('SIGTERM', stop);
await new Promise(() => {});
