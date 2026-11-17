//! Interpretation of ALREADY ACCEPTED contract V2.0.2 transactions (`scripts/core/src/accepted.ts`).
//! Not a VM or acceptance proof: the caller establishes selected-chain acceptance and authenticates the spent UTXO.
use super::bytes::{cat, check, from_le, hex, integer, same, uint, unhex};
use super::covenant::{Outpoint, p2pk, p2sh};
use super::hashes::{blake2b256, domain_hash};
use super::protocol::{Action, draw_winner, refund_batch};
use super::state::{self as S, Ledger, MODULES, Module, Profile, Snapshot};
use super::transaction::{Transaction, outpoint_key, validate_transaction};
use anyhow::{Result, anyhow};

pub fn script_pushes(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    check(bytes.len() <= 250_000, "WITNESS_SIZE")?;
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let op = bytes[i];
        i += 1;
        if op == 0 {
            out.push(vec![]);
            continue;
        }
        if op == 0x4f {
            out.push(vec![0x81]);
            continue;
        }
        if (0x51..=0x60).contains(&op) {
            out.push(vec![op - 0x50]);
            continue;
        }
        let n: usize;
        if op <= 75 {
            n = op as usize;
        } else {
            check((0x4c..=0x4e).contains(&op), "NON_PUSH_WITNESS")?;
            let width = if op == 0x4c {
                1
            } else if op == 0x4d {
                2
            } else {
                4
            };
            check(i + width <= bytes.len(), "PUSH_HEADER")?;
            n = from_le(&bytes[i..i + width]) as usize;
            i += width;
        }
        check(i + n <= bytes.len(), "PUSH_TRUNCATED")?;
        out.push(bytes[i..i + n].to_vec());
        i += n;
    }
    Ok(out)
}

/// SIL int ABI: signed ScriptNum, at most 8 bytes.
pub fn script_number(bytes: &[u8]) -> Result<i128> {
    check(bytes.len() <= 8, "SCRIPT_NUMBER_LENGTH")?;
    if bytes.is_empty() {
        return Ok(0);
    }
    let last = *bytes.last().unwrap();
    let negative = last & 128 != 0;
    let mut copy = bytes.to_vec();
    *copy.last_mut().unwrap() = last & 127;
    let n = from_le(&copy) as i128;
    Ok(if negative { -n } else { n })
}

fn action_name(b: &[u8]) -> Result<Action> {
    Action::from_number(script_number(b)?).ok_or_else(|| anyhow!("UNSUPPORTED_ACTION"))
}

fn u32_of(b: &[u8]) -> Result<i128> {
    check(b.len() == 4, "DATA_U32")?;
    integer(from_le(b) as i128, 0, 0x7fff_ffff)
}

pub fn target_seq_of(opening: &[u8]) -> Result<String> {
    check(opening.len() == 240, "PASS_A_LENGTH")?;
    let branch = |a: &[u8], b: &[u8]| domain_hash("SeqCommitmentMerkleBranchHash", &cat(&[a, b]));
    let seq = |base: usize, parent: &[u8]| -> Result<[u8; 32]> {
        let ctx = domain_hash("SeqCommitMergesetContext", &opening[base + 64..base + 88])?;
        let inner = branch(&ctx, &opening[base + 32..base + 64])?;
        let mid = branch(&opening[base..base + 32], &inner)?;
        branch(parent, &mid)
    };
    let p = seq(152, &opening[120..152])?;
    Ok(hex(&seq(32, &p)?))
}

pub struct Spend {
    pub action: Action,
    pub module: Module,
    #[allow(dead_code)]
    pub spent: Ledger,
    pub ledger: Vec<u8>,
    #[allow(dead_code)]
    pub origin: Outpoint,
    pub actor_key: String,
    pub data: Vec<u8>,
    pub next_tail: Vec<u8>,
    pub witness_fee: i128,
}

pub fn decode_spend(
    script: &str,
    profile: &Profile,
    context_origin: Option<&Outpoint>,
) -> Result<Spend> {
    let pushes = script_pushes(&unhex(script)?)?;
    let redeem = pushes.last();
    check(
        redeem.map(|r| r.first() == Some(&0x6b)).unwrap_or(false),
        "REDEEM_FORMAT",
    )?;
    let redeem = redeem.unwrap();
    let module = MODULES.iter().copied().find(|m| {
        let t = &profile.frame(*m).tail;
        redeem.len() > t.len() && same(&redeem[redeem.len() - t.len()..], t)
    });
    check(module.is_some(), "UNPINNED_TEMPLATE")?;
    let module = module.unwrap();
    let shift = if module == Module::Open { 2 } else { 0 };
    let frame = profile.frame(module);
    check(pushes.len() == 8 + shift, "SPEND_ABI")?;
    let fields = script_pushes(&redeem[1..redeem.len() - frame.tail.len()])?;
    check(fields.len() == 1, "LEDGER_PUSH_COUNT")?;
    let ledger = fields[0].clone();
    let spent = S::decode_ledger(&ledger)?;
    let pm = S::phase_module(spent.phase)?;
    check(
        module == pm && same(redeem, &S::script_of(&spent, profile)?),
        "REDEEM_STATE_TEMPLATE",
    )?;
    let origin = if module == Module::Open {
        Some(Outpoint {
            transaction_id: hex(&pushes[1]),
            index: script_number(&pushes[2])?,
        })
    } else {
        context_origin.cloned()
    };
    check(origin.is_some(), "ORIGIN_REQUIRED")?;
    let origin = origin.unwrap();
    super::bytes::unhex_len(&origin.transaction_id, 32)?;
    integer(origin.index, 0, 0x7fff_ffff)?;
    if let Some(ctx) = context_origin {
        check(&origin == ctx, "ORIGIN_MISMATCH")?;
    }
    check(
        script_number(&pushes[1 + shift])? == frame.tail.len() as i128,
        "TAIL_LENGTH",
    )?;
    check(
        hex(&pushes[6 + shift]) == frame.dispatch_tag,
        "DISPATCH_TAG",
    )?;
    let actor_key = hex(&pushes[4 + shift]);
    super::bytes::unhex_len(&actor_key, 32)?;
    Ok(Spend {
        action: action_name(&pushes[0])?,
        module,
        spent,
        ledger,
        origin,
        actor_key,
        data: pushes[3 + shift].clone(),
        next_tail: pushes[2 + shift].clone(),
        witness_fee: script_number(&pushes[5 + shift])?,
    })
}

pub struct Interpretation {
    pub action: Action,
    pub next: Option<Ledger>,
    pub terminal: Option<&'static str>,
    pub fee: Option<i128>,
    pub timeout_anchor_daa: Option<String>,
}

pub fn interpret_accepted(
    x: &Snapshot,
    p: &Profile,
    tx: &Transaction,
    input_values: Option<&[i128]>,
) -> Result<Interpretation> {
    validate_transaction(tx)?;
    let s = S::verify_snapshot(x, p)?;
    check(
        tx.version == 1
            && !tx.inputs.is_empty()
            && tx.inputs.len() <= S::INPUT_LIMIT
            && !tx.outputs.is_empty(),
        "TX_TOPOLOGY",
    )?;
    check(
        outpoint_key(&tx.inputs[0].previous_outpoint)? == outpoint_key(&x.tip)?,
        "SPENT_OUTPOINT",
    )?;
    let mut keys = std::collections::HashSet::new();
    for i in &tx.inputs {
        keys.insert(outpoint_key(&i.previous_outpoint)?);
    }
    check(keys.len() == tx.inputs.len(), "DUPLICATE_INPUT")?;
    let w = decode_spend(&tx.inputs[0].signature_script, p, x.origin.as_ref())?;
    check(tx.outputs.len() <= w.module.output_limit(), "OUTPUT_LIMIT")?;
    check(same(&w.ledger, &x.ledger), "SPENT_LEDGER")?;
    let allowed: &[Action] = match w.module {
        Module::Open => &[Action::Buy, Action::Close],
        Module::Sealed => &[Action::DrawAndPay, Action::TimeoutRefund],
        Module::Refunding => &[Action::Refund],
    };
    check(allowed.contains(&w.action), "ACTION_PHASE")?;
    let out_sum: i128 = tx.outputs.iter().map(|o| o.value).sum();
    let (mut fee, mut external): (Option<i128>, Option<i128>) = (None, None);
    if let Some(values) = input_values {
        check(
            values.len() == tx.inputs.len() && values[0] == x.value,
            "INPUT_VALUES",
        )?;
        for v in values {
            uint(*v, 64)?;
            check(*v > 0 && *v <= S::VALUE_LIMIT, "INPUT_VALUE")?;
        }
        let ext: i128 = values[1..].iter().sum();
        let f = x.value + ext - out_sum;
        check(f >= 0, "NEGATIVE_FEE")?;
        external = Some(ext);
        fee = Some(f);
    } else if tx.inputs.len() == 1 {
        external = Some(0);
        let f = x.value - out_sum;
        check(f >= 0, "NEGATIVE_FEE")?;
        fee = Some(f);
    }
    check(fee.is_some() && external.is_some(), "INPUT_VALUES_REQUIRED")?;
    let (fee_v, external_v) = (fee.unwrap(), external.unwrap());
    check(x.value > 0 && x.value <= S::VALUE_LIMIT, "INPUT_VALUE")?;
    check(
        tx.outputs
            .iter()
            .all(|o| o.value > 0 && o.value <= S::VALUE_LIMIT),
        "OUTPUT_VALUE",
    )?;
    check(
        fee_v > 0 && fee_v <= S::MAX_PAY_FEE && fee_v == w.witness_fee,
        "FEE_MISMATCH",
    )?;
    let ordinary = |index: usize, value: i128, key: &str| -> Result<()> {
        let ok = match tx.outputs.get(index) {
            Some(o) => {
                value > 0
                    && value <= S::VALUE_LIMIT
                    && o.value == value
                    && o.script_public_key == p2pk(key)?
                    && o.covenant.is_none()
            }
            None => false,
        };
        check(ok, "PAYOUT_MISMATCH")
    };
    let age = |required: i128| -> Result<()> {
        let seq = tx.inputs[0].sequence;
        check(
            seq & (1i128 << 63) == 0 && (seq & 0xffff_ffff) >= required,
            "SEQUENCE_REQUIREMENT",
        )?;
        check(x.current_daa >= x.utxo_daa + required, "AGE_REQUIREMENT")
    };
    let mut next: Option<Ledger> = None;
    let mut terminal: Option<&'static str> = None;
    let mut timeout_anchor_daa: Option<String> = None;
    match w.action {
        Action::Buy => {
            check(
                tx.inputs.len() >= 2 && w.next_tail.is_empty(),
                "BUY_TOPOLOGY",
            )?;
            next = Some(S::append_purchase(&s, u32_of(&w.data)?, &w.actor_key)?);
        }
        Action::Close => {
            check(tx.inputs.len() >= 2 && w.data.is_empty(), "CLOSE_TOPOLOGY")?;
            if s.sold < s.config.ticket_cap && s.purchase_count < s.config.purchase_cap {
                check(
                    tx.lock_time >= s.config.close_eligible_daa - 1
                        && tx.lock_time < S::DAA_LIMIT
                        && tx.inputs[0].sequence != (1i128 << 64) - 1,
                    "CLOSE_LOCKTIME",
                )?;
                check(
                    x.current_daa >= s.config.close_eligible_daa,
                    "CLOSE_TOO_EARLY",
                )?;
            }
            if s.sold == 0 {
                check(w.next_tail.is_empty(), "EMPTY_TAIL")?;
                terminal = Some("EMPTY");
                ordinary(0, S::DEPOSIT, &s.owner_key)?;
            } else {
                next = Some(Ledger {
                    phase: if s.sold >= s.config.min_tickets {
                        S::PHASE_SEALED
                    } else {
                        S::PHASE_REFUNDING
                    },
                    ..s.clone()
                });
            }
        }
        Action::DrawAndPay => {
            check(
                tx.inputs.len() == 1
                    && tx.outputs.len() == 3
                    && w.data.len() == 244
                    && w.next_tail.is_empty(),
                "DRAW_TOPOLOGY",
            )?;
            age(S::DRAW_DELAY)?;
            let opening = &w.data[0..240];
            let seq = target_seq_of(opening)?;
            let target = hex(&opening[0..32]);
            let drawn = draw_winner(x, &s, opening, &target, &seq)?;
            let hint = u32_of(&w.data[240..244])?;
            let records = S::records(&s)?;
            integer(hint, 0, records.len() as i128 - 1)?;
            let r = &records[hint as usize];
            check(hint as usize == drawn.record, "WINNER_HINT")?;
            check(
                fee_v == w.witness_fee && fee_v <= S::MAX_PAY_FEE,
                "DRAW_FEE",
            )?;
            let prize = s.sold * s.config.ticket_price - S::FINALIZER - fee_v;
            check(prize >= S::MIN_PRICE, "WINNER_MINIMUM")?;
            ordinary(0, prize, &r.key)?;
            ordinary(1, S::DEPOSIT, &s.owner_key)?;
            ordinary(2, S::FINALIZER, &w.actor_key)?;
            terminal = Some("PAID");
            let _ = (
                drawn.ticket,
                drawn.draw.boundary_daa,
                drawn.draw.anchor_daa,
                &drawn.draw.anchor_tx_id,
                drawn.draw.anchor_index,
            );
            let _ = (&drawn.draw.target_hash, &drawn.draw.target_seq);
        }
        Action::TimeoutRefund => {
            check(
                w.data.is_empty() && x.utxo_daa < S::DAA_LIMIT - S::TIMEOUT_DELAY,
                "TIMEOUT_DATA",
            )?;
            age(S::TIMEOUT_DELAY)?;
            next = Some(Ledger {
                phase: S::PHASE_REFUNDING,
                ..s.clone()
            });
            timeout_anchor_daa = Some(x.utxo_daa.to_string());
        }
        Action::Refund => {
            check(w.data.is_empty() && w.next_tail.is_empty(), "REFUND_DATA")?;
            let b = refund_batch(&s)?;
            let end = s.cursor + b.k;
            let records = S::records(&s)?;
            check(tx.outputs.len() as i128 == b.k + 2, "REFUND_TOPOLOGY")?;
            for i in 0..b.k {
                let r = &records[(s.cursor + i) as usize];
                ordinary(
                    (if b.last { 0 } else { 1 } + i) as usize,
                    r.count * s.config.ticket_price - S::REFUND_FEE,
                    &r.key,
                )?;
            }
            if b.last {
                terminal = Some("REFUNDED");
                ordinary(b.k as usize, b.deposit, &s.owner_key)?;
            } else {
                next = Some(Ledger {
                    cursor: end,
                    ..s.clone()
                });
            }
            ordinary(
                (b.k + 1) as usize,
                external_v + b.pool - fee_v,
                &w.actor_key,
            )?;
        }
    }
    let family: Vec<usize> = tx
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, o)| {
            o.covenant
                .as_ref()
                .map(|c| c.covenant_id == x.covenant_id)
                .unwrap_or(false)
        })
        .map(|(i, _)| i)
        .collect();
    if let Some(n) = &next {
        S::validate_ledger(n)?;
        let module = S::phase_module(n.phase)?;
        if module != w.module {
            check(
                same(&w.next_tail, &p.frame(module).tail),
                "FOREIGN_TEMPLATE",
            )?;
        }
        let spk = p2sh(&hex(&blake2b256(&S::script_of(n, p)?)))?;
        let o = &tx.outputs[0];
        check(
            family.len() == 1
                && family[0] == 0
                && o.covenant
                    .as_ref()
                    .map(|c| c.authorizing_input == 0)
                    .unwrap_or(false)
                && o.script_public_key == spk
                && o.value == S::value_of(n)?,
            "SUCCESSOR_MISMATCH",
        )?;
    } else {
        check(terminal.is_some() && family.is_empty(), "TERMINAL_FAMILY")?;
    }
    Ok(Interpretation {
        action: w.action,
        next,
        terminal,
        fee,
        timeout_anchor_daa,
    })
}
