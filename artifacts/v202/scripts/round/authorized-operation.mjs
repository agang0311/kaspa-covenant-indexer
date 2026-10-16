/** CLI/local signer adapter. Build-time input verification is supplied by the caller's state/funding discovery.
 * No extra live UTXO RPC before/after signing. A locked synchronous experiment journal remains mandatory. */
import {OperationEngine} from './operation-engine.mjs';
import {JournalIntentStore} from '../store/intent-store.mjs';
import {assertDraft} from '../core/lib/builders.js';
import {referenceTxId} from '../core/lib/transaction.js';
import {stable} from '../core/lib/bytes.js';
import {signDraft} from '../wallet/local.mjs';
import {signedWire} from '../tracking/signed-record.mjs';
import {selectedChainAnchor} from '../chain/selected-chain.mjs';
import {requireConfirmedDrawRecord, DRAW_REUSE_MS} from './draw-confirmations.mjs';
import {failure, requireSyncPermission} from '../chain/operation.mjs';
import {PROFILE_ID, CONTRACT_VERSION, requireRuntimeProfile} from '../contract/runtime.mjs';

export async function prepareAuthorizedOperation({link, ctx, profile, profileId, draft, snapshot, wallet,
  journal, context, check, trip, anchor = null}) {
  if (typeof check !== 'function' || typeof trip !== 'function') throw failure('OPERATION_ADAPTER_REQUIRED');
  const permit = () => {
    requireRuntimeProfile(profile);
    if (profileId !== PROFILE_ID) throw failure('RECORD_SCOPE');
    requireSyncPermission(check());
  };
  permit(); assertDraft(draft); draft.txid = referenceTxId(draft.transaction);
  const approved = structuredClone(draft.transaction), inputs = stable(draft.inputUtxos);
  let drawConfirmation = null;
  const store = new JournalIntentStore(journal);
  return OperationEngine.prepare({check: permit,
    beforeSign: async () => {
      assertDraft(draft);
      drawConfirmation = await requireConfirmedDrawRecord(link, {draft, snapshot}, profile, {maxAgeMs: DRAW_REUSE_MS});
    },
    sign: () => signDraft(draft, wallet.key, i => {
      permit();
      if (typeof wallet.signInput !== 'function') throw failure('SIGNER_REQUIRED');
      return wallet.signInput(draft, i);
    }),
    afterSign: signed => {
      assertDraft(draft);
      const normalized = structuredClone(signed);
      for (const i of draft.authorizedInputIndices) normalized.inputs[i].signatureScript = approved.inputs[i].signatureScript;
      if (signedWire(normalized) !== signedWire(approved) || stable(draft.inputUtxos) !== inputs) throw failure('SIGNED_FIELDS_CHANGED');
    },
    makeRecord: async signed => {
      // Reuse the guard's already verified search location; a location never substitutes for acceptance.
      const checkpoint = anchor ?? await selectedChainAnchor(link, null, {maxAgeMs: 60000});
      if (!/^[0-9a-f]{64}$/.test(checkpoint)) throw failure('ANCHOR_REQUIRED');
      permit();
      return {contractVersion: CONTRACT_VERSION, profileId, networkGenesis: ctx.genesis,
        draft, signed, snapshot, anchor: checkpoint, drawConfirmation};
    },
    context,
    persistence: {durability: store.durability, persist(where, record) {
      try { permit(); return store.persist(where, record); } catch (e) { trip(e); throw e; }
    }},
  });
}
