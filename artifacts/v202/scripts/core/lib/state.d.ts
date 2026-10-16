import { type Outpoint, type Spk } from './covenant-id.js';
export * from './ledger-layout.js';
export declare const CONTRACT_VERSION = "V2.0.2";
/** New source hashes imply a new Profile even if tails are byte-identical.
 * Empty until the V2.0.2 candidate is compiled and explicitly reviewed; never reuse the V2.0.1 pin. */
export declare const APPROVED_PROFILE_ID: string;
export declare const COMPILER_COMMIT = "3ed973335b59269293564805cc2c58a14595ec03";
export declare const DEPOSIT = 400000000n, MIN_PRICE = 100000000n;
/** Protocol batch limit; distinct from the coincidentally equal key width. */
export declare const MAX_REFUND_RECORDS = 32;
export declare const FINALIZER = 100000000n, MAX_PAY_FEE = 50000000n, REFUND_FEE = 2000000n;
export declare const DRAW_DELAY = 100n, TIMEOUT_DELAY = 432000n, DAA_LIMIT = 500000000000n;
export declare const MAX_PURCHASES = 256, MAX_TICKETS = 100000, VALUE_LIMIT = 9000000000000000n;
/** Per-template output caps retained in V2.0.2 (inputs remain <= 9). */
export declare const OUTPUT_LIMIT: {
    readonly open: 2;
    readonly sealed: 3;
    readonly refunding: 34;
}, INPUT_LIMIT = 9;
export declare const ZERO: string, MODULES: readonly ["open", "sealed", "refunding"];
export type Module = typeof MODULES[number];
/** Live phases only; KW21 has no DRAW_READY/WINNER_READY ledgers (values 3/4 are invalid here). */
export declare enum Phase {
    OPEN = 1,
    SEALED = 2,
    REFUNDING = 5
}
export interface Config {
    ticketPrice: bigint;
    ticketCap: number;
    purchaseCap: number;
    minTickets: number;
    closeEligibleDaa: bigint;
}
export interface Ledger {
    phase: Phase;
    ownerKey: string;
    config: Config;
    sold: number;
    purchaseCount: number;
    cursor: number;
    directory: Uint8Array;
}
export interface Frame {
    tail: Uint8Array;
    templateHash: string;
    dispatchTag: string;
    sourceSha256: string;
}
export interface Profile {
    id: string;
    frames: Record<Module, Frame>;
    compilerCommit: string;
}
export interface Snapshot {
    ledger: Uint8Array;
    tip: Outpoint;
    scriptPublicKey: Spk;
    covenantId: string;
    value: bigint;
    utxoDaa: bigint;
    currentDaa: bigint;
    origin: Outpoint;
}
export declare function checkPhase(phase: number): Phase;
export declare function phaseModule(phase: Phase): Module;
export declare function templateHash(tail: Uint8Array): string;
/** Recompute identity from actual bytecode, not caller-provided template hashes. */
export declare function profileId(frames: Record<Module, Frame>): string;
/** Fail closed before producing/consuming version-specific scripts. No object-identity
 * cache: Profile fields and Uint8Array tails can be mutated after a successful check. */
export declare function assertCompatibleProfile(p: Profile): void;
/** Data opcodes selected by payload length, matching the pinned compiler.
 * Ledger 80..224B (0..4 records): PUSHDATA1; 260..9296B (5..256 records): PUSHDATA2.
 * Also used for arbitrary witness items, so keep the full general push encoding. */
export declare function pushBytes(b: Uint8Array): Uint8Array;
export declare function scriptFor(ledger: Uint8Array, tail: Uint8Array): Uint8Array;
export declare function newOpen(ownerKey: string, config: Config): Ledger;
export declare function encodeLedger(s: Ledger): Uint8Array;
export declare function decodeLedger(b: Uint8Array): Ledger;
export declare function validateConfig(c: Config): void;
export declare function records(s: Ledger): {
    end: number;
    key: string;
    count: number;
}[];
export declare function validateLedger(s: Ledger): void;
/** Tickets already refunded (records before the cursor). */
export declare function paidTickets(s: Ledger): number;
export declare function valueOf(s: Ledger): bigint;
/** Canonical zero-ticket OPEN script the OPEN contract rebuilds (genesisLedger()) to authenticate the round CID. */
export declare function rootScript(s: Ledger, p: Profile): Uint8Array;
export declare function rootId(origin: Outpoint, genesisScript: Uint8Array): string;
/** Client bootstrap trust: ALL live phases must match the canonical OPEN Genesis CID.
 * Only the on-chain OPEN contract carries origin in its ABI; clients retain it as verified context. */
export declare function verifySnapshot(x: Snapshot, p: Profile): Ledger;
export declare function scriptOf(s: Ledger, p: Profile): Uint8Array;
export declare function appendPurchase(s: Ledger, quantity: number, buyer: string): Ledger;
export declare function cloneLedger(s: Ledger): Ledger;
