/** Shared node record verification. Context supplies network/fee/registry facts and receipt schema, never authority. */
import {PROFILE_ID, CONTRACT_VERSION, requireRuntimeProfile} from '../contract/runtime.mjs';
import {assertDraft} from '../core/lib/builders.js';
import {referenceTxId} from '../core/lib/transaction.js';
import {interpretAccepted} from '../core/lib/accepted.js';
import {verifyGenesisAnnouncement} from '../core/lib/genesis-discovery.js';
import {acceptedAt, matchesDraft} from '../chain/chain.mjs';
import {findAcceptance} from '../tracking/reorg-guard.mjs';
import {checkRegistration} from './registration.mjs';
import {failure} from '../chain/operation.mjs';
const need = (ok, code) => { if (!ok) throw failure(code); };

export async function verifyRecord(link, profile, record, acceptingHint, ctx) {
  requireRuntimeProfile(profile);
  need(ctx && record.profileId === PROFILE_ID && record.networkGenesis === ctx.genesis &&
    record.contractVersion === CONTRACT_VERSION, 'RECORD_SCOPE');
  assertDraft(record.draft);
  need(referenceTxId(record.draft.transaction) === record.draft.txid && referenceTxId(record.signed) === record.draft.txid, 'RECORD_TXID');
  const [rpc] = await link.connect();
  const accepting = acceptingHint ?? await findAcceptance(link, record.draft.txid, record.anchor);
  need(accepting, 'UNKNOWN');
  const ev = await acceptedAt(rpc, record.draft.txid, accepting), fee = matchesDraft(ev, record.draft);
  need(fee > 0n && fee <= ctx.maxFee, 'ACTUAL_FEE_CAP');
  let next = null, terminal = null, draw = null;
  const split = record.draft.action === 'FUND_SPLIT';
  if (split) {
    need(!record.snapshot && ev.tx.outputs.every(o => !o.covenant) && ev.inputs.every(u => !u.covenantId), 'SPLIT_COVENANT');
  } else if (record.snapshot) {
    const result = interpretAccepted({...record.snapshot, utxoDaa: ev.inputs[0].daa, currentDaa: ev.acceptingDaa}, profile, ev.tx, ev.inputs.map(u => u.value));
    next = result.next; terminal = result.terminal; draw = result.winner ? {winner: result.winner, draw: result.draw} : null;
  } else {
    const out = ev.tx.outputs[0];
    next = verifyGenesisAnnouncement({accepted: true, payload: ev.tx.payload, authorizingOutpoint: ev.tx.inputs[0].previousOutpoint,
      outputIndex: 0, value: out.value, spk: out.scriptPublicKey, covenantId: out.covenant?.covenantId,
      authorizingInput: out.covenant?.authorizingInput, covenantOutputIndices: ev.tx.outputs.flatMap((o, i) => o.covenant?.covenantId === out.covenant?.covenantId ? [i] : [])}, profile);
    checkRegistration(ev.tx, next.ownerKey, ctx.registry.spk);
  }
  const receipt = {schema: ctx.schemas.accepted, status: 'ACCEPTED', contractVersion: CONTRACT_VERSION, ...(split ? {kind: 'FUND_SPLIT'} : {}),
    txid: ev.txid, profileId: PROFILE_ID, networkGenesis: ctx.genesis,
    intentSha256: record.intentSha256, acceptingBlockHash: ev.accepting, acceptingDaa: ev.acceptingDaa, confirmations: ev.confirmations,
    actualFeeSompi: fee, computeMass: ev.computeMass, storageMass: ev.tx.storageMass, transaction: ev.tx, inputContext: ev.inputs,
    terminal, draw, checkedAt: new Date().toISOString(), boundary: 'Current selected-chain observation, not irreversible finality; null mass means unavailable'};
  return {ev, next, terminal, draw, receipt};
}

export function interpretVerifiedReceipt(record, receipt, profile, ctx) {
  requireRuntimeProfile(profile);
  need(ctx && record.profileId === PROFILE_ID && record.networkGenesis === ctx.genesis &&
    record.contractVersion === CONTRACT_VERSION, 'RECORD_SCOPE');
  assertDraft(record.draft);
  const ev = {
    txid: record.draft.txid,
    accepting: receipt.acceptingBlockHash,
    acceptingDaa: BigInt(receipt.acceptingDaa),
    confirmations: BigInt(receipt.confirmations ?? 100),
    computeMass: receipt.computeMass ? BigInt(receipt.computeMass) : null,
    tx: record.signed,
    inputs: record.draft.inputUtxos
  };
  const fee = matchesDraft(ev, record.draft);
  need(fee > 0n && fee <= ctx.maxFee, 'ACTUAL_FEE_CAP');
  let next = null, terminal = null, draw = null;
  const split = record.draft.action === 'FUND_SPLIT';
  if (split) {
    need(!record.snapshot && ev.tx.outputs.every(o => !o.covenant) && ev.inputs.every(u => !u.covenantId), 'SPLIT_COVENANT');
  } else if (record.snapshot) {
    const result = interpretAccepted({...record.snapshot, utxoDaa: ev.inputs[0].daa, currentDaa: ev.acceptingDaa}, profile, ev.tx, ev.inputs.map(u => u.value));
    next = result.next; terminal = result.terminal; draw = result.winner ? {winner: result.winner, draw: result.draw} : null;
  } else {
    const out = ev.tx.outputs[0];
    next = verifyGenesisAnnouncement({accepted: true, payload: ev.tx.payload, authorizingOutpoint: ev.tx.inputs[0].previousOutpoint,
      outputIndex: 0, value: out.value, spk: out.scriptPublicKey, covenantId: out.covenant?.covenantId,
      authorizingInput: out.covenant?.authorizingInput, covenantOutputIndices: ev.tx.outputs.flatMap((o, i) => o.covenant?.covenantId === out.covenant?.covenantId ? [i] : [])}, profile);
    checkRegistration(ev.tx, next.ownerKey, ctx.registry.spk);
  }
  return {ev, next, terminal, draw, receipt};
}
