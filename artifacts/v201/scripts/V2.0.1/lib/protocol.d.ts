import { type Spk } from './covenant-id.js';
import * as S from './state.js';
export declare const ACTIONS: {
    readonly BUY: 1;
    readonly CLOSE: 2;
    readonly DRAW_AND_PAY: 4;
    readonly TIMEOUT_REFUND: 9;
    readonly REFUND: 10;
};
export type Action = keyof typeof ACTIONS;
export type Operation = {
    action: Action;
    actorKey: string;
    quantity?: number;
    opening?: Uint8Array;
    accessor?: {
        blockHash: string;
        sequenceCommitment: string;
    };
};
export interface Payment {
    value: bigint;
    spk: Spk;
    role: 'WINNER' | 'CREATOR' | 'EXECUTOR' | 'BUYER_REFUND' | 'CHANGE';
}
export interface Transition {
    next: S.Ledger | null;
    terminal: 'PAID' | 'EMPTY' | 'REFUNDED' | null;
    payments: Payment[];
    data: Uint8Array;
    lockTime: bigint;
    sequence: bigint;
    requiredExternal: bigint;
    foreignTail: Uint8Array;
    draw: Draw | null;
}
/** Authenticated PASS-A draw context (V2 seed formula, unchanged). Anchor = the spent SEALED outpoint and its UTXO DAA. */
export interface Draw {
    seed: string;
    boundaryDaa: bigint;
    anchorDaa: bigint;
    anchorTxId: string;
    anchorIndex: number;
    targetHash: string;
    targetSeq: string;
}
export interface Sample {
    value: bigint;
    limit: bigint;
    ticket: number | null;
}
export declare function authenticateDraw(x: S.Snapshot, s: S.Ledger, proof: Uint8Array, accessor: {
    blockHash: string;
    sequenceCommitment: string;
}): Draw;
/** Single sample; the preimage keeps the former 8-byte zero counter (no redraw in V2.0.1). */
export declare function sample(seed: string, sold: number): Sample;
/** Zero-based winning record index for a zero-based ticket. */
export declare function recordOfTicket(s: S.Ledger, ticket: number): number;
/** Complete off-chain draw mirror: authenticated seed -> sample -> ticket/record. Throws SAMPLE_REJECTED (round stays SEALED until timeout). */
export declare function drawWinner(x: S.Snapshot, s: S.Ledger, proof: Uint8Array, accessor: {
    blockHash: string;
    sequenceCommitment: string;
}): {
    draw: Draw;
    sample: Sample;
    ticket: number;
    record: number;
};
/** TIMEOUT_REFUND maturity: the spent SEALED input's DAA + TIMEOUT_DELAY (V2.0.1 persists no anchor). */
export declare function timeoutDaa(x: S.Snapshot, s: S.Ledger): bigint;
/** Executor pool of one REFUND batch: k * REFUND_FEE, plus the fixed REFUND_FEE deducted from the deposit in the final batch. */
export declare function refundBatch(s: S.Ledger): {
    k: number;
    last: boolean;
    pool: bigint;
    deposit: bigint;
};
export declare function availableActions(x: S.Snapshot, p: S.Profile): Action[];
/** The supplied network fee must later equal actual input-output difference. */
export declare function transition(x: S.Snapshot, p: S.Profile, op: Operation, fee: bigint, external?: bigint): Transition;
/** PROVISIONAL sizing envelope: the reviewed V2 (Profile 7aaf76fe) formulas are reused unchanged as an upper bound.
 * V2.0.1 scripts are smaller; every targeted V2.0.1 SCRIPT_VM measurement (contracts/V2.0.1/artifacts/vm) stays
 * below these formulas. A full V2.0.1 calibration matrix must refit them before budgetProfileId is set. */
export declare function actionUnits(action: Action, s: S.Ledger): number;
export declare const BUDGET_MARGIN = 3, GENESIS_INPUT_BUDGET = 10, FUNDING_INPUT_BUDGET = 10;
export declare function actionBudget(action: Action | 'GENESIS', s?: S.Ledger | null): number;
export declare const budgetOf: typeof actionBudget;
