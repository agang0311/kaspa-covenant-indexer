// KW21 target-confirmation helpers for V2.0.2. Shared by CLI and TN10 Web.
// NOT a covenant consensus condition or a transaction-acceptance proof.
// cfafeb4 service.rs keeps distance > minConfirmationCount (not >=); check actual T explicitly.
import {decodeSpend, targetSeqOf} from '../core/lib/accepted.js';
import {fromLe, hex} from '../core/lib/bytes.js';
import {uint} from '../chain/json.mjs';
import {LIMITS, failure, pause, queryContext, sameQueryContext} from '../chain/operation.mjs';

export const DRAW_MIN_CONFIRMATIONS = 100n;
export const DRAW_MAX_TARGET_DEPTH = 432_000n - 1000n; // preserve the existing accessor safety margin
export const DRAW_POLICY = Object.freeze({metric: 'sink.blueScore - target.blueScore', minConfirmations: '100',
  boundary: 'Client policy for the actual PASS-A first-crossing target; not irreversible finality'});
const need = (v, code) => { if (!v) throw failure(code); };
const confirmationCache = new WeakMap(), provenance = new WeakMap();
const targetKey = t => `${t.hash}/${t.sequenceCommitment}/${t.blueScore}`;
const unchanged = (a, b) => a.node === b.node && a.socket === b.socket && a.epoch === b.epoch && a.revision === b.revision;
export const DRAW_REUSE_MS = 5000;

/** Read the actual authenticated proof context, never a caller-supplied confirmation label. */
export function drawTargetFromOpening(opening) {
  need(opening instanceof Uint8Array && opening.length === 240, 'PASS_A_LENGTH');
  const blueScore = fromLe(opening.slice(112, 120));
  need(blueScore < 1n << 63n, 'PASS_A_INTEGER');
  return {hash: hex(opening.slice(0, 32)), sequenceCommitment: targetSeqOf(opening), blueScore};
}
export function drawTargetFromRecord(record, profile) {
  if (!record.snapshot) {
    need(!record.draft.inputUtxos[0]?.covenantId && record.draft.action !== 'DRAW_AND_PAY', 'SEALED_SNAPSHOT_REQUIRED');
    return null;
  }
  const tx = record.signed ?? record.draft.transaction;
  const w = decodeSpend(tx.inputs[0].signatureScript, profile, record.snapshot.origin);
  need(w.action === record.draft.action, 'DRAFT_ACTION_MISMATCH'); // unsigned metadata must not disguise a DRAW as CLOSE
  if (w.action !== 'DRAW_AND_PAY') return null;
  need(w.data.length === 244, 'DRAW_PROOF_MISSING');
  return drawTargetFromOpening(w.data.slice(0, 240));
}

/** Fresh by default; explicit short reuse only comes from this verifier, on the same open connection/reorg revision. */
export async function requireConfirmedDrawTarget(link, target, {maxAgeMs = 0} = {}) {
  need(Number.isSafeInteger(maxAgeMs) && maxAgeMs >= 0 && maxAgeMs <= DRAW_REUSE_MS, 'DRAW_REUSE_AGE');
  await link.connect?.();
  const context = queryContext(link), key = targetKey(target), now = Date.now();
  const previous = confirmationCache.get(context.node)?.get(key);
  if (maxAgeMs && previous?.observation && sameQueryContext(previous.context, context) &&
      now >= previous.observation.observedAt && now - previous.observation.observedAt < maxAgeMs) {
    assertFreshDrawConfirmation(previous.observation, target, now, link);
    return previous.observation;
  }
  const cache = confirmationsCache(context.node), attempt = {context};
  cache.set(key, attempt); // a fresh failure/late result must never revive an older positive observation
  try {
    const observedAt = now; // covers header membership and sink reads
    const b = (await link.call('getBlock', {hash: target.hash, includeTransactions: false})).block;
    need(b?.header?.hash === target.hash, 'BLOCK_HASH_MISMATCH');
    if (b.verboseData?.isChainBlock === false) throw failure('PROOF_CONTEXT_INVALID');
    need(b.verboseData?.isChainBlock === true, 'PROOF_CONTEXT_UNKNOWN');
    const blue = uint(b.header.blueScore);
    need(blue === target.blueScore && b.header.acceptedIdMerkleRoot === target.sequenceCommitment, 'DRAW_TARGET_HEADER_MISMATCH');
    const sink = uint((await link.call('getSinkBlueScore')).blueScore);
    need(sink >= blue, 'SINK_BEHIND');
    const confirmations = sink - blue;
    need(confirmations < DRAW_MAX_TARGET_DEPTH, 'PROOF_CONTEXT_INVALID');
    const observation = Object.freeze({...target, sinkBlueScore: sink, confirmations, observedAt});
    need(unchanged(context, queryContext(link)) && cache.get(key) === attempt, 'DRAW_CONFIRMATION_STALE');
    if (confirmations < DRAW_MIN_CONFIRMATIONS) throw Object.assign(failure('DRAW_TARGET_UNCONFIRMED'), {drawConfirmation: observation});
    provenance.set(observation, {context, key});
    attempt.observation = observation;
    assertFreshDrawConfirmation(observation, target, Date.now(), link);
    return observation;
  } catch (e) {
    if (cache.get(key) === attempt) cache.delete(key);
    throw e;
  }
}
function confirmationsCache(node) {
  let cache = confirmationCache.get(node);
  if (!cache) { cache = new Map(); confirmationCache.set(node, cache); }
  // Keep this read-only observation cache bounded even across many independent rounds.
  if (cache.size >= 64) cache.delete(cache.keys().next().value);
  return cache;
}
export async function requireConfirmedDrawRecord(link, record, profile, options) {
  const target = drawTargetFromRecord(record, profile);
  return target ? requireConfirmedDrawTarget(link, target, options) : null;
}

/** Synchronous transport fence after connection and journal fsync; async reads cannot be atomic with consensus. */
export function assertFreshDrawConfirmation(observation, target, now = Date.now(), link = null) {
  if (link) {
    const source = provenance.get(observation), context = source?.context;
    need(context && unchanged(context, queryContext(link)) && (!context.socket || context.socket.readyState === 1) &&
      confirmationCache.get(context.node)?.get(source.key)?.observation === observation, 'DRAW_CONFIRMATION_STALE');
  }
  need(observation?.hash === target.hash && observation.sequenceCommitment === target.sequenceCommitment && observation.blueScore === target.blueScore,
    'DRAW_TARGET_HEADER_MISMATCH');
  need(observation.confirmations >= DRAW_MIN_CONFIRMATIONS && observation.confirmations < DRAW_MAX_TARGET_DEPTH, 'DRAW_TARGET_UNCONFIRMED');
  need(Number.isSafeInteger(observation.observedAt) && now >= observation.observedAt && now - observation.observedAt < LIMITS.rpcMs, 'DRAW_CONFIRMATION_STALE');
}

/** Single-round wait inside a cancellable 90s guard.operation; only insufficient depth is retryable. */
export async function waitForConfirmedDraw(task, scope, {check = () => {}, onWait = () => {}, pollMs = 2000} = {}) {
  for (;;) {
    scope.check(); check();
    try { const proof = await task(); scope.check(); check(); return proof; }
    catch (e) {
      scope.check(); check();
      if (e?.safeCode !== 'DRAW_TARGET_UNCONFIRMED') throw e;
      onWait(e.drawConfirmation);
      await pause(pollMs, scope.signal);
    }
  }
}
