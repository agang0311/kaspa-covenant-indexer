/** Shared read-only diagnosis. Hooks supply network/proof facts; policies decide what a verdict authorizes.
 * No persistence, signing, rebuilding, reservation release or transport is performed here. */
export async function diagnoseRecord({mempool, states, action, inputs, proof, parentState, pollAcceptance = async () => false,
  onChain, gone = new Set(), inspectDrawInMempool = false, check = () => {}, describeError = () => null}) {
  let m;
  try { m = await mempool(); check(); } catch (e) { check(); return {verdict: 'UNREACHABLE', error: describeError(e)}; }
  if (m && (action !== 'DRAW_AND_PAY' || !inspectDrawInMempool)) return {verdict: 'MEMPOOL'};
  let st;
  try { st = await states(); check(); } catch (e) { check(); return {verdict: 'UNREACHABLE', error: describeError(e)}; }
  if (!st || st.length !== inputs.length) return {verdict: 'UNREACHABLE'};
  if (st.every(x => x.live)) {
    if (action === 'DRAW_AND_PAY') {
      const result = await proof(st); check();
      if (result) return typeof result === 'string' ? {verdict: result} : result;
    }
    return {verdict: m ? 'MEMPOOL' : 'DROPPED'};
  }
  const missing = inputs.filter((_, i) => !st[i].live);
  if (missing.some(u => gone.has(parentState(u)))) return {verdict: 'PARENT_LOST'};
  if (missing.some(u => { const p = parentState(u); return p != null && !onChain.has(p); })) return {verdict: 'PARENT_PENDING'};
  if (await pollAcceptance()) { check(); return {verdict: 'ACCEPTED'}; }
  check();
  return {verdict: missing.some(u => !u.covenantId) ? 'FUNDING_CONFLICT' : 'CONFLICT'};
}

/** A response mismatch says nothing about whether the sent transaction entered the network. */
export function classifySubmitResponse(response, txid) {
  return response?.transactionId === txid ? {result: 'SENT'}
    : {result: 'NO_RESPONSE', message: 'SUBMIT_TXID_MISMATCH'};
}

/** Explicit submission policy: CLI rejects other RpcErrors; Web requires a named txid rejection.
 * These are not interchangeable release permissions. Caller retains its own repeated-observation gate. */
export function classifySubmit(err, txid, {rpcOnly = false, rejectOtherRpcErrors = false, orphanKnown = true} = {}) {
  if (rpcOnly && err?.name !== 'RpcError') return {result: 'NO_RESPONSE'};
  const m = String(err?.message ?? err).slice(0, 400);
  if (/already (been )?accepted|was already accepted|already in the mempool/i.test(m) || (orphanKnown && /already in the orphan pool/i.test(m))) return {result: 'KNOWN', message: m};
  if (/orphan|lacking a matching UTXO|missing from current utxo|sequence lock|already spent by transaction/i.test(m)) return {result: 'WAIT', message: m};
  if (rejectOtherRpcErrors || (/Rejected transaction/i.test(m) && m.includes(txid))) return {result: 'REJECTED', message: m};
  return {result: 'NO_RESPONSE', message: m};
}
