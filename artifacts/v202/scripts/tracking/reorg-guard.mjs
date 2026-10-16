// V2.0.2 CLI lineage/settlement policy adapter. diagnosis/signed-record/chain checks are shared with Web.
// This scheduler requires a SYNCHRONOUS durable event writer; Web CAS scheduling remains a distinct policy.
// Identical signed bytes may be resent ONLY while the caller's approval and send gate remain valid.
// Foreign transitions are read-only observations, but participate in the SAME lineage/settlement gate.
import * as S from '../core/lib/state.js';
import {interpretAccepted, decodeSpend} from '../core/lib/accepted.js';
import {stable, hex} from '../core/lib/bytes.js';
import {referenceTxId} from '../core/lib/transaction.js';
import {acceptedAt, mempoolEntry, REORG_RECHECK_DAA, matchesDraft} from '../chain/chain.mjs';
import {diagnoseRecord, classifySubmit, classifySubmitResponse} from './diagnosis.mjs';
import {requireSync} from '../store/intent-store.mjs';
import {signedWire, assertSignedRecord} from './signed-record.mjs';
import {txToRpc, parseSpk} from '../chain/nodes.mjs';
import {uint} from '../chain/json.mjs';
import {selectedChainAnchor} from '../chain/selected-chain.mjs';
import {PROFILE_ID, requireRuntimeProfile} from '../contract/runtime.mjs';
import {LIMITS, withDeadline, pause, scopedLink, failure, requireSyncPermission} from '../chain/operation.mjs';
import {DRAW_MIN_CONFIRMATIONS, drawTargetFromRecord, requireConfirmedDrawTarget, assertFreshDrawConfirmation, DRAW_REUSE_MS} from '../round/draw-confirmations.mjs';

export const SETTLE_DAA = REORG_RECHECK_DAA;
const fail = (code, extra = {}) => { throw Object.assign(failure(code), extra); };
const need = (v, code) => { if (!v) fail(code); };
const ON_CHAIN = new Set(['CONFIRMING', 'SETTLED']);
export const LOST = new Set(['CONFLICTED', 'DRIFTED', 'FUNDING_CONFLICTED', 'REJECTED']);
export const CHAIN_MOVED = new Set(['NOT_CHAIN', 'NOT_ACCEPTED', 'REORG', 'CHAIN_MOVED', 'SINK_BEHIND']);
const codeOf = e => e?.safeCode ?? e?.code ?? null;
const spk = s => typeof s === 'string' ? parseSpk(s) : s;

/** Location search, bounded in time and pages. Progress may be persisted by the caller; null means reached tip. */
export async function findAcceptance(link, txid, fromHash, {pages = 40, scope = null, progress = () => {}} = {}) {
  if (!scope) return withDeadline(s => findAcceptance(scopedLink(link, s), txid, fromHash, {pages, scope: s, progress}),
    {code: 'ACCEPTANCE_SEARCH_TIMEOUT', onCancel: () => link.close?.(false)});
  let at = fromHash;
  for (let p = 0; p < pages; p++) {
    scope.check();
    const r = await link.call('getVirtualChainFromBlock', {startHash: at, includeAcceptedTransactionIds: true}); scope.check();
    need(Array.isArray(r?.addedChainBlockHashes) && Array.isArray(r.acceptedTransactionIds), 'CHAIN_QUERY');
    for (const row of r.acceptedTransactionIds) if (row.acceptedTransactionIds?.includes(txid)) return row.acceptingBlockHash;
    if (!r.addedChainBlockHashes.length) return null;
    at = r.addedChainBlockHashes.at(-1); progress(at);
  }
  fail('SEARCH_CONTINUES');
}
export async function findSpender(rpc, outpoint, creatorAccepting, {window = 60n, maxBlue = 36000n, scope = null, scan = {}, progress = () => {}} = {}) {
  if (!scope) return withDeadline(s => findSpender(scopedLink(rpc, s), outpoint, creatorAccepting, {window, maxBlue, scope: s, scan, progress}),
    {code: 'SPENDER_SEARCH_TIMEOUT', onCancel: () => rpc.close?.()});
  if (!scan.from) {
    const b = (await rpc.call('getBlock', {hash: creatorAccepting, includeTransactions: false})).block; scope.check();
    need(b?.header?.hash === creatorAccepting && b.verboseData?.isChainBlock === true, 'CHAIN_MOVED');
    const from = b.verboseData.selectedParentHash, parent = (await rpc.call('getBlock', {hash: from, includeTransactions: false})).block; scope.check();
    Object.assign(scan, {from, blue: uint(parent.header.blueScore), startBlue: uint(parent.header.blueScore)});
  }
  for (let page = 0; page < 8; page++) {
    scope.check(); need(scan.blue - scan.startBlue <= maxBlue, 'SPENDER_SEARCH_LIMIT');
    const sink = uint((await rpc.call('getSinkBlueScore')).blueScore);
    const min = sink - scan.blue > window ? sink - scan.blue - window : 0n;
    const v = await rpc.call('getVirtualChainFromBlockV2', {startHash: scan.from, dataVerbosityLevel: 'High', minConfirmationCount: Number(min)}); scope.check();
    need(Array.isArray(v?.removedChainBlockHashes), 'CHAIN_QUERY');
    if (v.removedChainBlockHashes.length) { delete scan.from; scope.check(); progress(scan); fail('CHAIN_MOVED'); }
    need(Array.isArray(v.addedChainBlockHashes) && Array.isArray(v.chainBlockAcceptedTransactions) && v.chainBlockAcceptedTransactions.length === v.addedChainBlockHashes.length, 'CHAIN_QUERY');
    for (const [i, row] of v.chainBlockAcceptedTransactions.entries()) {
      need(row.chainBlockHeader?.hash === v.addedChainBlockHashes[i], 'CHAIN_QUERY');
      for (const t of row.acceptedTransactions ?? []) if ((t.inputs ?? []).some(input => input.previousOutpoint?.transactionId === outpoint.transactionId && Number(input.previousOutpoint.index) === outpoint.index))
        return {txid: t.verboseData.transactionId, accepting: row.chainBlockHeader.hash};
    }
    if (!v.addedChainBlockHashes.length) { if (min === 0n) return null; window *= 2n; continue; }
    const last = v.chainBlockAcceptedTransactions.at(-1).chainBlockHeader;
    Object.assign(scan, {from: last.hash, blue: last.blueScore != null ? uint(last.blueScore) : uint((await rpc.call('getBlock', {hash: last.hash, includeTransactions: false})).block.header.blueScore)});
    scope.check(); progress(scan);
    if (min === 0n) return null;
  }
  fail('SEARCH_CONTINUES');
}
export function liveFrom(ev, ledger, origin, profile) {
  const out = ev.tx.outputs[0];
  const snapshot = {ledger: S.encodeLedger(ledger), tip: {transactionId: ev.txid, index: 0}, origin, covenantId: out.covenant.covenantId,
    value: out.value, scriptPublicKey: out.scriptPublicKey, utxoDaa: ev.acceptingDaa, currentDaa: ev.acceptingDaa};
  S.verifySnapshot(snapshot, profile);
  return {ledger, snapshot, accepting: ev.accepting, acceptedTx: ev};
}
export function driveAction(ledger) { return ({[S.Phase.OPEN]: 'CLOSE', [S.Phase.SEALED]: 'DRAW_AND_PAY', [S.Phase.REFUNDING]: 'REFUND'})[ledger.phase] ?? null; }

export class ReorgGuard {
  constructor({link, ctx, event, assertSend = () => fail('SEND_PERMISSION_REQUIRED'), log = () => {}, limits = {}, now = () => Date.now()}) {
    Object.assign(this, {link, ctx, event, assertSend, log, now}); this.limits = {...LIMITS, ...limits};
    this.settleDaa = limits.settleDaa !== undefined ? BigInt(limits.settleDaa) : (limits.initialConfirmation ? 0n : SETTLE_DAA);
    this.items = new Map(); this.cursor = null; this.virtualDaa = 0n; this.blockDaa = new Map(); this.scans = new Map();
    this.ticking = null; this.loop = null; this.stopped = false; this.fatal = null; this.sendBlocked = null;
    this.acceptanceRevision = 0; // invalidate an in-progress completion scan even if a removed anchor is later restored
    this.cancel = new AbortController(); this.end = this.now() + this.limits.runMs; this.lastSuccess = this.now();
  }
  check(scope) { scope?.check(); if (this.fatal) throw this.fatal; if (this.stopped) fail('GUARD_STOPPED'); if (this.now() >= this.end) fail('RUN_DEADLINE'); }
  trip(e) { this.fatal ??= e; this.cancel.abort(this.fatal); this.link.close?.(false); return e; }
  blockSends(e = failure('SUITE_HALTED')) { this.sendBlocked ??= e; }
  operation(task, {timeoutMs = this.limits.stepMs, code = 'STEP_TIMEOUT', tripOnCancel = true} = {}) {
    return withDeadline(s => { this.check(s); return task(scopedLink(this.link, s), s); },
      {timeoutMs: Math.min(timeoutMs, Math.max(0, this.end - this.now())), signal: this.cancel.signal, code, onCancel: e => { if (tripOnCancel) this.trip(e); else this.link.close?.(false); }});
  }
  async start(pollMs = 2000) {
    this.cursor = await this.operation(async (link, s) => { const [rpc] = await link.connect(); const at = await selectedChainAnchor(rpc); this.check(s); return at; });
    this.link.rememberChainAnchor?.(this.cursor);
    this.loop = (async () => {
      while (!this.stopped && !this.fatal) {
        try { await this.tick(); this.lastSuccess = this.now(); }
        catch (e) { if (this.stopped || this.fatal) break; if (this.now() - this.lastSuccess >= this.limits.acceptanceMs) { this.trip(e); break; } }
        try { await pause(pollMs, this.cancel.signal); } catch { break; }
      }
    })();
  }
  async stop() {
    this.stopped = true; this.cancel.abort(failure('GUARD_STOPPED')); this.link.close?.(false);
    // All tails are fenced by the aborted signal. Do not hold the lock for slow sockets or a mock ignoring cancellation.
    await withDeadline(() => Promise.allSettled([this.loop, this.ticking]), {timeoutMs: this.limits.stopMs, code: 'STOP_TIMEOUT'}).catch(() => {});
  }
  note(it, status, data = {}) {
    this.check();
    try {
      requireSync(this.event(it.round, it.step, status, {txid: it.txid, ...data}));
      if (['DISPLACED', 'REACCEPTED', 'CONFIRMING', 'REVIVED', 'FOREIGN_OBSERVED'].includes(status)) this.acceptanceRevision++;
    } catch (cause) { throw this.trip(Object.assign(failure('EVIDENCE_WRITE_FAILED'), {cause})); }
  }
  track(round, step, record) {
    this.check(); const txid = record.draft.txid; need(!this.items.has(txid), 'DUPLICATE_TRACK');
    const copy = structuredClone(record);
    const it = {txid, round, step, record: copy, foreign: false, wire: signedWire(copy.signed), state: 'PENDING', accepting: null, acceptingDaa: null,
      firstAccepting: null, since: this.now(), lastDiag: 0, lastSeen: null, rebroadcasts: 0, rejections: 0, searchFrom: record.anchor, ev: null};
    this.items.set(txid, it); return it;
  }
  adopt(round, step, record, accepting, acceptingDaa, ev = null) {
    const it = this.track(round, step, record);
    Object.assign(it, {state: 'CONFIRMING', accepting, acceptingDaa, firstAccepting: accepting, ev}); return it;
  }
  get(txid) { return this.items.get(txid) ?? null; }
  supersede(txid) { const it = this.get(txid); if (it) it.state = 'SUPERSEDED'; return it; }
  revive(txid) {
    const it = this.get(txid); need(it, 'REVIVE_UNKNOWN');
    this.note(it, 'REVIVED', {acceptingBlockHash: it.accepting});
    it.state = it.accepting ? 'CONFIRMING' : 'DISPLACED'; it.since = this.now(); it.lastSeen = null; it.lastDiag = 0; return it;
  }
  async daaOf(hash, link, scope) {
    if (!this.blockDaa.has(hash)) {
      const b = (await link.call('getBlock', {hash, includeTransactions: false})).block; this.check(scope);
      need(b?.header?.hash === hash, 'BLOCK_HASH_MISMATCH'); this.blockDaa.set(hash, uint(b.header.daaScore));
    }
    return this.blockDaa.get(hash);
  }
  displaced(it, scope) {
    this.check(scope); this.note(it, 'DISPLACED', {acceptingBlockHash: it.accepting, acceptingDaa: String(it.acceptingDaa)});
    it.accepting = null; it.acceptingDaa = null; it.ev = null; it.searchFrom = it.record.anchor; it.since = this.now(); it.lastDiag = 0;
    if (it.state !== 'SUPERSEDED') it.state = 'DISPLACED';
  }
  tick() {
    return this.ticking ??= this.operation(async (link, s) => {
      await this.#poll(link, s); const daa = await link.currentDaa(); this.check(s); this.virtualDaa = daa;
      await this.#settle(link, s); await this.#maintain(link, s);
    }, {timeoutMs: this.limits.tickMs, code: 'TICK_TIMEOUT', tripOnCancel: false}).finally(() => { this.ticking = null; });
  }
  async #poll(link, scope) {
    for (let page = 0; page < 4; page++) {
      const r = await link.call('getVirtualChainFromBlock', {startHash: this.cursor, includeAcceptedTransactionIds: true}); this.check(scope);
      need(Array.isArray(r?.removedChainBlockHashes) && Array.isArray(r.addedChainBlockHashes) && Array.isArray(r.acceptedTransactionIds), 'CHAIN_QUERY');
      const removed = new Set(r.removedChainBlockHashes);
      for (const it of this.items.values()) if (it.accepting && removed.has(it.accepting)) this.displaced(it, scope);
      for (const row of r.acceptedTransactionIds) for (const id of row.acceptedTransactionIds ?? []) {
        const it = this.get(id); if (!it || it.accepting === row.acceptingBlockHash) continue;
        const daa = await this.daaOf(row.acceptingBlockHash, link, scope); this.check(scope);
        this.note(it, it.firstAccepting ? 'REACCEPTED' : 'CONFIRMING', {acceptingBlockHash: row.acceptingBlockHash, acceptingDaa: String(daa)});
        it.accepting = row.acceptingBlockHash; it.acceptingDaa = daa; it.firstAccepting ??= row.acceptingBlockHash; it.ev = null;
        if (it.state !== 'SUPERSEDED') it.state = 'CONFIRMING';
      }
      if (!r.addedChainBlockHashes.length) break;
      this.cursor = r.addedChainBlockHashes.at(-1);
      this.link.rememberChainAnchor?.(this.cursor);
    }
  }
  async #verify(it, link, scope) {
    const [rpc] = await link.connect(), ev = await acceptedAt(rpc, it.txid, it.accepting); this.check(scope);
    if (it.foreign) {
      need(stable(ev.tx) === stable(it.record.signed), 'FOREIGN_ACCEPTED_FIELDS');
      this.#interpretForeign(it.record.snapshot, it.profile, ev);
    } else {
      matchesDraft(ev, it.record.draft);
      if (it.record.snapshot) interpretAccepted({...it.record.snapshot, utxoDaa: ev.inputs[0].daa, currentDaa: ev.acceptingDaa},
        this.profile, ev.tx, ev.inputs.map(u => u.value));
    }
    // Independent recheck: no acceptance proof is inferred from block inclusion or cached DAA.
    const b = (await link.call('getBlock', {hash: ev.accepting, includeTransactions: false})).block; this.check(scope);
    need(b?.header?.hash === ev.accepting && b.verboseData?.isChainBlock === true, 'CHAIN_MOVED');
    need(uint(b.header.daaScore) === ev.acceptingDaa, 'ACCEPTANCE_DAA_MISMATCH'); return ev;
  }
  async #settle(link, scope) {
    let n = 0;
    for (const it of this.items.values()) {
      if (it.state !== 'CONFIRMING' || (it.foreign && !it.active) || this.virtualDaa < it.acceptingDaa + this.settleDaa) continue;
      if (scope.remaining() < 4000 || n++ >= 20) break; // single tick bounded <= 30s; settle loop accumulates ticks
      try {
        const at = it.accepting, ev = await this.#verify(it, link, scope); this.check(scope);
        if (it.state !== 'CONFIRMING' || it.accepting !== at) continue;
        if (this.virtualDaa < ev.acceptingDaa + this.settleDaa) continue;
        this.note(it, 'SETTLED', {acceptingBlockHash: ev.accepting, acceptingDaa: String(ev.acceptingDaa), depthDaa: String(this.virtualDaa - ev.acceptingDaa),
          boundary: '100 DAA and selected-chain acceptance rechecked; project policy, not irreversible finality'});
        it.ev = ev; it.acceptingDaa = ev.acceptingDaa; it.state = 'SETTLED';
      } catch (e) { this.check(scope); if (!CHAIN_MOVED.has(codeOf(e))) throw e; this.displaced(it, scope); }
    }
  }
  async #locate(it, link, scope) {
    let at;
    try { at = await findAcceptance(link, it.txid, it.searchFrom ?? it.record.anchor, {pages: 4, scope,
      progress: from => { this.check(scope); this.note(it, 'SEARCH_PROGRESS', {from}); it.searchFrom = from; }}); }
    catch (e) { if (codeOf(e) === 'SEARCH_CONTINUES') return 'SYNCING'; throw e; }
    if (!at) return null;
    const daa = await this.daaOf(at, link, scope); this.check(scope);
    this.note(it, 'REACCEPTED', {acceptingBlockHash: at, acceptingDaa: String(daa)});
    it.accepting = at; it.acceptingDaa = daa; it.firstAccepting ??= at; it.state = 'CONFIRMING'; return 'ACCEPTED';
  }
  async #maintain(link, scope) {
    const now = this.now(); let n = 0;
    for (const it of this.items.values()) {
      if ((it.foreign && !it.active) || !['PENDING', 'DISPLACED'].includes(it.state) || now - it.since < (it.state === 'PENDING' ? 15000 : 0) || now - it.lastDiag < 8000) continue;
      if (scope.remaining() < 8000 || n++ >= 2) break;
      it.lastDiag = now;
      const located = await this.#locate(it, link, scope); if (located) continue;
      const d = await this.#diagnose(it, link, scope); this.check(scope);
      const seen = it.lastSeen === d; it.lastSeen = d;
      if (it.foreign) { if ((d === 'DRIFT') || (seen && ['DROPPED', 'CONFLICT', 'FUNDING_CONFLICT'].includes(d))) this.#lose(it, 'CONFLICTED', {reason: 'FOREIGN_WITHDRAWN'}); continue; }
      if (d === 'DROPPED' && seen) { it.lastSeen = null; if (!this.sendBlocked) await this.#rebroadcast(it, link, scope); }
      else if (d === 'CONFLICT' && seen) this.#lose(it, 'CONFLICTED');
      else if (d === 'FUNDING_CONFLICT' && seen) this.#lose(it, 'FUNDING_CONFLICTED');
      else if (d === 'DRIFT') this.#lose(it, 'DRIFTED');
    }
  }
  activateForeign(txid) { const it = this.get(txid); need(it?.foreign, 'FOREIGN_TRACK_SCOPE'); it.active = true; }
  #lose(it, state, data = {}) { this.note(it, state, data); it.state = state; }
  #parentLost(it) { return it.record.draft.inputUtxos.some(u => { const p = this.get(u.outpoint.transactionId); return p && (LOST.has(p.state) || p.state === 'SUPERSEDED'); }); }
  async #diagnose(it, link, scope) {
    const inputs = it.record.draft.inputUtxos;
    const result = await diagnoseRecord({action: it.record.draft.action, inputs,
      mempool: () => mempoolEntry(link, it.txid),
      states: async () => (await this.ctx.inputsLive(link, inputs))?.map(live => ({live})),
      parentState: u => this.get(u.outpoint.transactionId)?.state,
      onChain: ON_CHAIN, inspectDrawInMempool: true, check: () => this.check(scope),
      pollAcceptance: async () => { await this.#poll(link, scope); return !!it.accepting; },
      proof: async () => {
        let u; try { [u] = await this.ctx.verifyInputsLive(link, [inputs[0]], {allowDaaDrift: true}); } catch { this.check(scope); return 'UNREACHABLE'; }
        if (u.daa !== inputs[0].daa) return 'DRIFT';
        // Target-only invalidity is NOT authorization to rebuild with different bytes. Stop, keep the intent reserved.
        const w = decodeSpend(it.record.signed.inputs[0].signatureScript, it.profile ?? this.profile, it.record.snapshot.origin);
        const target = hex(w.data.slice(0, 32));
        let b;
        try { b = (await link.call('getBlock', {hash: target, includeTransactions: false})).block; }
        catch (e) { this.check(scope); return 'UNREACHABLE'; } // absence/pruning/transport failure isn't a proof to replace
        this.check(scope);
        need(b?.header?.hash === target, 'BLOCK_HASH_MISMATCH');
        const sink = uint((await link.call('getSinkBlueScore')).blueScore); this.check(scope);
        if (b.verboseData?.isChainBlock === false || sink - uint(b.header.blueScore) >= 432_000n) {
          if (it.foreign) return 'CONFLICT'; // read-only foreign transition withdrew; re-resolve its predecessor
          this.note(it, 'PROOF_CONTEXT_INVALID', {target, isChainBlock: b.verboseData?.isChainBlock, boundary: 'No target-only rebuild authorized; inputs remain reserved'});
          throw this.trip(failure('PROOF_CONTEXT_INVALID'));
        }
        need(b.verboseData?.isChainBlock === true, 'PROOF_CONTEXT_UNKNOWN');
        const blue = uint(b.header.blueScore); need(sink >= blue, 'SINK_BEHIND');
        if (!it.foreign && sink - blue < DRAW_MIN_CONFIRMATIONS) return 'DRAW_WAIT'; // no resend, replace or release
        return null;
      },
    });
    return result.verdict;
  }
  permit(it, scope) {
    this.check(scope); if (this.sendBlocked) throw this.sendBlocked;
    try {
      requireSyncPermission(this.assertSend(it));
      requireRuntimeProfile(this.profile);
      need(!it.foreign && it.record.profileId === PROFILE_ID && it.record.networkGenesis === this.ctx.genesis, 'SEND_SCOPE');
      need(assertSignedRecord(it.record, it.wire) === it.txid, 'SEND_BYTES_CHANGED');
    } catch (e) { throw this.trip(e); }
  }
  async #send(it, link, scope, kind) {
    this.permit(it, scope);
    // Every FIRST/REBROADCAST reads the target encoded in these exact persisted signed bytes (not a stored label).
    const target = drawTargetFromRecord(it.record, this.profile);
    let drawConfirmation = null;
    if (target) {
      try { drawConfirmation = await requireConfirmedDrawTarget(link, target, {maxAgeMs: kind === 'FIRST' ? DRAW_REUSE_MS : 0}); this.check(scope); }
      catch (e) {
        this.check(scope);
        if (codeOf(e) === 'DRAW_TARGET_UNCONFIRMED' || codeOf(e) === 'DRAW_CONFIRMATION_STALE') return {status: 'WAIT', message: codeOf(e)};
        if (codeOf(e) === 'PROOF_CONTEXT_INVALID') {
          this.note(it, 'PROOF_CONTEXT_INVALID', {target: target.hash, boundary: 'No target-only rebuild authorized; inputs remain reserved'});
          throw this.trip(e);
        }
        throw e; // unknown/unreadable is never permission to send
      }
    }
    // After connection, synchronously fence the fresh observation and permission, including fsync latency.
    const beforeSend = () => {
      this.permit(it, scope); if (target) assertFreshDrawConfirmation(drawConfirmation, target, Date.now(), link);
      this.note(it, 'SEND_INTENT', {kind, attempt: it.rebroadcasts, bytes: 'identical signed transaction', ...(target ? {drawConfirmation} : {})});
      this.permit(it, scope); if (target) assertFreshDrawConfirmation(drawConfirmation, target, Date.now(), link);
    };
    try {
      const r = await link.call('submitTransaction', {transaction: txToRpc(it.record.signed), allowOrphan: false}, this.limits.rpcMs, {beforeSend}); this.check(scope);
      const {result, ...details} = classifySubmitResponse(r, it.txid);
      return {status: result, ...details};
    } catch (e) {
      this.check(scope); if (this.sendBlocked) throw this.sendBlocked;
      if (codeOf(e) === 'APPROVAL_EXPIRED' || codeOf(e) === 'SEND_SCOPE' || codeOf(e) === 'SEND_BYTES_CHANGED' || codeOf(e) === 'DRAW_TARGET_HEADER_MISMATCH') throw this.trip(e);
      if (codeOf(e) === 'DRAW_CONFIRMATION_STALE' || codeOf(e) === 'DRAW_TARGET_UNCONFIRMED') return {status: 'WAIT', message: codeOf(e)};
      const {result, ...details} = classifySubmit(e, it.txid, {rpcOnly: true, rejectOtherRpcErrors: true, orphanKnown: false});
      return {status: result, ...details};
    }
  }
  async #rebroadcast(it, link, scope) {
    // Diagnosis may have become stale while another round was checked. Fresh liveness/mempool facts before resend.
    try {
      const live = await this.ctx.verifyInputsLive(link, it.record.draft.inputUtxos, {allowDaaDrift: true});
      this.check(scope);
      if (it.record.draft.action === 'DRAW_AND_PAY' && live[0].daa !== it.record.draft.inputUtxos[0].daa) return;
      if (await mempoolEntry(link, it.txid)) return;
      this.check(scope);
    } catch (e) { this.check(scope); return; } // unknown/spent is never permission to send or release
    // Exhausting a local retry budget is not evidence of network rejection.
    if (it.rebroadcasts >= 30) throw this.trip(failure('REBROADCAST_LIMIT'));
    it.rebroadcasts++;
    const r = await this.#send(it, link, scope, 'REBROADCAST'); this.check(scope);
    this.note(it, 'REBROADCAST', {attempt: it.rebroadcasts, result: r.status, ...(r.message ? {message: r.message} : {})});
    if (r.status === 'REJECTED') { if (++it.rejections >= 2) this.#lose(it, 'REJECTED', {message: r.message}); } else it.rejections = 0;
  }
  broadcast(it) {
    return this.operation(async (link, s) => {
      const r = await this.#send(it, link, s, 'FIRST'); this.check(s);
      this.note(it, r.status === 'SENT' || r.status === 'KNOWN' ? 'SUBMITTED' : 'SUBMIT_' + r.status, r.message ? {message: r.message} : {});
      if (r.status === 'REJECTED') { it.rejections = 1; it.since = 0; } return r.status;
    });
  }
  awaitAccepted(it, verify, {timeoutMs = this.limits.acceptanceMs} = {}) {
    return this.operation(async (link, s) => {
      for (;;) {
        this.check(s);
        if (it.state === 'REJECTED') fail('SUBMIT_REJECTED');
        if (it.state === 'FUNDING_CONFLICTED') fail('FUNDING_CONFLICT');
        if (LOST.has(it.state) || (!ON_CHAIN.has(it.state) && this.#parentLost(it))) fail('ROUND_LOST', {lostTxid: it.txid, lostState: it.state});
        if (it.accepting && ON_CHAIN.has(it.state)) {
          try { const result = await verify(it.record, it.accepting, link); this.check(s);
            const b = (await link.call('getBlock', {hash: result.ev.accepting, includeTransactions: false})).block; this.check(s);
            need(b?.header?.hash === result.ev.accepting && b.verboseData?.isChainBlock === true, 'CHAIN_MOVED');
            return result; }
          catch (e) { this.check(s); if (!CHAIN_MOVED.has(codeOf(e))) throw e; this.displaced(it, s); }
        }
        await pause(300, s.signal);
      }
    }, {timeoutMs, code: 'UNKNOWN'});
  }
  syncTip(live) {
    return this.operation(async (link, s) => {
      const x = live.snapshot, u = {outpoint: x.tip, value: x.value, spk: x.scriptPublicKey, daa: x.utxoDaa, covenantId: x.covenantId};
      let got; try { [got] = await this.ctx.verifyInputsLive(link, [u], {allowDaaDrift: true}); } catch (e) { this.check(s); if (codeOf(e) === 'STALE_INPUT') return {state: 'NOT_LIVE'}; throw e; }
      const it = this.get(x.tip.transactionId); let accepting = it?.accepting ?? live.accepting;
      try {
        const [rpc] = await link.connect(), ev = await acceptedAt(rpc, x.tip.transactionId, accepting); this.check(s);
        if (it?.state === 'SUPERSEDED') return {state: 'SYNCING'};
        if (ev.acceptingDaa !== got.daa) return {state: 'SYNCING'};
        return {state: 'LIVE', utxoDaa: got.daa, accepting: ev.accepting};
      } catch (e) {
        this.check(s); if (!CHAIN_MOVED.has(codeOf(e))) throw e;
        if (it?.accepting) this.displaced(it, s);
        const at = await findAcceptance(link, x.tip.transactionId, it?.record.anchor ?? live.accepting, {scope: s});
        if (!at || (await this.daaOf(at, link, s)) !== got.daa) return {state: 'SYNCING'};
        const [rpc] = await link.connect(); await acceptedAt(rpc, x.tip.transactionId, at); this.check(s);
        if (it) { this.note(it, 'REACCEPTED', {acceptingBlockHash: at, acceptingDaa: String(got.daa)}); it.accepting = at; it.acceptingDaa = got.daa; it.state = 'CONFIRMING'; }
        return {state: 'LIVE', utxoDaa: got.daa, accepting: at};
      }
    });
  }
  #interpretForeign(x, profile, ev) {
    need(referenceTxId(ev.tx) === ev.txid, 'FOREIGN_TXID');
    need(ev.inputs?.every(Boolean) && ev.inputs[0].value === x.value && stable(spk(ev.inputs[0].spk)) === stable(x.scriptPublicKey) && ev.inputs[0].covenantId === x.covenantId, 'FOREIGN_INPUT_CONTEXT');
    return interpretAccepted({...x, utxoDaa: ev.inputs[0].daa, currentDaa: ev.acceptingDaa}, profile, ev.tx, ev.inputs.map(u => u.value));
  }
  observeForeign(round, ev, snapshot, profile, result, scope) {
    this.check(scope); requireRuntimeProfile(profile); ev = structuredClone(ev); snapshot = structuredClone(snapshot);
    result = this.#interpretForeign(snapshot, profile, ev); let it = this.get(ev.txid);
    if (!it) {
      const record = {profileId: PROFILE_ID, networkGenesis: this.ctx.genesis, anchor: ev.accepting, snapshot: structuredClone(snapshot), signed: ev.tx,
        draft: {txid: ev.txid, action: result.action, fee: result.fee, inputUtxos: ev.inputs.map((u, i) => ({...u, spk: spk(u.spk), outpoint: ev.tx.inputs[i].previousOutpoint}))}};
      it = {round, step: '00-FOREIGN_' + ev.txid.toUpperCase(), txid: ev.txid, record, profile, foreign: true, active: false, state: 'CONFIRMING', accepting: ev.accepting,
        acceptingDaa: ev.acceptingDaa, firstAccepting: ev.accepting, ev, since: this.now(), lastDiag: 0, lastSeen: null, searchFrom: ev.accepting};
      this.note(it, 'FOREIGN_OBSERVED', {record, acceptance: ev, interpretation: result}); this.items.set(ev.txid, it);
    } else {
      need(it.foreign && it.round === round, 'FOREIGN_TRACK_SCOPE');
      if (it.accepting !== ev.accepting || !ON_CHAIN.has(it.state)) {
        this.note(it, 'REACCEPTED', {acceptingBlockHash: ev.accepting, acceptingDaa: String(ev.acceptingDaa)});
        Object.assign(it, {accepting: ev.accepting, acceptingDaa: ev.acceptingDaa, state: 'CONFIRMING', ev});
      }
    }
    return {txid: ev.txid, action: result.action, actorKey: result.actorKey, acceptingBlockHash: ev.accepting, acceptingDaa: String(ev.acceptingDaa), terminal: result.terminal,
      ev, next: result.next, snapshot, name: it.step, foreign: true};
  }
  resolveRound(chain, profile, origin, own, {tipSpent = false} = {}) {
    return this.operation(async (link, s) => {
      await this.tick(); this.check(s);
      const state = d => this.get(d.txid)?.state;
      let k = chain.findIndex(d => !ON_CHAIN.has(state(d)));
      if (k >= 0) {
        const st = state(chain[k]);
        if (st === undefined || ['PENDING', 'DISPLACED', 'SUPERSEDED'].includes(st)) return {pending: true};
        if (st === 'FUNDING_CONFLICTED') fail('FUNDING_CONFLICT'); if (st === 'REJECTED') fail('SUBMIT_REJECTED'); need(k >= 1, 'GENESIS_LOST');
      } else {
        if (!tipSpent || !chain.length || !chain.at(-1).ledger) return {healthy: true};
        const last = this.get(chain.at(-1).txid); if (!last?.accepting) return {pending: true};
        const ev = await this.#verify(last, link, s), tip = await this.ctx.inputsLive(link, [{outpoint: {transactionId: last.txid, index: 0}, spk: ev.tx.outputs[0].scriptPublicKey}]); this.check(s);
        if (!tip) return {pending: true}; if (tip[0]) return {healthy: true}; k = chain.length;
      }
      const prev = chain[k - 1], pIt = this.get(prev.txid); need(prev.ledger, 'RESOLVE_LINEAGE'); if (!pIt?.accepting) return {pending: true};
      let pEv;
      try { pEv = await this.#verify(pIt, link, s); } catch (e) { this.check(s); if (!CHAIN_MOVED.has(codeOf(e))) throw e; this.displaced(pIt, s); return {pending: true}; }
      let cur = liveFrom(pEv, prev.ledger, origin, profile);
      if (k < chain.length && this.get(chain[k].txid)?.foreign) {
        const present = await this.ctx.inputsLive(link, [{outpoint: cur.snapshot.tip, spk: cur.snapshot.scriptPublicKey}]); this.check(s);
        // The third-party transition was undone; a current live predecessor is safe to drive normally, not to resend
        // the third party's bytes. Every new own draft still verifies exact value/SPK/CID before signing.
        if (present?.[0]) return {lost: k, reason: 'CONFLICT', live: cur, terminal: null, foreign: []};
      }
      if (k < chain.length && state(chain[k]) === 'DRIFTED') return {lost: k, reason: 'DRIFT', live: cur, terminal: null, foreign: []};
      const foreign = [], round = pIt.round, [rpc] = await link.connect();
      for (let n = 0; n < 300; n++) {
        const key = `${cur.snapshot.tip.transactionId}:${cur.snapshot.tip.index}@${cur.accepting}`;
        const scan = this.scans.get(key) ?? {}; this.scans.set(key, scan);
        let spender;
        try { spender = await findSpender(rpc, cur.snapshot.tip, cur.accepting, {scope: s, scan, progress: v => this.note(pIt, 'SPENDER_PROGRESS', {outpoint: cur.snapshot.tip, from: v.from, blue: String(v.blue)})}); }
        catch (e) { if (codeOf(e) === 'SEARCH_CONTINUES') return {pending: true}; throw e; }
        this.check(s); if (!spender) return {pending: true};
        if (own.has(spender.txid)) {
          if (k < chain.length && spender.txid === chain[k].txid) return {pending: true};
          return {revived: spender.txid, lost: k, foreign};
        }
        const ev = await acceptedAt(rpc, spender.txid, spender.accepting); this.check(s);
        const result = this.#interpretForeign(cur.snapshot, profile, ev);
        foreign.push(this.observeForeign(round, ev, cur.snapshot, profile, result, s));
        if (result.terminal) return {lost: k, reason: 'CONFLICT', live: null, terminal: result.terminal, foreign};
        cur = liveFrom(ev, result.next, origin, profile);
        const tip = await this.ctx.inputsLive(link, [{outpoint: cur.snapshot.tip, spk: cur.snapshot.scriptPublicKey}]); this.check(s);
        if (!tip) return {pending: true};
        if (tip[0]) return {lost: k, reason: 'CONFLICT', live: cur, terminal: null, foreign};
      }
      fail('FOREIGN_CHAIN_TOO_LONG');
    });
  }
  assertSettled() {
    this.check();
    need([...this.items.values()].filter(it => it.state !== 'SUPERSEDED' && (!it.foreign || it.active)).every(it => it.state === 'SETTLED'), 'ROUND_LOST_BEFORE_COMPLETE');
  }
  /** Final gate AFTER auxiliary waits. Short two-header batches, deduplicated by accepting block,
   * under a separate bounded total budget. Never carry successful checks across calls/resumes. */
  confirmCompletion(chains, {timeoutMs = this.limits.completionMs, boundedScan = false} = {}) {
    return this.operation(async (_link, s) => {
      await this.tick(); this.check(s);
      const active = () => [...this.items.values()].filter(it => it.state !== 'SUPERSEDED' && (!it.foreign || it.active));
      const chainIds = new Set(chains.flatMap(c => c.map(d => d.txid)));
      need(active().filter(it => it.record.draft.action !== 'FUND_SPLIT').every(it => chainIds.has(it.txid)), 'COMPLETION_LINEAGE');
      const unsettled = () => active().filter(it => it.state !== 'SETTLED').map(it => it.txid);
      if (unsettled().length) return {lost: unsettled()};
      const items = active(), revision = this.acceptanceRevision;
      const snapshot = items.map(it => ({it, at: it.accepting, daa: it.acceptingDaa}));
      const sameContext = () => {
        this.check(s);
        const current = active();
        need(this.acceptanceRevision === revision && current.length === items.length && current.every((it, i) => it === items[i]) &&
          snapshot.every(x => x.it.state === 'SETTLED' && x.it.accepting === x.at && x.it.acceptingDaa === x.daa), 'COMPLETION_CONTEXT_CHANGED');
      };
      const batch = task => {
        this.check(s);
        // When the outer deadline is what limits this batch, report the outer code deterministically (both timers race).
        const left = s.remaining(), outer = left <= this.limits.stepMs;
        return this.operation(task, {timeoutMs: outer ? left : this.limits.stepMs, code: outer ? 'COMPLETION_TIMEOUT' : 'COMPLETION_BATCH_TIMEOUT'});
      };
      // Independent start cursor: the background poller may advance this.cursor while we scan.
      let cursor = await batch(link => selectedChainAnchor(link));
      sameContext(); // capture a fresh chain tip, not a possibly lagging background pagination cursor
      const groups = new Map();
      for (const x of snapshot) (groups.get(x.at) ?? groups.set(x.at, []).get(x.at)).push(x);
      const entries = [...groups];
      for (let offset = 0; offset < entries.length; offset += 2) {
        sameContext();
        const lost = [];
        await batch(async (link, scope) => {
          const checked = await Promise.allSettled(entries.slice(offset, offset + 2).map(async ([at, rows]) => {
            const b = (await link.call('getBlock', {hash: at, includeTransactions: false})).block;
            this.check(scope); this.check(s);
            need(b?.header?.hash === at, 'BLOCK_HASH_MISMATCH');
            if (b.verboseData?.isChainBlock !== true) {
              for (const x of rows) { this.displaced(x.it, scope); lost.push(x.it.txid); }
            } else for (const x of rows) need(uint(b.header.daaScore) === x.daa, 'ACCEPTANCE_DAA_MISMATCH');
          }));
          const bad = checked.find(r => r.status === 'rejected'); if (bad) throw bad.reason;
        });
        this.check(s); if (lost.length) return {lost};
        sameContext();
      }
      // Immutable accepted bodies were bound at settlement; terminal Full bodies get an additional fresh check.
      for (const chain of chains) {
        sameContext();
        const last = this.get(chain.at(-1)?.txid); need(items.includes(last), 'COMPLETION_LINEAGE');
        try {
          const ev = await batch((link, scope) => this.#verify(last, link, scope));
          sameContext(); last.ev = ev;
        } catch (e) {
          this.check(s); if (!CHAIN_MOVED.has(codeOf(e))) throw e;
          this.displaced(last, s); return {lost: [last.txid]};
        }
      }
      // Close the multi-batch window using our ORIGINAL cursor, not the live background cursor.
      // If an early checked block left the chain, even if subsequently reaccepted, this scan cannot complete.
      // Tests may close this finite verification window at a freshly observed chain point rather than
      // require an empty moving tip (which a remote RPC can never catch on a continuously growing DAG).
      // Default callers retain the existing scan policy. All removal/context/terminal checks stay mandatory.
      const end = boundedScan ? await batch(link => selectedChainAnchor(link)) : null;
      sameContext();
      const visited = new Set();
      for (let page = 0; page < 128; page++) {
        sameContext(); need(!visited.has(cursor), 'COMPLETION_CURSOR_STALLED'); visited.add(cursor);
        const r = await batch(link => link.call('getVirtualChainFromBlock', {startHash: cursor, includeAcceptedTransactionIds: false}));
        this.check(s);
        need(Array.isArray(r?.removedChainBlockHashes) && Array.isArray(r.addedChainBlockHashes), 'CHAIN_QUERY');
        const lost = [];
        for (const hash of r.removedChainBlockHashes) for (const x of groups.get(hash) ?? []) {
          this.displaced(x.it, s); lost.push(x.it.txid);
        }
        if (lost.length) return {lost};
        sameContext();
        need(!end || !r.removedChainBlockHashes.includes(end), 'COMPLETION_CONTEXT_CHANGED');
        if (!r.addedChainBlockHashes.length || (end && (cursor === end || r.addedChainBlockHashes.includes(end)))) return {lost: []};
        cursor = r.addedChainBlockHashes.at(-1);
        need(typeof cursor === 'string' && /^[0-9a-f]{64}$/.test(cursor), 'CHAIN_QUERY');
      }
      fail('COMPLETION_SCAN_LIMIT');
    }, {timeoutMs, code: 'COMPLETION_TIMEOUT'});
  }
  settle({timeoutMs = this.limits.settleMs} = {}) {
    return this.operation(async (_link, s) => {
      for (;;) {
        this.check(s);
        await this.tick(); // each tick remains short; the outer deadline cancels an in-flight tick too
        this.check(s);
        const its = [...this.items.values()].filter(it => it.state !== 'SUPERSEDED' && (!it.foreign || it.active));
        if (its.some(it => it.state === 'REJECTED')) fail('SUBMIT_REJECTED');
        if (its.some(it => it.state === 'FUNDING_CONFLICTED')) fail('FUNDING_CONFLICT');
        const lost = its.filter(it => LOST.has(it.state) || (!ON_CHAIN.has(it.state) && this.#parentLost(it)));
        if (lost.length) return {lost: lost.map(it => it.txid)};
        if (its.every(it => it.state === 'SETTLED')) return {lost: [], settled: new Map(its.map(it => [it.txid, it]))};
        await pause(300, s.signal);
      }
    }, {timeoutMs, code: 'SETTLE_TIMEOUT'});
  }
}
