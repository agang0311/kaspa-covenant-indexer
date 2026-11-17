//! Typed accepted-transaction view used by the interpreter (`scripts/core/src/transaction.ts`, validation part).
use super::bytes::{check, integer, uint, unhex, unhex_len};
use super::covenant::{Outpoint, Spk};
use anyhow::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub covenant_id: String,
    pub authorizing_input: i128,
}

#[derive(Clone, Debug)]
pub struct Input {
    pub previous_outpoint: Outpoint,
    pub signature_script: String,
    pub sequence: i128,
    pub compute_budget: i128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub value: i128,
    pub script_public_key: Spk,
    pub covenant: Option<Binding>,
}

#[derive(Clone, Debug)]
pub struct Transaction {
    pub version: i128,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
    pub lock_time: i128,
    pub subnetwork_id: String,
    pub gas: i128,
    pub payload: String,
    pub storage_mass: i128,
}

pub fn validate_transaction(t: &Transaction) -> Result<()> {
    integer(t.version, 0, 1)?;
    integer(t.inputs.len() as i128, 0, 65535)?;
    integer(t.outputs.len() as i128, 0, 65535)?;
    for i in &t.inputs {
        unhex_len(&i.previous_outpoint.transaction_id, 32)?;
        integer(i.previous_outpoint.index, 0, 0xffff_ffff)?;
        unhex(&i.signature_script)?;
        uint(i.sequence, 64)?;
        integer(i.compute_budget, 0, 65535)?;
    }
    for o in &t.outputs {
        uint(o.value, 64)?;
        integer(o.script_public_key.version, 0, 65535)?;
        unhex(&o.script_public_key.script)?;
        if let Some(c) = &o.covenant {
            check(t.version == 1, "V0_COVENANT")?;
            unhex_len(&c.covenant_id, 32)?;
            integer(c.authorizing_input, 0, t.inputs.len() as i128 - 1)?;
        }
    }
    uint(t.lock_time, 64)?;
    unhex_len(&t.subnetwork_id, 20)?;
    uint(t.gas, 64)?;
    uint(t.storage_mass, 64)?;
    check(
        unhex(&t.payload)?.len() <= 16 * 1024 * 1024,
        "PAYLOAD_LIMIT",
    )?;
    Ok(())
}

pub fn outpoint_key(o: &Outpoint) -> Result<String> {
    unhex_len(&o.transaction_id, 32)?;
    integer(o.index, 0, 0xffff_ffff)?;
    Ok(format!("{}:{}", o.transaction_id, o.index))
}
