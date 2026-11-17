//! On-chain draw mirror and refund batching, contract V2.0.2 (`scripts/core/src/protocol.ts`, indexer-relevant part).
use super::bytes::{ascii, cat, check, from_le, hex, integer, le, unhex_len};
use super::hashes::{blake2b256, domain_hash};
use super::state::{self as S, Ledger, Snapshot};
use anyhow::Result;

pub const ACTION_BUY: i128 = 1;
pub const ACTION_CLOSE: i128 = 2;
pub const ACTION_DRAW_AND_PAY: i128 = 4;
pub const ACTION_TIMEOUT_REFUND: i128 = 9;
pub const ACTION_REFUND: i128 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Buy,
    Close,
    DrawAndPay,
    TimeoutRefund,
    Refund,
}

impl Action {
    pub fn from_number(n: i128) -> Option<Action> {
        match n {
            ACTION_BUY => Some(Action::Buy),
            ACTION_CLOSE => Some(Action::Close),
            ACTION_DRAW_AND_PAY => Some(Action::DrawAndPay),
            ACTION_TIMEOUT_REFUND => Some(Action::TimeoutRefund),
            ACTION_REFUND => Some(Action::Refund),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Action::Buy => "BUY",
            Action::Close => "CLOSE",
            Action::DrawAndPay => "DRAW_AND_PAY",
            Action::TimeoutRefund => "TIMEOUT_REFUND",
            Action::Refund => "REFUND",
        }
    }
}

pub struct Draw {
    pub seed: String,
    pub boundary_daa: i128,
    pub anchor_daa: i128,
    pub anchor_tx_id: String,
    pub anchor_index: i128,
    pub target_hash: String,
    pub target_seq: String,
}

pub struct Sample {
    pub value: i128,
    pub limit: i128,
    pub ticket: Option<i128>,
}

fn opening_number(proof: &[u8], offset: usize) -> Result<i128> {
    let n = from_le(&proof[offset..offset + 8]);
    check(n < (1u128 << 63), "PASS_A_INTEGER")?;
    Ok(n as i128)
}

fn open_seq(proof: &[u8], base: usize, parent: &[u8]) -> Result<[u8; 32]> {
    let ctx = domain_hash("SeqCommitMergesetContext", &proof[base + 64..base + 88])?;
    let pc = domain_hash(
        "SeqCommitmentMerkleBranchHash",
        &cat(&[&ctx, &proof[base + 32..base + 64]]),
    )?;
    let root = domain_hash(
        "SeqCommitmentMerkleBranchHash",
        &cat(&[&proof[base..base + 32], &pc]),
    )?;
    domain_hash("SeqCommitmentMerkleBranchHash", &cat(&[parent, &root]))
}

pub fn authenticate_draw(
    x: &Snapshot,
    s: &Ledger,
    proof: &[u8],
    block_hash: &str,
    sequence_commitment: &str,
) -> Result<Draw> {
    check(s.phase == S::PHASE_SEALED, "NOT_SEALED")?;
    check(proof.len() == 240, "PASS_A_LENGTH")?;
    check(
        x.utxo_daa >= 0 && x.utxo_daa < S::DAA_LIMIT - S::TIMEOUT_DELAY,
        "ANCHOR_DAA",
    )?;
    let boundary = x.utxo_daa + S::DRAW_DELAY;
    check(x.current_daa >= boundary, "DRAW_TOO_EARLY")?;
    let pd = opening_number(proof, 224)?;
    let td = opening_number(proof, 104)?;
    check(
        pd < boundary && td >= boundary && td < S::DAA_LIMIT,
        "PASS_A_FIRST_CROSSING",
    )?;
    check(
        opening_number(proof, 112)? > opening_number(proof, 232)?,
        "PASS_A_BLUE_ORDER",
    )?;
    opening_number(proof, 96)?;
    opening_number(proof, 216)?;
    check(hex(&proof[0..32]) == block_hash, "PASS_A_TARGET_HASH")?;
    let p = open_seq(proof, 152, &proof[120..152])?;
    let t = open_seq(proof, 32, &p)?;
    check(hex(&t) == sequence_commitment, "PASS_A_COMMITMENT")?;
    let raw = S::encode_ledger(s)?;
    let frozen = blake2b256(&cat(&[
        &raw[S::OFFSET_TICKET_PRICE..S::OFFSET_REFUND_CURSOR],
        &s.directory,
    ]));
    let seed = hex(&blake2b256(&cat(&[
        &ascii("KASWIN_V2_DRAW"),
        &unhex_len(&x.covenant_id, 32)?,
        &unhex_len(&x.tip.transaction_id, 32)?,
        &le(x.tip.index, 4)?,
        &frozen,
        &le(boundary, 8)?,
        &proof[0..32],
        &t,
    ])));
    Ok(Draw {
        seed,
        boundary_daa: boundary,
        anchor_daa: x.utxo_daa,
        anchor_tx_id: x.tip.transaction_id.clone(),
        anchor_index: x.tip.index,
        target_hash: hex(&proof[0..32]),
        target_seq: hex(&t),
    })
}

/// Single sample; the preimage keeps the former 8-byte zero counter (no redraw in V2.0.2).
pub fn sample(seed: &str, sold: i128) -> Result<Sample> {
    integer(sold, 3, S::MAX_TICKETS)?;
    let digest = blake2b256(&cat(&[
        &ascii("KASWIN_V2_SAMPLE"),
        &unhex_len(seed, 32)?,
        &[0u8; 8],
    ]));
    let value = from_le(&digest[0..7]) as i128;
    let space: i128 = 1 << 56;
    let limit = space - space % sold;
    Ok(Sample {
        value,
        limit,
        ticket: if value < limit {
            Some(value % sold)
        } else {
            None
        },
    })
}

pub fn record_of_ticket(s: &Ledger, ticket: i128) -> Result<usize> {
    integer(ticket, 0, s.sold - 1)?;
    let i = S::records(s)?.iter().position(|p| ticket < p.end);
    check(i.is_some(), "WINNER_NOT_FOUND")?;
    Ok(i.unwrap())
}

pub struct DrawResult {
    pub draw: Draw,
    #[allow(dead_code)]
    pub sample: Sample,
    pub ticket: i128,
    pub record: usize,
}

pub fn draw_winner(
    x: &Snapshot,
    s: &Ledger,
    proof: &[u8],
    block_hash: &str,
    sequence_commitment: &str,
) -> Result<DrawResult> {
    let draw = authenticate_draw(x, s, proof, block_hash, sequence_commitment)?;
    let smp = sample(&draw.seed, s.sold)?;
    check(smp.ticket.is_some(), "SAMPLE_REJECTED")?;
    let ticket = smp.ticket.unwrap();
    let record = record_of_ticket(s, ticket)?;
    Ok(DrawResult {
        draw,
        sample: smp,
        ticket,
        record,
    })
}

pub struct RefundBatch {
    pub k: i128,
    pub last: bool,
    pub pool: i128,
    pub deposit: i128,
}

pub fn refund_batch(s: &Ledger) -> Result<RefundBatch> {
    check(s.phase == S::PHASE_REFUNDING, "NOT_REFUNDING")?;
    let k = S::MAX_REFUND_RECORDS.min(s.purchase_count - s.cursor);
    let last = s.cursor + k == s.purchase_count;
    check(k > 0, "NO_REFUND_RECORDS")?;
    Ok(RefundBatch {
        k,
        last,
        pool: k * S::REFUND_FEE + if last { S::REFUND_FEE } else { 0 },
        deposit: if last { S::DEPOSIT - S::REFUND_FEE } else { 0 },
    })
}
