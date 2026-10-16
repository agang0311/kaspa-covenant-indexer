/** Deterministic action preparation, contract V2.0.2 source candidate (on-chain actions only; no networkGenesis).
 * The draw is a transient result (seed, sample, ticket) computed for DRAW_AND_PAY; it is never a Ledger. */
import { ascii, cat, check, fromLe, hex, integer, le, unhex } from './bytes.js';
import { blake2b256 } from './hashes.js';
import { domainHash } from './blake3.js';
import { p2pk } from './covenant-id.js';
import * as S from './state.js';
export const ACTIONS = { BUY: 1, CLOSE: 2, DRAW_AND_PAY: 4, TIMEOUT_REFUND: 9, REFUND: 10 };
const Z = new Uint8Array();
function openingNumber(proof, offset) { const n = fromLe(proof.slice(offset, offset + 8)); check(n < 1n << 63n, 'PASS_A_INTEGER'); return n; }
function openSeq(proof, base, parent) { const ctx = domainHash('SeqCommitMergesetContext', proof.slice(base + 64, base + 88)); const pc = domainHash('SeqCommitmentMerkleBranchHash', cat(ctx, proof.slice(base + 32, base + 64))); const root = domainHash('SeqCommitmentMerkleBranchHash', cat(proof.slice(base, base + 32), pc)); return domainHash('SeqCommitmentMerkleBranchHash', cat(parent, root)); }
export function authenticateDraw(x, s, proof, accessor) {
    check(s.phase === S.Phase.SEALED, 'NOT_SEALED');
    check(proof.length === 240, 'PASS_A_LENGTH');
    check(x.utxoDaa >= 0n && x.utxoDaa < S.DAA_LIMIT - S.TIMEOUT_DELAY, 'ANCHOR_DAA');
    const boundary = x.utxoDaa + S.DRAW_DELAY;
    check(x.currentDaa >= boundary, 'DRAW_TOO_EARLY');
    const pd = openingNumber(proof, 224), td = openingNumber(proof, 104);
    check(pd < boundary && td >= boundary && td < S.DAA_LIMIT, 'PASS_A_FIRST_CROSSING');
    check(openingNumber(proof, 112) > openingNumber(proof, 232), 'PASS_A_BLUE_ORDER');
    openingNumber(proof, 96);
    openingNumber(proof, 216);
    check(hex(proof.slice(0, 32)) === accessor.blockHash, 'PASS_A_TARGET_HASH');
    const p = openSeq(proof, 152, proof.slice(120, 152)), t = openSeq(proof, 32, p);
    check(hex(t) === accessor.sequenceCommitment, 'PASS_A_COMMITMENT');
    const raw = S.encodeLedger(s), frozen = blake2b256(cat(raw.slice(S.OFFSET_TICKET_PRICE, S.OFFSET_REFUND_CURSOR), s.directory));
    const seed = hex(blake2b256(cat(ascii('KASWIN_V2_DRAW'), unhex(x.covenantId, 32), unhex(x.tip.transactionId, 32), le(BigInt(x.tip.index), 4), frozen, le(boundary, 8), proof.slice(0, 32), t)));
    return { seed, boundaryDaa: boundary, anchorDaa: x.utxoDaa, anchorTxId: x.tip.transactionId, anchorIndex: x.tip.index, targetHash: hex(proof.slice(0, 32)), targetSeq: hex(t) };
}
/** Single sample; the preimage keeps the former 8-byte zero counter (no redraw in V2.0.2). */
export function sample(seed, sold) {
    integer(sold, 3, S.MAX_TICKETS);
    const digest = blake2b256(cat(ascii('KASWIN_V2_SAMPLE'), unhex(seed, 32), new Uint8Array(8)));
    const value = fromLe(digest.slice(0, 7)), space = 1n << 56n, limit = space - space % BigInt(sold);
    return { value, limit, ticket: value < limit ? Number(value % BigInt(sold)) : null };
}
/** Zero-based winning record index for a zero-based ticket. */
export function recordOfTicket(s, ticket) { integer(ticket, 0, s.sold - 1); const i = S.records(s).findIndex(p => ticket < p.end); check(i >= 0, 'WINNER_NOT_FOUND'); return i; }
/** Complete off-chain draw mirror: authenticated seed -> sample -> ticket/record. Throws SAMPLE_REJECTED (round stays SEALED until timeout). */
export function drawWinner(x, s, proof, accessor) {
    const draw = authenticateDraw(x, s, proof, accessor), smp = sample(draw.seed, s.sold);
    check(smp.ticket !== null, 'SAMPLE_REJECTED');
    return { draw, sample: smp, ticket: smp.ticket, record: recordOfTicket(s, smp.ticket) };
}
/** TIMEOUT_REFUND maturity: the spent SEALED input's DAA + TIMEOUT_DELAY (KW21 persists no anchor). */
export function timeoutDaa(x, s) { check(s.phase === S.Phase.SEALED, 'NOT_SEALED'); const d = x.utxoDaa; check(d >= 0n && d < S.DAA_LIMIT - S.TIMEOUT_DELAY, 'ANCHOR_DAA'); return d + S.TIMEOUT_DELAY; }
/** Executor pool of one REFUND batch: k * REFUND_FEE, plus the fixed REFUND_FEE deducted from the deposit in the final batch. */
export function refundBatch(s) {
    check(s.phase === S.Phase.REFUNDING, 'NOT_REFUNDING');
    const k = Math.min(S.MAX_REFUND_RECORDS, s.purchaseCount - s.cursor), last = s.cursor + k === s.purchaseCount;
    check(k > 0, 'NO_REFUND_RECORDS');
    return { k, last, pool: BigInt(k) * S.REFUND_FEE + (last ? S.REFUND_FEE : 0n), deposit: last ? S.DEPOSIT - S.REFUND_FEE : 0n };
}
export function availableActions(x, p) {
    const s = S.verifySnapshot(x, p);
    if (s.phase === S.Phase.OPEN) {
        const a = [];
        if (s.sold < s.config.ticketCap && s.purchaseCount < s.config.purchaseCap)
            a.push('BUY');
        if (s.sold === s.config.ticketCap || s.purchaseCount === s.config.purchaseCap || x.currentDaa >= s.config.closeEligibleDaa)
            a.push('CLOSE');
        return a;
    }
    if (s.phase === S.Phase.REFUNDING)
        return ['REFUND'];
    const a = [];
    if (x.currentDaa >= x.utxoDaa + S.DRAW_DELAY)
        a.push('DRAW_AND_PAY');
    if (x.currentDaa >= timeoutDaa(x, s))
        a.push('TIMEOUT_REFUND');
    return a;
}
/** The supplied network fee must later equal actual input-output difference. */
export function transition(x, p, op, fee, external = 0n) {
    const s = S.verifySnapshot(x, p);
    unhex(op.actorKey, 32);
    check(fee > 0n && fee <= S.MAX_PAY_FEE, 'NETWORK_FEE');
    check(external >= 0n && external < S.VALUE_LIMIT, 'EXTERNAL_VALUE');
    check(availableActions(x, p).includes(op.action), 'ACTION_NOT_AVAILABLE');
    let next = null, terminal = null, lockTime = 0n, sequence = 0n, data = Z, foreignTail = Z, requiredExternal = fee, draw = null;
    const payments = [];
    const pay = (value, key, role) => { check(value > 0n && value <= S.VALUE_LIMIT, 'PAYMENT_VALUE'); payments.push({ value, spk: p2pk(key), role }); };
    switch (op.action) {
        case 'BUY': {
            check(op.quantity !== undefined, 'QUANTITY_MISSING');
            next = S.appendPurchase(s, op.quantity, op.actorKey);
            data = le(BigInt(op.quantity), 4);
            requiredExternal = BigInt(op.quantity) * s.config.ticketPrice + fee;
            break;
        }
        case 'CLOSE': {
            if (s.sold < s.config.ticketCap && s.purchaseCount < s.config.purchaseCap)
                lockTime = s.config.closeEligibleDaa - 1n;
            if (s.sold === 0) {
                terminal = 'EMPTY';
                pay(S.DEPOSIT, s.ownerKey, 'CREATOR');
            }
            else {
                next = { ...s, phase: s.sold >= s.config.minTickets ? S.Phase.SEALED : S.Phase.REFUNDING };
                foreignTail = p.frames[S.phaseModule(next.phase)].tail;
            }
            break;
        }
        case 'DRAW_AND_PAY': {
            check(op.opening && op.accessor, 'DRAW_PROOF_MISSING');
            check(external === 0n, 'PAY_HAS_EXTERNAL_INPUT');
            const w = drawWinner(x, s, op.opening, op.accessor), r = S.records(s)[w.record];
            draw = w.draw;
            sequence = S.DRAW_DELAY;
            const prize = BigInt(s.sold) * s.config.ticketPrice - S.FINALIZER - fee;
            check(prize >= S.MIN_PRICE, 'WINNER_MINIMUM');
            pay(prize, r.key, 'WINNER');
            pay(S.DEPOSIT, s.ownerKey, 'CREATOR');
            pay(S.FINALIZER, op.actorKey, 'EXECUTOR');
            data = cat(op.opening, le(BigInt(w.record), 4));
            terminal = 'PAID';
            requiredExternal = 0n;
            break;
        }
        case 'TIMEOUT_REFUND': {
            check(x.currentDaa >= timeoutDaa(x, s), 'TIMEOUT_EARLY');
            sequence = S.TIMEOUT_DELAY;
            next = { ...s, phase: S.Phase.REFUNDING };
            foreignTail = p.frames.refunding.tail;
            break;
        }
        case 'REFUND': {
            const b = refundBatch(s), end = s.cursor + b.k, r = S.records(s);
            for (let i = s.cursor; i < end; i++) {
                const q = r[i];
                pay(BigInt(q.count) * s.config.ticketPrice - S.REFUND_FEE, q.key, 'BUYER_REFUND');
            }
            if (b.last) {
                terminal = 'REFUNDED';
                pay(b.deposit, s.ownerKey, 'CREATOR');
            }
            else
                next = { ...s, cursor: end };
            check(external + b.pool > fee, 'REFUND_FEE_POOL');
            pay(external + b.pool - fee, op.actorKey, 'EXECUTOR');
            requiredExternal = fee >= b.pool ? fee - b.pool + 1n : 0n;
            break;
        }
    }
    if (op.action !== 'DRAW_AND_PAY' && op.action !== 'REFUND') {
        check(external >= requiredExternal, 'INSUFFICIENT_FUNDING');
        if (external > requiredExternal)
            pay(external - requiredExternal, op.actorKey, 'CHANGE');
    }
    if (next)
        S.validateLedger(next);
    check((next ? 1 : 0) + payments.length <= S.OUTPUT_LIMIT[S.phaseModule(s.phase)], 'OUTPUT_LIMIT');
    return { next, terminal, payments, data, lockTime, sequence, requiredExternal, foreignTail, draw };
}
/** PROVISIONAL sizing envelope inherited from V2.0.1 source, NOT calibrated for V2.0.2.
 * Historical V2.0.1 logs do not establish execution cost for newly compiled templates.
 * Keep approval blocked until tail equivalence and the new Profile's budget evidence are reviewed. */
export function actionUnits(action, s) {
    const pc = s.purchaseCount;
    switch (action) {
        case 'BUY': return 204300 + 4090 * pc;
        case 'CLOSE': return pc === 0 ? 144800 : s.sold >= s.config.minTickets ? 246700 + 3750 * pc : 300800 + 3760 * pc;
        case 'DRAW_AND_PAY': return 230000 + 7000 * pc;
        case 'TIMEOUT_REFUND': return 290800 + 2850 * pc;
        case 'REFUND': {
            const k = Math.min(S.MAX_REFUND_RECORDS, pc - s.cursor), last = s.cursor + k === pc;
            return s.cursor === 0 ? (last ? 221200 + 3420 * k + 71 * k * k : 371100 + 5330 * pc) : (last ? 230100 + 3240 * pc + 950 * k + 72 * pc * k : 379500 + 6120 * pc);
        }
        default:
            check(false, 'NO_CALIBRATED_BUDGET');
            return 0;
    }
}
export const BUDGET_MARGIN = 3, GENESIS_INPUT_BUDGET = 10, FUNDING_INPUT_BUDGET = 10;
export function actionBudget(action, s) {
    if (action === 'GENESIS')
        return GENESIS_INPUT_BUDGET;
    check(s !== undefined && s !== null, 'LEDGER_REQUIRED_FOR_BUDGET');
    const units = actionUnits(action, s);
    return Math.max(0, Math.ceil((units - 9999) / 10000)) + BUDGET_MARGIN;
}
export const budgetOf = actionBudget;
//# sourceMappingURL=protocol.js.map