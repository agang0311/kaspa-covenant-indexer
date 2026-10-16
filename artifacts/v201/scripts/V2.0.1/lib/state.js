/** Kaswin physical ledger codec, contract V2.0.1 (magic KW21). Fixed header (80B) + 36*N directory.
 * [0,4) magic | [4,8) phase | [8,16) ticketPrice | [16,20) ticketCap | [20,24) purchaseCap | [24,28) minTickets
 * [28,36) closeEligibleDaa | [36,40) sold | [40,44) purchaseCount | [44,48) refund cursor | [48,80) ownerKey | [80,..) directory
 * No draw/anchor field is persisted: the draw is computed inside DRAW_AND_PAY and TIMEOUT_REFUND only changes phase.
 * Not interchangeable with the V2 (228B, KW20) codec in scripts/f3.2; a Profile selects exactly one codec. */
import { check, integer, cat, ascii, le, fromLe, hex, unhex, same } from './bytes.js';
import { blake2b256 } from './hashes.js';
import { blake3 } from './blake3.js';
import { covenantId, p2sh } from './covenant-id.js';
export const CONTRACT_VERSION = 'V2.0.1', MAGIC = 'KW21';
/** Reviewed codec/template binding; not a claim of production or VM-budget approval.
 * Source: contracts/V2.0.1/pins.json at 9474e5c. New profiles require explicit review. */
export const APPROVED_PROFILE_ID = '78c11314f95531345fb6f8614d4d628e2dbfac3429b7c9e37f4161a3a40907f7';
export const COMPILER_COMMIT = '3ed973335b59269293564805cc2c58a14595ec03';
export const HEADER = 80, DEPOSIT = 400000000n, MIN_PRICE = 100000000n;
export const FINALIZER = 100000000n, MAX_PAY_FEE = 50000000n, REFUND_FEE = 2000000n;
export const DRAW_DELAY = 100n, TIMEOUT_DELAY = 432000n, DAA_LIMIT = 500000000000n;
export const MAX_PURCHASES = 256, MAX_TICKETS = 100_000, VALUE_LIMIT = 9000000000000000n;
/** Per-template output caps enforced by the V2.0.1 contracts (inputs remain <= 9). */
export const OUTPUT_LIMIT = { open: 2, sealed: 3, refunding: 34 }, INPUT_LIMIT = 9;
export const ZERO = '00'.repeat(32), MODULES = ['open', 'sealed', 'refunding'];
/** Live phases only; V2.0.1 has no DRAW_READY/WINNER_READY ledgers (values 3/4 are invalid here). */
export var Phase;
(function (Phase) {
    Phase[Phase["OPEN"] = 1] = "OPEN";
    Phase[Phase["SEALED"] = 2] = "SEALED";
    Phase[Phase["REFUNDING"] = 5] = "REFUNDING";
})(Phase || (Phase = {}));
const PHASES = [Phase.OPEN, Phase.SEALED, Phase.REFUNDING];
export function checkPhase(phase) { check(PHASES.includes(phase), 'PHASE'); return phase; }
export function phaseModule(phase) { checkPhase(phase); return phase === Phase.OPEN ? 'open' : phase === Phase.REFUNDING ? 'refunding' : 'sealed'; }
export function templateHash(tail) { check(tail[0] === 0x6c, 'TEMPLATE_BOUNDARY'); return hex(blake3(cat(le(1n, 8), new Uint8Array([0x6b]), le(BigInt(tail.length), 8), tail))); }
/** Recompute identity from actual bytecode, not caller-provided template hashes. */
export function profileId(frames) {
    for (const m of MODULES) {
        const f = frames?.[m];
        check(f && f.tail instanceof Uint8Array, 'PROFILE_FRAME');
        check(templateHash(f.tail) === f.templateHash, 'ARTIFACT_HASH');
    }
    check(new Set(MODULES.map(m => frames[m].templateHash)).size === 3, 'FRAME_COLLISION');
    return hex(blake2b256(cat(ascii('KASWIN_PROFILE_V2'), ...MODULES.flatMap(m => [unhex(frames[m].sourceSha256, 32), unhex(frames[m].templateHash, 32), unhex(frames[m].dispatchTag, 4)]))));
}
/** Fail closed before producing/consuming version-specific scripts. No object-identity
 * cache: Profile fields and Uint8Array tails can be mutated after a successful check. */
export function assertCompatibleProfile(p) {
    check(p?.id === APPROVED_PROFILE_ID, 'INCOMPATIBLE_PROFILE');
    check(p.compilerCommit === COMPILER_COMMIT, 'PROFILE_COMPILER');
    check(profileId(p.frames) === APPROVED_PROFILE_ID, 'PROFILE_CONTENT_MISMATCH');
}
/** Data opcodes selected by payload length, matching the pinned compiler.
 * Ledger 80..224B (0..4 records): PUSHDATA1; 260..9296B (5..256 records): PUSHDATA2.
 * Also used for arbitrary witness items, so keep the full general push encoding. */
export function pushBytes(b) {
    const n = b.length;
    check(n <= 0x7fffffff, 'PUSH_LIMIT');
    return cat(n === 0 ? new Uint8Array([0]) : n <= 75 ? new Uint8Array([n]) : n <= 255 ? new Uint8Array([0x4c, n]) : n <= 65535 ? cat(new Uint8Array([0x4d]), le(BigInt(n), 2)) : cat(new Uint8Array([0x4e]), le(BigInt(n), 4)), b);
}
export function scriptFor(ledger, tail) { check(tail[0] === 0x6c, 'TEMPLATE_BOUNDARY'); return cat(new Uint8Array([0x6b]), pushBytes(ledger), tail); }
export function newOpen(ownerKey, config) {
    return { phase: Phase.OPEN, ownerKey, config: { ...config }, sold: 0, purchaseCount: 0, cursor: 0, directory: new Uint8Array() };
}
export function encodeLedger(s) {
    const p = cat(ascii(MAGIC), le(BigInt(s.phase), 4), le(s.config.ticketPrice, 8), le(BigInt(s.config.ticketCap), 4), le(BigInt(s.config.purchaseCap), 4), le(BigInt(s.config.minTickets), 4), le(s.config.closeEligibleDaa, 8), le(BigInt(s.sold), 4), le(BigInt(s.purchaseCount), 4), le(BigInt(s.cursor), 4), unhex(s.ownerKey, 32), s.directory);
    check(p.length === HEADER + s.directory.length, 'STATE_LAYOUT');
    return p;
}
export function decodeLedger(b) {
    check(b.length >= HEADER && b.length <= HEADER + MAX_PURCHASES * 36, 'LEDGER_LENGTH');
    check(same(b.slice(0, 4), ascii(MAGIC)), 'STATE_MAGIC');
    const u = (a, w) => { const n = fromLe(b.slice(a, a + w)); check(n < 1n << BigInt(w * 8 - 1), 'SIGNED_DOMAIN'); return n; };
    const n = (a) => Number(u(a, 4));
    const pc = n(40);
    check(pc <= MAX_PURCHASES && b.length === HEADER + 36 * pc, 'DIRECTORY_LENGTH');
    return { phase: n(4), config: { ticketPrice: u(8, 8), ticketCap: n(16), purchaseCap: n(20), minTickets: n(24), closeEligibleDaa: u(28, 8) }, sold: n(36), purchaseCount: pc, cursor: n(44), ownerKey: hex(b.slice(48, 80)), directory: b.slice(HEADER) };
}
export function validateConfig(c) {
    integer(c.ticketCap, 3, MAX_TICKETS);
    integer(c.purchaseCap, 1, MAX_PURCHASES);
    integer(c.minTickets, 3, c.ticketCap);
    check(c.ticketPrice >= MIN_PRICE && c.ticketPrice <= (VALUE_LIMIT - DEPOSIT) / BigInt(c.ticketCap), 'TICKET_PRICE');
    check(c.closeEligibleDaa > 0n && c.closeEligibleDaa < DAA_LIMIT - TIMEOUT_DELAY, 'CLOSE_DAA');
}
export function records(s) {
    check(s.directory.length === s.purchaseCount * 36, 'DIRECTORY_LENGTH');
    let previous = 0;
    return Array.from({ length: s.purchaseCount }, (_, i) => { const at = i * 36, end = Number(fromLe(s.directory.slice(at, at + 4))); integer(end, previous + 1, s.sold); const r = { end, key: hex(s.directory.slice(at + 4, at + 36)), count: end - previous }; previous = end; return r; });
}
export function validateLedger(s) {
    checkPhase(s.phase);
    unhex(s.ownerKey, 32);
    validateConfig(s.config);
    integer(s.sold, 0, s.config.ticketCap);
    integer(s.purchaseCount, 0, Math.min(s.config.purchaseCap, s.sold));
    check((s.sold === 0) === (s.purchaseCount === 0), 'EMPTY_MISMATCH');
    const rs = records(s);
    check((rs.at(-1)?.end ?? 0) === s.sold, 'DIRECTORY_TOTAL');
    if (s.phase === Phase.REFUNDING) {
        integer(s.cursor, 0, s.purchaseCount - 1);
    }
    else
        check(s.cursor === 0, 'CURSOR_NONREFUND');
    if (s.phase === Phase.SEALED)
        check(s.sold >= s.config.minTickets, 'DRAW_BELOW_MINIMUM');
}
/** Tickets already refunded (records before the cursor). */
export function paidTickets(s) { return s.cursor ? records(s)[s.cursor - 1].end : 0; }
export function valueOf(s) { return DEPOSIT + BigInt(s.sold - paidTickets(s)) * s.config.ticketPrice; }
/** Canonical zero-ticket OPEN script the OPEN contract rebuilds (genesisLedger()) to authenticate the round CID. */
export function rootScript(s, p) { assertCompatibleProfile(p); return scriptFor(encodeLedger(newOpen(s.ownerKey, s.config)), p.frames.open.tail); }
export function rootId(origin, genesisScript) { return covenantId(origin, [{ index: 0, value: DEPOSIT, spk: p2sh(hex(blake2b256(genesisScript))) }]); }
/** Client bootstrap trust: ALL live phases must match the canonical OPEN Genesis CID.
 * Only the on-chain OPEN contract carries origin in its ABI; clients retain it as verified context. */
export function verifySnapshot(x, p) {
    assertCompatibleProfile(p);
    unhex(x.tip.transactionId, 32);
    integer(x.tip.index, 0, 0x7fffffff);
    check(x.origin, 'ORIGIN_REQUIRED');
    unhex(x.origin.transactionId, 32);
    check(x.origin.transactionId !== ZERO, 'ORIGIN_ZERO');
    integer(x.origin.index, 0, 0x7fffffff);
    unhex(x.covenantId, 32);
    check(typeof x.utxoDaa === 'bigint' && x.utxoDaa >= 0n && x.utxoDaa < DAA_LIMIT, 'UTXO_DAA');
    check(typeof x.currentDaa === 'bigint' && x.currentDaa >= 0n && x.currentDaa < DAA_LIMIT, 'CURRENT_DAA');
    const s = decodeLedger(x.ledger);
    validateLedger(s);
    const root = rootScript(s, p);
    check(rootId(x.origin, root) === x.covenantId, 'OPEN_GENESIS_CID_MISMATCH');
    check(x.value === valueOf(s), 'STATE_INPUT_AMOUNT');
    const expected = p2sh(hex(blake2b256(scriptOf(s, p))));
    check(x.scriptPublicKey?.version === expected.version && x.scriptPublicKey?.script === expected.script, 'STATE_INPUT_SPK');
    return s;
}
export function scriptOf(s, p) { assertCompatibleProfile(p); return scriptFor(encodeLedger(s), p.frames[phaseModule(s.phase)].tail); }
export function appendPurchase(s, quantity, buyer) { validateLedger(s); check(s.phase === Phase.OPEN, 'NOT_OPEN'); integer(quantity, 1, s.config.ticketCap - s.sold); check(s.purchaseCount < s.config.purchaseCap, 'DIRECTORY_FULL'); unhex(buyer, 32); const sold = s.sold + quantity; return { ...s, sold, purchaseCount: s.purchaseCount + 1, directory: cat(s.directory, le(BigInt(sold), 4), unhex(buyer, 32)) }; }
export function cloneLedger(s) { return decodeLedger(encodeLedger(s)); }
//# sourceMappingURL=state.js.map