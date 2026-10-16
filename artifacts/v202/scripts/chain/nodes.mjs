/** Direct TN10 JSON wRPC (no SDK/WASM) to ONE configured Kaspa node.
 * Trust model (project decision 2026-10-02): the page relies on the Kaspa network consensus as served by the node the
 * user configured (default: wss://tn10.kaspay.top/wrpc). It no longer cross-checks a second
 * node. Value safety still comes from L1 consensus: the covenant scripts are executed by every node, and the page only
 * signs transactions it built and verified itself against the fixed contract templates.
 * Message format (rusty-kaspa cfafeb4 / workflow-rpc): request {id,method,params}; response {id,params} or {id,error}.
 */
import {parseJson, jsonText, uint} from './json.mjs';
import {ensure, hash32, NETWORK_ID, errorText} from '../core.mjs';
import {requireTradingRelease} from '../release-safety.mjs';
import {parseFeeEstimate} from './feetiers.mjs';
import {LIMITS, withDeadline, scopedLink, pause, failure, requireSyncPermission, queryContext, sameQueryContext} from './operation.mjs';
import {assertNodeNetwork, checkServerIdentity} from './node-identity.mjs';
import {DEFAULT_NODES} from './endpoints.mjs';

const READ = new Set(['getServerInfo', 'getInfo', 'getBlockDagInfo', 'getSink', 'getSinkBlueScore', 'getBlock', 'getUtxosByAddresses',
  'getVirtualChainFromBlock', 'getVirtualChainFromBlockV2', 'getFeeEstimate', 'getFeeEstimateExperimental', 'getSeqCommitLaneProof', 'getMempoolEntriesByAddresses', 'getMempoolEntry']);
const MAX_FRAME = 64 * 1024 * 1024;

/** Node endpoint: wss:// or ws:// (e.g. your own kaspad with `--rpclisten-json` on the LAN). */
export function nodeUrl(v) {
  const u = new URL(String(v).trim());
  ensure(u.protocol === 'wss:' || u.protocol === 'ws:', '节点地址必须以 wss:// 或 ws:// 开头');
  ensure(!u.username && !u.password && !u.search && !u.hash, '节点地址不能包含凭据、查询参数或片段');
  return u.href;
}

export class JsonRpc {
  constructor(url, {timeoutMs = 45000, maxTimeoutMs = Infinity, WebSocketImpl = globalThis.WebSocket} = {}) {
    this.url = nodeUrl(url); this.timeoutMs = timeoutMs; this.maxTimeoutMs = maxTimeoutMs; this.WS = WebSocketImpl;
    this.ws = null; this.next = 0; this.pending = new Map(); this.info = null; this.closed = false;
    this.epoch = 0; this.chainRevision = 0;
  }
  connect() {
    if (this.ws) return this.ready;
    this.ready = new Promise((resolve, reject) => {
      let ws;
      try { ws = new this.WS(this.url); } catch (e) { reject(e); return; }
      this.ws = ws; this.epoch++;
      this.closed = false;
      const timer = setTimeout(() => { reject(new Error(`连接超时：${host(this.url)}`)); this.close(); }, Math.min(this.timeoutMs, 15000));
      this.cancelConnect = reason => { clearTimeout(timer); reject(reason); };
      ws.onopen = () => { clearTimeout(timer); this.cancelConnect = null; resolve(); };
      ws.onerror = () => { clearTimeout(timer); reject(new Error(`无法连接：${host(this.url)}`)); this.close(); };
      ws.onclose = () => { clearTimeout(timer); reject(new Error(`连接已关闭：${host(this.url)}`)); this.close(new Error('NODE_DISCONNECTED')); };
      ws.onmessage = ev => {
        try {
          if (typeof ev.data !== 'string' || ev.data.length > MAX_FRAME) throw new Error('RPC_FRAME');
          const m = parseJson(ev.data);
          const id = typeof m?.id === 'number' ? m.id : null;
          const p = id === null ? null : this.pending.get(id);
          if (!p) return;
          clearTimeout(p.timer); this.pending.delete(id);
          if (m.error !== undefined && m.error !== null) p.reject(new RpcError(p.method, m.error));
          else {
            if (m.params?.removedChainBlockHashes?.length) this.chainRevision++;
            p.resolve(m.params);
          }
        } catch (e) { this.close(e); }
      };
    });
    return this.ready;
  }
  async call(method, params = {}, timeoutMs = this.timeoutMs, {signal, beforeSend = () => {}} = {}) {
    timeoutMs = Math.min(timeoutMs, this.maxTimeoutMs);
    const submit = method === 'submitTransaction' || method === 'submitTransactionReplacement';
    if (!READ.has(method) && !submit) throw failure('RPC_METHOD');
    if (submit) requireTradingRelease();
    return withDeadline(async scope => {
      await this.connect(); scope.check();
      if (this.ws?.readyState !== 1) throw failure('NODE_DISCONNECTED');
      const id = ++this.next, text = `{"id":${id},"method":${JSON.stringify(method)},"params":${jsonText(params)}}`;
      const result = await new Promise((resolve, reject) => {
        const timer = setTimeout(() => { this.pending.delete(id); reject(failure('RPC_TIMEOUT')); }, timeoutMs);
        this.pending.set(id, {resolve, reject, timer, method});
        try {
          requireSyncPermission(beforeSend());
          scope.check(); // no await between synchronous permission fence and transport
          if (submit) requireTradingRelease();
          this.ws.send(text);
        } catch (e) { clearTimeout(timer); this.pending.delete(id); reject(e); }
      });
      scope.check(); return result;
    }, {timeoutMs, signal, code: 'RPC_TIMEOUT', onCancel: () => this.close()});
  }
  close(reason = new Error('NODE_CLOSED')) {
    const ws = this.ws; this.ws = null; this.closed = true; this.epoch++;
    this.cancelConnect?.(reason); this.cancelConnect = null;
    if (ws) { ws.onopen = ws.onclose = ws.onerror = ws.onmessage = null; try { ws.close(); } catch {} }
    for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(reason); }
    this.pending.clear();
  }
}
export class RpcError extends Error {
  constructor(method, err) { super(`${method}: ${typeof err?.message === 'string' ? err.message : JSON.stringify(err)}`); this.name = 'RpcError'; this.method = method; this.rpc = err; }
}
export const host = url => { try { return new URL(url).host; } catch { return String(url); } };

/** Probe one node: network / sync / UTXO index / DAA / RTT. */
export async function probeNode(url, opts = {}) {
  const rpc = new JsonRpc(url, opts);
  try {
    await identify(rpc, opts.net ?? {networkId: NETWORK_ID}, opts.verifyDag === true);
    return rpc;
  } catch (e) { rpc.close(); throw e; }
}
async function identify(rpc, net, verifyDag, scope = null) {
  const start = Date.now(), view = scope ? scopedLink(rpc, scope) : rpc;
  const id = verifyDag ? await assertNodeNetwork(view, net) : checkServerIdentity(await view.call('getServerInfo'), net.networkId);
  scope?.check();
  rpc.info = {version: id.serverVersion, daa: BigInt(id.virtualDaa), rtt: Date.now() - start};
}

/** One node, chosen from an ordered list: the first reachable, synced TN10 node with a UTXO index wins.
 * `connect()` returns the active RPC list; `call()` applies the configured operation deadline and retry policy. */
export class NodeLink {
  constructor(urls = DEFAULT_NODES, opts = {}) {
    this.urls = [...new Set(urls.map(nodeUrl))]; ensure(this.urls.length, '至少需要配置一个节点');
    this.opts = opts; this.net = opts.net ?? {networkId: NETWORK_ID};
    this.rpcMs = opts.timeoutMs ?? 45000; this.stepMs = opts.stepMs ?? 90000;
    this.connectMs = opts.connectMs ?? Math.max(30000, this.urls.length * this.rpcMs * 2);
    this.nodes = []; this.opening = null; this.connectionCancel = null; this.listeners = new Set(); this.heartbeat = null; this.rejected = [];
    this.readViews = new Map(); this._anchor = null;
  }
  readContext() { return queryContext(this.nodes[0] ?? {ws: null}); }
  rememberChainAnchor(hash) { this._anchor = {hash: hash32(hash), observedAt: Date.now(), context: this.readContext()}; }
  get knownAnchor() { return this._anchor && sameQueryContext(this._anchor.context, this.readContext()) ? this._anchor : null; }
  /** Fresh by default. Reuse is explicit, bounded, connection/reorg-scoped and never used for submission RPCs. */
  async readView(key, task, {maxAgeMs = 0, signal} = {}, apply = () => {}, retries = 1) {
    ensure(Number.isSafeInteger(maxAgeMs) && maxAgeMs >= 0 && maxAgeMs <= 10000, 'READ_VIEW_AGE');
    if (signal?.aborted) throw signal.reason;
    await this.connect();
    const context = this.readContext(), now = Date.now(), previous = this.readViews.get(key);
    if (maxAgeMs && previous?.value !== undefined && sameQueryContext(previous.context, context) &&
        now >= previous.observedAt && now - previous.observedAt < maxAgeMs) {
      apply(previous.value); return structuredClone(previous.value);
    }
    // A scoped caller owns its cancellation; do not share its in-flight deadline with unrelated callers.
    if (!signal && previous?.pending && sameQueryContext(previous.context, context)) return structuredClone(await previous.pending);
    const entry = {context, observedAt: now};
    const pending = Promise.resolve().then(task).then(value => {
      if (signal?.aborted) throw signal.reason;
      if (!sameQueryContext(context, this.readContext())) {
        if (retries) return this.readView(key, task, {maxAgeMs: 0, signal}, apply, retries - 1);
        throw failure('READ_VIEW_CHANGED');
      }
      if (this.readViews.get(key) === entry) { entry.value = structuredClone(value); apply(entry.value); }
      return value;
    }).finally(() => { if (this.readViews.get(key) === entry) delete entry.pending; });
    entry.pending = pending; this.readViews.set(key, entry);
    try { return structuredClone(await pending); }
    catch (e) { if (this.readViews.get(key) === entry) this.readViews.delete(key); throw e; }
  }
  onChange(f) { this.listeners.add(f); return () => this.listeners.delete(f); }
  emit() { for (const f of this.listeners) { try { f(this.status()); } catch {} } }
  status() { return {connected: this.nodes.length === 1, nodes: this.nodes.map(n => ({url: n.url, host: host(n.url), ...n.info})), rejected: this.rejected, preferred: this.urls[0]}; }
  async connect({exclude = new Set()} = {}) {
    if (this.nodes.length === 1 && this.nodes[0].ws && !exclude.has(this.nodes[0].url)) return this.nodes;
    if (this.opening) {
      const nodes = await this.opening;
      if (!nodes.some(n => exclude.has(n.url))) return nodes;
      // A concurrent connection may have selected this operation's failed endpoint.
      return this.connect({exclude});
    }
    this.close(false);
    const cancel = new AbortController(); this.connectionCancel = cancel;
    let active = null;
    const opening = this.opening = withDeadline(async scope => {
      const rejected = [];
      for (const url of this.urls) {
        scope.check();
        if (exclude.has(url)) continue;
        const n = active = new JsonRpc(url, {...this.opts, timeoutMs: this.rpcMs});
        try {
          await identify(n, this.net, this.opts.verifyDag === true, scope); scope.check();
          this.nodes = [n]; this.rejected = rejected; active = null;
          n.ws.addEventListener?.('close', () => { if (this.nodes[0] === n) { this.nodes = []; this.emit(); } });
          if (this.opts.heartbeat !== false) {
            this.heartbeat = setInterval(() => { for (const x of this.nodes) x.call('getSinkBlueScore').catch(() => {}); }, 20000);
            this.heartbeat.unref?.();
          }
          this.emit(); return this.nodes;
        } catch (e) { n.close(); scope.check(); rejected.push({url, reason: errorText(e), code: e?.safeCode}); }
      }
      this.rejected = rejected; this.emit();
      const code = rejected.length && rejected.every(r => r.code === 'NODE_NETWORK_MISMATCH') ? 'NODE_NETWORK_MISMATCH' : 'NO_NODE';
      throw Object.assign(failure(code), {message: `没有可用的 ${this.net.networkId === NETWORK_ID ? 'TN10' : this.net.networkId} 节点：${rejected.map(r => `${host(r.url)}（${r.reason}）`).join('；')}`});
    }, {timeoutMs: this.connectMs, signal: cancel.signal, code: 'CONNECT_TIMEOUT', onCancel: () => active?.close()});
    try { return await opening; } finally { if (this.opening === opening) this.opening = null; if (this.connectionCancel === cancel) this.connectionCancel = null; }
  }
  close(emit = true) {
    this.connectionCancel?.abort(failure('CONNECTION_CANCELLED'));
    clearInterval(this.heartbeat); this.heartbeat = null;
    for (const n of this.nodes) n.close(); this.nodes = []; this.readViews.clear(); this._anchor = null;
    if (emit) this.emit();
  }
  async call(method, params, timeoutMs = this.stepMs, options = {}) {
    return withDeadline(async scope => {
      const attempts = READ.has(method) ? (this.opts.readAttempts ?? 1) : 1;
      const failed = new Set();
      for (let attempt = 0; attempt < attempts; attempt++) {
        let rpc;
        try {
          [rpc] = await this.connect({exclude: failed}); scope.check();
          const result = await rpc.call(method, params, Math.min(this.rpcMs, scope.remaining()),
            {...options, signal: scope.signal, beforeSend: () => { scope.check(); return options.beforeSend?.(); }});
          scope.check(); return result;
        } catch (e) {
          scope.check();
          if (attempt === attempts - 1 || e?.safeCode === 'NODE_NETWORK_MISMATCH') throw e;
          if (!rpc) throw e; // connect already exhausted its eligible candidates
          failed.add(rpc.url);
          if (!this.urls.some(url => !failed.has(url))) throw e;
          if (this.nodes[0] === rpc) this.close(false);
          await pause(500, scope.signal);
        }
      }
    }, {timeoutMs: Math.min(timeoutMs, this.stepMs), signal: options.signal, code: 'RPC_OPERATION_TIMEOUT', onCancel: () => this.close(false)});
  }
  /** Current virtual DAA of the connected node; it must be a synced TN10 node with a UTXO index. */
  async currentDaa(options = {}) {
    const s = await this.readView('health', () => this.call('getServerInfo', {}, this.rpcMs, options), options, info => {
      checkServerIdentity(info, this.net.networkId);
      const daa = uint(info.virtualDaaScore);
      if (this.nodes[0]) this.nodes[0].info.daa = daa;
      this.emit();
    });
    checkServerIdentity(s, this.net.networkId);
    return uint(s.virtualDaaScore);
  }
  /** Node fee estimate in sompi per gram of the mempool ORDERING mass max(compute, normalized transient, storage)
   * (rusty-kaspa cfafeb4 mining/src/mempool/model/frontier/feerate_key.rs from_tx). Price against that mass. */
  async feerate(options = {}) { return (await this.feeConditions(options)).normal.feerate; }
  /** Fee buckets plus, when the node serves it, the ready-mempool size (getFeeEstimateExperimental verbose; read-only,
   * rusty-kaspa v2.1.0 rpc/core). Falls back to getFeeEstimate; never guesses mempool data it was not given. */
  async feeConditions(options = {}) {
    return this.readView('fees', async () => {
      let r = null;
      try { r = await this.call('getFeeEstimateExperimental', {verbose: true}, 8000, options); }
      catch (e) { if (options.signal?.aborted) throw options.signal.reason; }
      if (r?.estimate) { try { return parseFeeEstimate(r, r.verbose ?? null); } catch {} }
      return parseFeeEstimate(await this.call('getFeeEstimate', {}, this.rpcMs, options));
    }, options);
  }
}
/** Multi-network policy adapter; all connection/RPC/fee algorithms live above. No network table in browser graph. */
export class NetworkLink extends NodeLink {
  constructor(net, opts = {}) {
    super(net.nodes, {...opts, net, verifyDag: true, timeoutMs: LIMITS.rpcMs, stepMs: LIMITS.stepMs,
      connectMs: LIMITS.connectMs, maxTimeoutMs: LIMITS.rpcMs, readAttempts: 3, heartbeat: false});
  }
}
export class NetworkRpc extends JsonRpc {
  constructor(url, opts = {}) { super(url, {...opts, timeoutMs: LIMITS.rpcMs, maxTimeoutMs: LIMITS.rpcMs}); }
}

/** RPC transaction JSON (human-readable serde of RpcTransaction) <-> library DTO. */
export function parseSpk(raw) {
  ensure(typeof raw === 'string' && /^[0-9a-f]{4}(?:[0-9a-f]{2})*$/i.test(raw), '节点返回的 scriptPublicKey 格式错误');
  return {version: parseInt(raw.slice(0, 4), 16), script: raw.slice(4).toLowerCase()};
}
export const spkText = spk => spk.version.toString(16).padStart(4, '0') + spk.script;
export function txFromRpc(t) {
  ensure(t && Array.isArray(t.inputs) && Array.isArray(t.outputs) && Number.isInteger(t.version), '节点交易缺少完整字段');
  return {
    version: t.version,
    inputs: t.inputs.map(i => ({previousOutpoint: {transactionId: hash32(i.previousOutpoint.transactionId), index: Number(uint(i.previousOutpoint.index))}, signatureScript: String(i.signatureScript).toLowerCase(), sequence: uint(i.sequence), computeBudget: Number(uint(i.computeBudget ?? 0))})),
    outputs: t.outputs.map(o => ({value: uint(o.value), scriptPublicKey: parseSpk(o.scriptPublicKey), covenant: o.covenant ? {covenantId: hash32(o.covenant.covenantId), authorizingInput: Number(uint(o.covenant.authorizingInput))} : null})),
    lockTime: uint(t.lockTime), subnetworkId: String(t.subnetworkId).toLowerCase(), gas: uint(t.gas), payload: String(t.payload ?? '').toLowerCase(),
    storageMass: uint(t.storageMass ?? t.mass ?? 0),
  };
}
export function txToRpc(t) {
  return {
    version: t.version,
    inputs: t.inputs.map(i => ({previousOutpoint: {transactionId: i.previousOutpoint.transactionId, index: i.previousOutpoint.index}, signatureScript: i.signatureScript, sequence: i.sequence, sigOpCount: 0, computeBudget: i.computeBudget ?? 0, verboseData: null})),
    outputs: t.outputs.map(o => ({value: o.value, scriptPublicKey: spkText(o.scriptPublicKey), verboseData: null, covenant: o.covenant ? {authorizingInput: o.covenant.authorizingInput, covenantId: o.covenant.covenantId} : null})),
    lockTime: t.lockTime, subnetworkId: t.subnetworkId, gas: t.gas, payload: t.payload, storageMass: t.storageMass, verboseData: null,
  };
}
/** UTXO entry from getUtxosByAddresses. */
export function utxoFromRpc(e) {
  const u = e.utxoEntry;
  return {outpoint: {transactionId: hash32(e.outpoint.transactionId), index: Number(uint(e.outpoint.index))}, value: uint(u.amount), spk: parseSpk(u.scriptPublicKey), daa: uint(u.blockDaaScore), covenantId: u.covenantId ?? null, isCoinbase: u.isCoinbase === true, address: e.address};
}
