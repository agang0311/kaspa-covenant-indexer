// Read-only cache API and Admin Management API.
import http from 'node:http';
import {handleAdmin} from './admin.mjs';
const HASH = /^[0-9a-f]{64}$/;
/** Reorg recheck depth (project policy, same as the Kaswin page; NOT consensus finality). Read-only VSPC monitor:
 * deepest observed selected-chain removal 83 DAA (mainnet 24 h, run incomplete), 75 DAA (TN10 5.3 h). */
export const REORG_RECHECK_DAA = 100n;
/**
 * Confidence of the round's CURRENT state from the transition that produced it:
 *  CONFIRMING: accepting block < 100 DAA below the latest checkpointed virtual DAA (a short reorg can still undo it),
 *  ACCEPTED:   deeper, not yet FINAL by the indexer's blue-score policy,
 *  FINAL:      the indexer's finality policy (blue score) reached; UNKNOWN: no checkpoint or no transition.
 * The checkpoint is at most a few seconds old, so depth is a lower bound; consumers must still verify on their node.
 */
export function acceptanceOf(t, virtualDaa) {
  if (!t) return {acceptance: 'UNKNOWN', acceptingDaa: null, depthDaa: null};
  const acceptingDaa = BigInt(t.acceptingDaa);
  if (virtualDaa == null) return {acceptance: t.status === 'FINAL' ? 'FINAL' : 'UNKNOWN', acceptingDaa: String(acceptingDaa), depthDaa: null};
  const v = BigInt(virtualDaa), depth = v > acceptingDaa ? v - acceptingDaa : 0n;
  const acceptance = t.status === 'FINAL' ? 'FINAL' : depth >= REORG_RECHECK_DAA ? 'ACCEPTED' : 'CONFIRMING';
  return {acceptance, acceptingDaa: String(acceptingDaa), depthDaa: String(depth)};
}
/** The only supported contract version is Kaswin V2.0.2. Live phases are exactly 1 (open), 2 (sealed), 5 (refunding). */
export const CONTRACT_FAMILY = 'kaswin-v2.0.2@';
export function categoryOf(r) {
  const known = r.contract?.startsWith(CONTRACT_FAMILY);
  if (r.status === 'STALE' || !known) return 'unknown';
  if (r.status === 'TERMINAL') return 'close';
  const p = r.data.state?.phase;
  return p === 1 ? 'open' : p === 2 ? 'sealed' : p === 5 ? 'close' : 'unknown';
}
function view(r, detail = false, store = null, virtualDaa = null) {
  const d = r.data;
  const acc = store ? acceptanceOf(store.latestActive(r.id), virtualDaa) : null;
  // The node already showed the indexed tip as spent; the successor is still being resolved: this state is OUT OF DATE.
  const spentAt = store && r.status === 'LIVE' ? store.tipSpent(r.id, d.tip) : null;
  return {cid: d.cid, genesisTxid: r.id, contract: r.contract, status: categoryOf(r), phase: d.state?.phase ?? null,
    terminal: d.terminal ?? null, indexStatus: r.status, tip: d.tip, address: r.address, value: String(d.value),
    latestTxid: d.latestTxid ?? null, accepting: d.accepting ?? null, utxoDaa: d.utxoDaa == null ? null : String(d.utxoDaa),
    updatedAt: r.updatedAt, liveSeenAt: r.liveSeenAt, ...(acc ?? {}), ...(store ? {tipSpentSeenAt: spentAt} : {}),
    ...(detail ? {state: d.state, purchases: d.purchases, origin: d.origin, scriptPublicKey: d.spk} : {})};
}
export function createApi({
  store,
  corsOrigin = '*',
  engine = null,
  contracts = null,
  node = null,
  sdk = null,
  network = 'testnet-10',
  networks = null,
  adminToken = null,
  contractsPath = null,
  openMember = null,
  log = () => {}
}) {
  const isNone = corsOrigin === false || corsOrigin === null || corsOrigin === 'none';
  if (!isNone && corsOrigin !== '*' && !/^https?:\/\/[^/]+$/.test(corsOrigin)) throw Error('INVALID_CORS_ORIGIN');
  const server = http.createServer(async (req, res) => {
    const send = (status, body) => {
      res.writeHead(status, {'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff'});
      res.end(JSON.stringify(body, (_, v) => typeof v === 'bigint' ? v.toString() : v));
    };
    if (!isNone) {
      if (corsOrigin === '*') {
        res.setHeader('Access-Control-Allow-Origin', '*');
        res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
        res.setHeader('Access-Control-Allow-Headers', 'Content-Type, Authorization');
      } else if (req.headers.origin === corsOrigin) {
        res.setHeader('Access-Control-Allow-Origin', corsOrigin);
        res.setHeader('Vary', 'Origin');
        res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
        res.setHeader('Access-Control-Allow-Headers', 'Content-Type, Authorization');
      }
    }
    if (req.method === 'OPTIONS') { res.writeHead(204); res.end(); return; }

    const url = new URL(req.url, 'http://localhost');

    // Public status endpoints (aliased to status handler, completely open & unauthenticated)
    if (url.pathname === '/status' || url.pathname === '/v1/status') {
      try {
        const handled = await handleAdmin(Object.assign(Object.create(req), {url: '/admin/api/status' + url.search}), res, {
          store, engine, contracts, node, sdk, network, networks, adminToken, contractsPath, openMember, log
        });
        if (handled) return;
      } catch (err) {
        send(500, {ok: false, error: 'STATUS_ERROR', detail: err.message});
        return;
      }
    }

    // Admin dashboard & API handler (multi-network views are passed through)
    if (url.pathname.startsWith('/admin')) {
      try {
        const handled = await handleAdmin(req, res, {
          store, engine, contracts, node, sdk, network, networks, adminToken, contractsPath, openMember, log
        });
        if (handled) return;
      } catch (err) {
        send(500, {ok: false, error: 'ADMIN_HANDLER_ERROR', detail: err.message});
        return;
      }
    }

    if (req.method !== 'GET') { res.setHeader('Allow', 'GET, OPTIONS'); send(405, {detail: 'METHOD_NOT_ALLOWED'}); return; }
    try {
      // Multi-network catalog: build lookup index by network key ('tn10', 'mainnet') and networkId ('testnet-10').
      const netList = networks ?? [{key: 'tn10', networkId: network, store, engine, contracts, node}];
      const views = new Map();
      for (const v of netList) {
        views.set(v.key, v);
        if (v.networkId && v.networkId !== v.key) views.set(v.networkId, v);
      }
      const primaryKey = netList[0]?.key ?? 'tn10';
      const availableKeys = [...new Set(netList.map(v => v.key))];

      // Public Networks Catalog Discovery: GET /v1/networks
      if (url.pathname === '/v1/networks') {
        return send(200, {
          ok: true,
          defaultNetwork: primaryKey,
          networks: netList.map(v => ({
            key: v.key,
            label: v.label ?? null,
            networkId: v.networkId ?? v.key,
            genesis: v.genesis ?? null,
            rest: v.rest ?? null,
            registry: v.registry ?? null,
            rounds: v.store ? v.store.rounds().length : 0,
            live: v.store ? v.store.rounds({liveOnly: true}).length : 0,
            activeNode: v.node?.identity?.url ?? null,
            virtualDaaScore: v.node?.identity?.virtualDaaScore ?? null
          }))
        });
      }

      // Multi-network routing: support both query parameter ?network=<key> and path prefix /v1/<network>/rounds...
      let pathname = url.pathname;
      let pathNetwork = null;
      const pathNetMatch = /^\/v1\/([a-z0-9_-]+)\/(rounds(?:\/[0-9a-f]{64})?)$/.exec(pathname);
      if (pathNetMatch && views.has(pathNetMatch[1])) {
        pathNetwork = pathNetMatch[1];
        pathname = '/v1/' + pathNetMatch[2];
      }

      const requested = pathNetwork || url.searchParams.get('network');
      if (requested !== null && !views.has(requested)) {
        return send(400, {detail: 'UNKNOWN_NETWORK', networks: availableKeys});
      }

      const v = requested !== null ? views.get(requested) : netList[0];
      const vStore = v.store, vEngine = v.engine, vContracts = v.contracts, vNode = v.node;
      const net = {key: v.key, networkId: v.networkId ?? network, genesis: v.genesis ?? null, label: v.label ?? null, rest: v.rest ?? null, registry: v.registry ?? null};
      const cp = vStore.checkpointsBefore(Date.now() + 1, 1)[0] ?? null, virtualDaa = cp?.virtualDaa ?? null;
      const meta = {network: net.networkId, networkKey: net.key, networkGenesis: net.genesis, networks: availableKeys,
        coverage: 'tracked-rounds-only', requiresIndependentVerification: true,
        lastCheckpointAt: cp?.at ?? null, virtualDaa: virtualDaa == null ? null : String(virtualDaa), reorgRecheckDaa: String(REORG_RECHECK_DAA),
        discoveryCoverage: 'notification-or-manual-only', discoveryScan: {enabled: false}, discoveryPending: vStore.db.prepare("SELECT COUNT(*) n FROM discoveries WHERE status='PENDING'").get().n};
      const match = /^\/v1\/rounds\/([^/]+)$/.exec(pathname);
      if (match) {
        if (!HASH.test(match[1]) || [...url.searchParams.keys()].some(k => k !== 'network')) return send(400, {detail: 'INVALID_CID_OR_QUERY'});
        const rows = vStore.byCid(match[1]);
        if (!rows.length) return send(404, {detail: 'NOT_INDEXED', ...meta});
        if (rows.length > 1) return send(409, {detail: 'CID_CONFLICT'});
        return send(200, {item: view(rows[0], true, vStore, virtualDaa), ...meta});
      }
      if (pathname !== '/v1/rounds') return send(404, {detail: 'NOT_FOUND'});
      for (const key of url.searchParams.keys()) if (!['status', 'limit', 'cursor', 'network'].includes(key) || url.searchParams.getAll(key).length !== 1)
        return send(400, {detail: 'INVALID_QUERY'});
      const status = url.searchParams.get('status'), cursor = url.searchParams.get('cursor') ?? '', rawLimit = url.searchParams.get('limit') ?? '50';
      if ((status !== null && !['open', 'sealed', 'close', 'unknown'].includes(status)) ||
          (cursor && !HASH.test(cursor)) || !/^[1-9][0-9]{0,2}$/.test(rawLimit) || Number(rawLimit) > 200)
        return send(400, {detail: 'INVALID_QUERY'});
      const limit = Number(rawLimit), rows = vStore.cidPage(cursor, limit + 1, status), page = rows.slice(0, limit);
      if (page.some(r => vStore.byCid(r.data.cid).length > 1)) return send(409, {detail: 'CID_CONFLICT'});
      return send(200, {items: page.map(r => view(r, false, vStore, virtualDaa)), nextCursor: rows.length > limit ? page.at(-1).data.cid : null, ...meta});
    } catch { send(500, {detail: 'INTERNAL_ERROR'}); }
  });
  server.requestTimeout = 10000; server.headersTimeout = 10000;
  return server;
}
