//! Native Kaswin kUSDT v2 (256 parallel shards) contract plugin.
//! Tracks manager UTXO + 0..256 parallel shard UTXOs, produces v0.1.3.0-compatible summaries,
//! wallet activity aggregation, and per-covenant live UTXO lookup.

use super::registry::{ScopeEntry, registry_scope};
use super::v202::BlockRef;
use crate::address::spk_to_address;
use crate::core::bytes::{check, from_le, hex, unhex_len};
use crate::core::covenant::{Outpoint, Spk, covenant_id};
use crate::core::hashes::blake2b256;
use crate::core::kusdt_v2::{
    self as K, ContractState, Decision, ManagerEntry, ManagerState, ShardState, Stage,
    TemplateHashes, TicketRecord, WitnessArg, decode_genesis_payload, decode_shard_state,
    decode_spend, draw_sample, encode_manager_state, encode_shard_state, identify_redeem,
    kcc20_vault_spk, pinned_m_open_tail, redeem_p2sh, shard_genesis_cid, shard_tickets_from_state,
};
use crate::core::rolling::Accessor;
use crate::core::state::script_for;
use crate::networks::{Network, network_config};
use crate::normalize::Utxo;
use crate::util::js_bigint;
use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

struct ReceiptMeta {
    action: String,
    shard_cid: Option<String>,
    record_index: Option<usize>,
    rec_idx: Option<u8>,
}

impl ReceiptMeta {
    fn simple(action: &str) -> Self {
        Self {
            action: action.to_string(),
            shard_cid: None,
            record_index: None,
            rec_idx: None,
        }
    }
}

pub const VERSION: &str = "1.0.0";

pub struct KusdtV2 {
    pub network: &'static Network,
    pub hashes: TemplateHashes,
    pub kusdt_cid: String,
    pub scope: Vec<ScopeEntry>,
    pub watch: Vec<String>,
    pub config: Value,
}

#[derive(Clone, Debug)]
struct ParsedOutput {
    value: i128,
    spk: Spk,
    cov_cid: Option<String>,
    cov_auth: Option<i64>,
}

#[derive(Clone, Debug)]
struct ParsedInput {
    previous_outpoint: Outpoint,
    signature_script: String,
    value: Option<i128>,
    cov_cid: Option<String>,
}

#[derive(Clone, Debug)]
struct ParsedTx {
    version: i64,
    lock_time: i128,
    inputs: Vec<ParsedInput>,
    outputs: Vec<ParsedOutput>,
}

fn outpoint_of(v: Option<&Value>) -> Result<Outpoint> {
    let o = v.ok_or_else(|| anyhow!("OUTPOINT_MISSING"))?;
    let txid = o
        .get("transactionId")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow!("OUTPOINT_TXID"))?
        .to_string();
    let idx = js_bigint(o.get("index"))?;
    Ok(Outpoint {
        transaction_id: txid,
        index: idx,
    })
}

fn spk_struct(v: &Value) -> Spk {
    match v {
        Value::String(s) if s.len() >= 4 => {
            let ver = u16::from_le_bytes([
                u8::from_str_radix(&s[0..2], 16).unwrap_or(0),
                u8::from_str_radix(&s[2..4], 16).unwrap_or(0),
            ]) as i128;
            Spk {
                version: ver,
                script: s[4..].to_string(),
            }
        }
        _ => Spk {
            version: js_bigint(v.get("version")).unwrap_or(0),
            script: v
                .get("script")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        },
    }
}

fn spk_value(s: &Spk) -> Value {
    json!({"version": s.version, "script": s.script})
}

fn parse_tx(tx: &Value) -> Result<ParsedTx> {
    let version = if tx.get("version").is_none_or(|v| v.is_null()) {
        1
    } else {
        js_bigint(tx.get("version")).unwrap_or(-1) as i64
    };
    let lock_time = js_bigint(tx.get("lockTime")).unwrap_or(0);
    let in_arr = tx
        .get("inputs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("TX_INPUTS"))?;
    let mut inputs = Vec::with_capacity(in_arr.len());
    for i in in_arr {
        let po = outpoint_of(i.get("previousOutpoint"))?;
        let sig = i
            .get("signatureScript")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let entry = i.get("verboseData").and_then(|v| v.get("utxoEntry"));
        let value = entry
            .and_then(|u| u.get("amount"))
            .and_then(|a| js_bigint(Some(a)).ok());
        let cov_cid = entry
            .and_then(|u| u.get("covenantId"))
            .and_then(|c| c.as_str())
            .map(str::to_owned);
        inputs.push(ParsedInput {
            previous_outpoint: po,
            signature_script: sig,
            value,
            cov_cid,
        });
    }
    let out_arr = tx
        .get("outputs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("TX_OUTPUTS"))?;
    let mut outputs = Vec::with_capacity(out_arr.len());
    for o in out_arr {
        let value = js_bigint(o.get("value"))?;
        let spk = spk_struct(o.get("scriptPublicKey").unwrap_or(&Value::Null));
        let cov = o.get("covenant").filter(|c| !c.is_null());
        let cov_cid = cov
            .and_then(|c| c.get("covenantId"))
            .and_then(|s| s.as_str())
            .map(str::to_owned);
        let cov_auth = cov
            .and_then(|c| c.get("authorizingInput"))
            .and_then(|a| a.as_i64());
        outputs.push(ParsedOutput {
            value,
            spk,
            cov_cid,
            cov_auth,
        });
    }
    Ok(ParsedTx {
        version,
        lock_time,
        inputs,
        outputs,
    })
}

fn arg_int(args: &BTreeMap<&'static str, WitnessArg>, k: &str) -> Result<i64> {
    match args.get(k) {
        Some(WitnessArg::Int(v)) => Ok(*v),
        _ => bail!("MISSING_INT_ARG {k}"),
    }
}

fn arg_bytes<'a>(args: &'a BTreeMap<&'static str, WitnessArg>, k: &str) -> Result<&'a [u8]> {
    match args.get(k) {
        Some(WitnessArg::Bytes(v)) => Ok(v.as_slice()),
        _ => bail!("MISSING_BYTES_ARG {k}"),
    }
}

fn tx_fee(parsed: &ParsedTx) -> i128 {
    let in_sum: i128 = parsed.inputs.iter().filter_map(|i| i.value).sum();
    let out_sum: i128 = parsed.outputs.iter().map(|o| o.value).sum();
    if in_sum >= out_sum {
        in_sum - out_sum
    } else {
        0
    }
}

fn outpoint_json(op: &Outpoint) -> Value {
    json!({"transactionId": op.transaction_id, "index": op.index})
}

pub fn action_of(tx: &Value) -> String {
    let hashes = TemplateHashes::default();
    let Some(inputs) = tx.get("inputs").and_then(|i| i.as_array()) else {
        return "UNKNOWN".into();
    };
    let Some(sig0) = inputs
        .first()
        .and_then(|i| i.get("signatureScript"))
        .and_then(|s| s.as_str())
    else {
        return "GENESIS".into();
    };
    let Ok(w) = decode_spend(sig0, &hashes) else {
        return "GENESIS".into();
    };
    match w.state.stage() {
        Stage::MOpen => match arg_int(&w.args, "action").ok() {
            Some(1) => "OPEN_SHARD".into(),
            Some(2) => "EMPTY_CLOSE".into(),
            Some(3) => "COLLECT_BATCH".into(),
            _ => "M_OPEN".into(),
        },
        Stage::MSealed => match arg_int(&w.args, "action").ok() {
            Some(1) => "DRAW".into(),
            Some(2) => "REROLL".into(),
            _ => "M_SEALED".into(),
        },
        Stage::MDispatch => "DISPATCH_BATCH".into(),
        Stage::SOpen => match arg_int(&w.args, "action").ok() {
            Some(1) => "BUY".into(),
            Some(2) => "COLLECT_SHARD".into(),
            _ => "S_OPEN".into(),
        },
        Stage::SFrozen => "DISPATCH_SHARD".into(),
        Stage::SPayout => match arg_int(&w.args, "action").ok() {
            Some(1) => "PAYOUT_EXECUTOR".into(),
            Some(2) => "PAYOUT_SELF".into(),
            _ => "PAYOUT".into(),
        },
        Stage::SRefund => "CLAIM_REFUND".into(),
    }
}

pub fn draw_target_hash(tx: &Value) -> Option<String> {
    let hashes = TemplateHashes::default();
    let sig0 = tx
        .get("inputs")?
        .as_array()?
        .first()?
        .get("signatureScript")?
        .as_str()?;
    let w = decode_spend(sig0, &hashes).ok()?;
    if w.state.stage() != Stage::MSealed || arg_int(&w.args, "action").ok() != Some(1) {
        return None;
    }
    let data = arg_bytes(&w.args, "data").ok()?;
    if data.len() < 32 {
        return None;
    }
    Some(hex(&data[0..32]))
}

impl KusdtV2 {
    pub fn create(options: &Value, network_name: Option<&str>) -> Result<Self> {
        let net_name = network_name.unwrap_or("mainnet");
        let net = network_config(net_name)?;
        let hashes = TemplateHashes::from_config(options.get("templateHashes"))?;
        if let Some(pid) = options.get("profileId").and_then(|v| v.as_str())
            && pid != hashes.m_open
        {
            bail!("KUSDT_V2_PROFILE_MISMATCH {pid}");
        }
        let kusdt_cid = options
            .get("kusdtCid")
            .and_then(|v| v.as_str())
            .unwrap_or(K::MAINNET_KUSDT_CID)
            .to_string();
        unhex_len(&kusdt_cid, 32)?;

        let addrs: Vec<String> = match options.get("registryAddresses").and_then(|a| a.as_array()) {
            Some(arr) if !arr.is_empty() => arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
            _ => vec![net.registry.to_string()],
        };
        let scope = registry_scope(&addrs, net.key)?;
        let watch: Vec<String> = scope.iter().map(|s| s.address.clone()).collect();
        let config = json!({
            "profileId": hashes.m_open,
            "kusdtCid": kusdt_cid,
            "registryAddresses": watch,
        });
        Ok(Self {
            network: net,
            hashes,
            kusdt_cid,
            scope,
            watch,
            config,
        })
    }

    pub fn profile_id(&self) -> &str {
        &self.hashes.m_open
    }

    pub fn watch_addresses(&self) -> Vec<String> {
        self.watch.clone()
    }

    pub fn discover(&self, u: &Utxo) -> Option<String> {
        if u.address
            .as_deref()
            .is_some_and(|a| self.watch.iter().any(|w| w == a))
        {
            Some(u.outpoint_txid().to_string())
        } else {
            None
        }
    }

    pub fn candidate_genesis(&self, tx: &Value) -> bool {
        self.inspect_genesis(tx).0
    }

    pub fn inspect_genesis(&self, tx: &Value) -> (bool, String) {
        let Some(payload) = tx.get("payload").and_then(|p| p.as_str()) else {
            return (false, "MISSING_GENESIS_PAYLOAD".into());
        };
        let mgr_state = match decode_genesis_payload(payload, &self.hashes.m_open) {
            Ok(s) => s,
            Err(e) => return (false, e.to_string()),
        };
        let Ok(parsed) = parse_tx(tx) else {
            return (false, "TX_PARSE".into());
        };
        if parsed.version != 1 || parsed.inputs.is_empty() || parsed.outputs.is_empty() {
            return (false, "GENESIS_SHAPE".into());
        }
        let out0 = &parsed.outputs[0];
        if out0.value != K::MANAGER_DEPOSIT {
            return (false, "MANAGER_DEPOSIT".into());
        }
        let Some(cid) = out0.cov_cid.as_deref() else {
            return (false, "MISSING_COVENANT".into());
        };
        if out0.cov_auth != Some(0) {
            return (false, "AUTHORIZING_INPUT".into());
        }
        if parsed.outputs[1..].iter().any(|o| o.cov_cid.is_some()) {
            return (false, "EXTRA_COVENANT_OUTPUT".into());
        }
        let Ok(tail) = pinned_m_open_tail() else {
            return (false, "M_OPEN_TAIL".into());
        };
        let Ok(state_bytes) = encode_manager_state(&mgr_state) else {
            return (false, "M_OPEN_ENCODE".into());
        };
        let Ok(redeem) = script_for(&state_bytes, &tail) else {
            return (false, "REDEEM_BUILD".into());
        };
        let Ok(expected_spk) = redeem_p2sh(&redeem) else {
            return (false, "REDEEM_SPK".into());
        };
        if out0.spk != expected_spk {
            return (false, "M_OPEN_SPK_MISMATCH".into());
        }
        let expected_cid = covenant_id(
            &parsed.inputs[0].previous_outpoint,
            &[crate::core::covenant::AuthorizedOutput {
                index: 0,
                value: K::MANAGER_DEPOSIT,
                spk: &out0.spk,
            }],
        );
        match expected_cid {
            Ok(c) if c == cid => (true, "KUSDT_V2_GENESIS_OK".into()),
            _ => (false, "COVENANT_ID_MISMATCH".into()),
        }
    }

    pub fn genesis(&self, tx: &Value, b: &BlockRef) -> Result<Option<Value>> {
        if !self.inspect_genesis(tx).0 {
            return Ok(None);
        }
        let payload = tx["payload"].as_str().unwrap_or("");
        let mgr_state = decode_genesis_payload(payload, &self.hashes.m_open)?;
        let tail = pinned_m_open_tail()?;
        let redeem = script_for(&encode_manager_state(&mgr_state)?, &tail)?;
        let parsed = parse_tx(tx)?;
        let out0 = &parsed.outputs[0];
        let cid = out0.cov_cid.as_deref().unwrap();
        let txid = tx["verboseData"]["transactionId"]
            .as_str()
            .ok_or_else(|| anyhow!("TXID_MISSING"))?;
        let block_hash = tx["verboseData"].get("blockHash").cloned();
        let fee = tx_fee(&parsed);

        let mut r = Map::new();
        r.insert("id".into(), json!(txid));
        r.insert("cid".into(), json!(cid));
        r.insert("profileId".into(), json!(self.hashes.m_open));
        r.insert("protocolId".into(), json!(K::PROTOCOL_ID));
        r.insert("version".into(), json!(VERSION));
        r.insert("network".into(), json!(self.network.key));
        r.insert("networkGenesis".into(), json!(self.network.genesis));
        r.insert("genesisAccepting".into(), json!(b.hash));
        if let Some(h) = &block_hash {
            r.insert("genesisContaining".into(), h.clone());
        }
        r.insert("genesisTransaction".into(), tx.clone());
        r.insert("latestTxid".into(), json!(txid));
        r.insert("latestTransaction".into(), tx.clone());
        r.insert("accepting".into(), json!(b.hash));
        if let Some(h) = block_hash {
            r.insert("containing".into(), h);
        }
        let tip = json!({"transactionId": txid, "index": 0});
        r.insert("tip".into(), tip.clone());
        r.insert(
            "origin".into(),
            outpoint_json(&parsed.inputs[0].previous_outpoint),
        );
        r.insert("spk".into(), spk_value(&out0.spk));
        r.insert("value".into(), json!(K::MANAGER_DEPOSIT.to_string()));
        r.insert("utxoDaa".into(), json!(b.daa));
        r.insert("stage".into(), json!("M_OPEN"));
        r.insert("redeemHex".into(), json!(hex(&redeem)));
        r.insert("terminal".into(), Value::Null);
        r.insert("winnerPk".into(), Value::Null);
        r.insert("step".into(), json!(1));
        r.insert("fees".into(), json!(fee.to_string()));
        r.insert(
            "settings".into(),
            json!({
                "kusdtCid": self.kusdt_cid,
                "creatorPk": mgr_state.header.creator_pk,
                "ticketPrice": mgr_state.header.price.to_string(),
                "closeDaa": mgr_state.header.close_daa.to_string(),
            }),
        );
        r.insert(
            "manager".into(),
            json!({
                "outpoint": tip,
                "value": K::MANAGER_DEPOSIT.to_string(),
                "spk": spk_value(&out0.spk),
                "utxoDaa": b.daa,
                "redeemHex": hex(&redeem),
                "state": mgr_state.to_json(),
            }),
        );
        r.insert("shards".into(), json!({}));
        r.insert(
            "wallets".into(),
            json!({ mgr_state.header.creator_pk.clone(): ["creator"] }),
        );
        r.insert(
            "_deltaReceipt".into(),
            json!({
                "txid": txid,
                "action": "GENESIS",
                "accepted": true,
                "candidate": false,
                "finalized": true,
                "acceptingBlock": b.hash,
                "acceptingDaa": b.daa,
                "feeSompi": fee.to_string(),
            }),
        );
        Ok(Some(Value::Object(r)))
    }

    pub fn advance(&self, r: &Value, t: &Value, b: &BlockRef) -> Result<Value> {
        self.advance_with_accessor(r, t, b, None)
    }

    pub fn advance_with_accessor(
        &self,
        r: &Value,
        t: &Value,
        b: &BlockRef,
        accessor: Option<&Accessor>,
    ) -> Result<Value> {
        let mut next = r.as_object().cloned().ok_or_else(|| anyhow!("ROUND_OBJ"))?;
        let cid = next
            .get("cid")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("KUSDT_V2_CID"))?
            .to_string();
        let tx = parse_tx(t)?;
        check(tx.version == 1 && !tx.inputs.is_empty(), "KUSDT_V2_TX_SHAPE")?;
        let txid = t["verboseData"]["transactionId"]
            .as_str()
            .ok_or_else(|| anyhow!("TXID_MISSING"))?
            .to_string();
        let block_hash = t["verboseData"].get("blockHash").cloned();
        let fee = tx_fee(&tx);
        check(fee <= K::FEE_CAP, "KUSDT_V2_FEE_CAP")?;

        let mgr_outpoint = next
            .get("manager")
            .and_then(|m| m.get("outpoint"))
            .and_then(|o| outpoint_of(Some(o)).ok());

        let spends_manager = mgr_outpoint
            .as_ref()
            .is_some_and(|op| tx.inputs[0].previous_outpoint == *op);

        let receipt_meta = if spends_manager {
            self.apply_manager_spend(&mut next, &cid, &tx, &txid, b, accessor)?
        } else {
            self.apply_shard_spend(&mut next, &cid, &tx, &txid, b)?
        };

        // Reconcile round tip, address, and terminal state.
        refresh_round_head(&mut next, &txid, t, b, block_hash, fee, &receipt_meta)?;
        Ok(Value::Object(next))
    }

    fn apply_manager_spend(
        &self,
        next: &mut Map<String, Value>,
        cid: &str,
        tx: &ParsedTx,
        txid: &str,
        b: &BlockRef,
        accessor: Option<&Accessor>,
    ) -> Result<ReceiptMeta> {
        let w0 = decode_spend(&tx.inputs[0].signature_script, &self.hashes)?;
        let ContractState::Manager(mgr) = w0.state else {
            bail!("EXPECTED_MANAGER_INPUT");
        };
        let mgr_spk = redeem_p2sh(&w0.raw_redeem)?;
        if let Some(expected_spk) = next.get("manager").and_then(|m| m.get("spk")) {
            let cur_spk = spk_struct(expected_spk);
            check(cur_spk == mgr_spk, "MANAGER_SPK_MISMATCH")?;
        }
        // Populate immutable settings once observed from first M_OPEN spend.
        if let Some(settings) = next.get_mut("settings").and_then(|s| s.as_object_mut()) {
            settings.insert("creatorPk".into(), json!(mgr.header.creator_pk));
            settings.insert("ticketPrice".into(), json!(mgr.header.price.to_string()));
            settings.insert("closeDaa".into(), json!(mgr.header.close_daa.to_string()));
            settings.insert("kusdtCid".into(), json!(self.kusdt_cid));
        }
        record_wallet_role(next, &mgr.header.creator_pk, "creator");

        match mgr.header.stage {
            Stage::MOpen => {
                let action = arg_int(&w0.args, "action")?;
                match action {
                    1 => {
                        // OPEN_SHARD: input[0]=M_OPEN, out[0]=M_OPEN(openedCount+1), out[1]=S_OPEN(2 KAS), out[2]=KCC20 vault(2 KAS)
                        let shard_tail = arg_bytes(&w0.args, "shardTail")?;
                        let kcc20_suffix = arg_bytes(&w0.args, "extraBytes")?;
                        let data = arg_bytes(&w0.args, "data")?;
                        check(data.len() == 36, "OPEN_SHARD_DATA_LEN")?;
                        let qty = from_le(&data[0..4]) as u32;
                        let buyer_pk = hex(&data[4..36]);
                        check(qty >= 1, "OPEN_SHARD_QTY")?;
                        check(tx.inputs.len() >= 3, "OPEN_SHARD_INPUTS")?;
                        check(tx.outputs.len() >= 3, "OPEN_SHARD_OUTPUTS")?;

                        let s_open_state = ShardState::OpenOrFrozen {
                            stage: Stage::SOpen,
                            version: K::VERSION,
                            manager_cid: cid.to_string(),
                            first_buyer_pk: buyer_pk.clone(),
                            price: mgr.header.price,
                            records: vec![TicketRecord {
                                cum_tickets: qty,
                                buyer_pk: buyer_pk.clone(),
                            }],
                        };
                        let shard_redeem =
                            script_for(&encode_shard_state(&s_open_state)?, shard_tail)?;
                        let (decoded_shard, _) = identify_redeem(&shard_redeem, &self.hashes)?;
                        check(decoded_shard.stage() == Stage::SOpen, "OPEN_SHARD_TEMPLATE")?;
                        let shard_spk = redeem_p2sh(&shard_redeem)?;
                        let auth_idx_raw = tx.outputs[1]
                            .cov_auth
                            .ok_or_else(|| anyhow!("SHARD_AUTHORIZING_INPUT"))?;
                        let auth_idx = usize::try_from(auth_idx_raw)
                            .map_err(|_| anyhow!("SHARD_AUTHORIZING_INPUT"))?;
                        check(auth_idx < tx.inputs.len(), "SHARD_AUTHORIZING_INPUT")?;
                        let shard_cid = shard_genesis_cid(
                            &tx.inputs[auth_idx].previous_outpoint,
                            1,
                            &s_open_state,
                            shard_tail,
                        )?;
                        check(tx.outputs[1].value == K::SHARD_LEDGER_DEPOSIT, "SHARD_LEDGER_VAL")?;
                        check(tx.outputs[1].spk == shard_spk, "SHARD_LEDGER_SPK")?;
                        check(
                            tx.outputs[1].cov_cid.as_deref() == Some(shard_cid.as_str()),
                            "SHARD_CID",
                        )?;

                        let vault_amount = (qty as i128) * mgr.header.price;
                        let mut owner_cid = [0u8; 32];
                        owner_cid.copy_from_slice(&unhex_len(&shard_cid, 32)?);
                        let expected_vault_spk =
                            kcc20_vault_spk(&owner_cid, 0x04, vault_amount, kcc20_suffix)?;
                        check(tx.outputs[2].value == K::SHARD_VAULT_DEPOSIT, "SHARD_VAULT_VAL")?;
                        check(tx.outputs[2].spk == expected_vault_spk, "SHARD_VAULT_SPK")?;
                        check(
                            tx.outputs[2].cov_cid.as_deref() == Some(self.kusdt_cid.as_str()),
                            "SHARD_VAULT_CID",
                        )?;

                        let mut next_entries = mgr.entries.clone();
                        next_entries.push(ManagerEntry {
                            shard_cid: shard_cid.clone(),
                            ledger_hash: K::ZERO32.to_string(),
                            cum_tickets: 0,
                        });
                        let mut next_hdr = mgr.header.clone();
                        next_hdr.opened_count += 1;
                        let next_mgr = ManagerState {
                            header: next_hdr,
                            entries: next_entries,
                            winner_pk: None,
                            shard_cids: vec![],
                        };
                        update_manager_node(next, &next_mgr, &w0.tail, txid, 0, &tx.outputs[0], b)?;
                        upsert_shard_node(
                            next,
                            &shard_cid,
                            &s_open_state,
                            &shard_redeem,
                            &Outpoint {
                                transaction_id: txid.to_string(),
                                index: 1,
                            },
                            tx.outputs[1].value,
                            &shard_spk,
                            &b.daa,
                            Some((
                                Outpoint {
                                    transaction_id: txid.to_string(),
                                    index: 2,
                                },
                                vault_amount,
                            )),
                            false,
                        );
                        record_wallet_role(next, &buyer_pk, "buyer");
                        record_wallet_role(next, &buyer_pk, "firstBuyerDeposit");
                        Ok(ReceiptMeta {
                            action: "OPEN_SHARD".into(),
                            shard_cid: Some(shard_cid),
                            record_index: Some(0),
                            rec_idx: None,
                        })
                    }
                    2 => {
                        // EMPTY_CLOSE: openedCount == 0, tx.daa >= closeDaa, manager destroyed, output[0]=creatorPk 4 KAS
                        check(mgr.header.opened_count == 0, "EMPTY_CLOSE_NOT_EMPTY")?;
                        next.insert("manager".into(), Value::Null);
                        next.insert("stage".into(), json!("DONE"));
                        next.insert("terminal".into(), json!("EMPTY"));
                        Ok(ReceiptMeta::simple("EMPTY_CLOSE"))
                    }
                    3 => {
                        // COLLECT_BATCH: input[0]=M_OPEN, inputs[1..=k]=S_OPEN; out[0]=M_OPEN|M_SEALED|M_DISPATCH, outs[1..=k]=S_FROZEN
                        let shard_frozen_tail = arg_bytes(&w0.args, "shardTail")?;
                        let extra_mgr_tail = arg_bytes(&w0.args, "extraBytes")?;
                        let data = arg_bytes(&w0.args, "data")?;
                        check(data.len() >= 6, "COLLECT_DATA_LEN")?;
                        let k = u16::from_le_bytes(data[0..2].try_into().unwrap()) as usize;
                        check((1..=K::MAX_BATCH).contains(&k), "COLLECT_BATCH_K")?;
                        check(tx.inputs.len() > k && tx.outputs.len() > k, "COLLECT_IO_COUNT")?;
                        check(
                            (mgr.header.cursor as usize) + k <= mgr.header.opened_count as usize,
                            "COLLECT_CURSOR_RANGE",
                        )?;

                        let mut next_entries = mgr.entries.clone();
                        let mut running_total = mgr.header.total_tickets;
                        for idx in 0..k {
                            let shard_idx = (mgr.header.cursor as usize) + idx;
                            let expected_scid = &mgr.entries[shard_idx].shard_cid;
                            let wi = decode_spend(&tx.inputs[1 + idx].signature_script, &self.hashes)?;
                            let ContractState::Shard(ShardState::OpenOrFrozen {
                                stage: Stage::SOpen,
                                version,
                                manager_cid,
                                first_buyer_pk,
                                price,
                                records,
                            }) = wi.state
                            else {
                                bail!("COLLECT_EXPECTED_S_OPEN");
                            };
                            check(manager_cid == cid, "COLLECT_SHARD_MANAGER_CID")?;
                            let frozen_state = ShardState::OpenOrFrozen {
                                stage: Stage::SFrozen,
                                version,
                                manager_cid,
                                first_buyer_pk: first_buyer_pk.clone(),
                                price,
                                records: records.clone(),
                            };
                            let encoded_frozen = encode_shard_state(&frozen_state)?;
                            let frozen_hash = hex(&blake2b256(&encoded_frozen));
                            let shard_ticks = shard_tickets_from_state(&frozen_state);
                            running_total += shard_ticks;
                            next_entries[shard_idx].ledger_hash = frozen_hash;
                            next_entries[shard_idx].cum_tickets = running_total;

                            let frozen_redeem = script_for(&encoded_frozen, shard_frozen_tail)?;
                            let (dec_f, _) = identify_redeem(&frozen_redeem, &self.hashes)?;
                            check(dec_f.stage() == Stage::SFrozen, "COLLECT_FROZEN_TEMPLATE")?;
                            let frozen_spk = redeem_p2sh(&frozen_redeem)?;
                            check(tx.outputs[1 + idx].spk == frozen_spk, "COLLECT_SHARD_OUT_SPK")?;

                            upsert_shard_node(
                                next,
                                expected_scid,
                                &frozen_state,
                                &frozen_redeem,
                                &Outpoint {
                                    transaction_id: txid.to_string(),
                                    index: (1 + idx) as i128,
                                },
                                tx.outputs[1 + idx].value,
                                &frozen_spk,
                                &b.daa,
                                None,
                                false,
                            );
                            for rec in &records {
                                record_wallet_role(next, &rec.buyer_pk, "buyer");
                            }
                            record_wallet_role(next, &first_buyer_pk, "firstBuyerDeposit");
                        }

                        let next_cursor = mgr.header.cursor + (k as u16);
                        let is_last_batch = next_cursor == mgr.header.opened_count;
                        if !is_last_batch {
                            let mut hdr = mgr.header.clone();
                            hdr.cursor = next_cursor;
                            hdr.total_tickets = running_total;
                            let next_mgr = ManagerState {
                                header: hdr,
                                entries: next_entries,
                                winner_pk: None,
                                shard_cids: vec![],
                            };
                            update_manager_node(
                                next,
                                &next_mgr,
                                &w0.tail,
                                txid,
                                0,
                                &tx.outputs[0],
                                b,
                            )?;
                        } else if running_total >= K::MIN_DRAW_TICKETS {
                            let mut hdr = mgr.header.clone();
                            hdr.stage = Stage::MSealed;
                            hdr.cursor = 0;
                            hdr.total_tickets = running_total;
                            let next_mgr = ManagerState {
                                header: hdr,
                                entries: next_entries,
                                winner_pk: None,
                                shard_cids: vec![],
                            };
                            update_manager_node(
                                next,
                                &next_mgr,
                                extra_mgr_tail,
                                txid,
                                0,
                                &tx.outputs[0],
                                b,
                            )?;
                        } else {
                            // 1..2 tickets: direct transition M_OPEN -> M_DISPATCH (REFUND)
                            let shard_cids: Vec<String> =
                                next_entries.iter().map(|e| e.shard_cid.clone()).collect();
                            let mut hdr = mgr.header.clone();
                            hdr.stage = Stage::MDispatch;
                            hdr.cursor = 0;
                            hdr.total_tickets = running_total;
                            hdr.decision = Decision::Refund;
                            let next_mgr = ManagerState {
                                header: hdr,
                                entries: vec![],
                                winner_pk: Some(K::ZERO32.to_string()),
                                shard_cids,
                            };
                            update_manager_node(
                                next,
                                &next_mgr,
                                extra_mgr_tail,
                                txid,
                                0,
                                &tx.outputs[0],
                                b,
                            )?;
                        }
                        Ok(ReceiptMeta::simple("COLLECT_BATCH"))
                    }
                    _ => bail!("UNKNOWN_M_OPEN_ACTION {action}"),
                }
            }
            Stage::MSealed => {
                let action = arg_int(&w0.args, "action")?;
                match action {
                    1 => {
                        // DRAW: data = passAProof(240) || j(u16 LE) || i(u16 LE) || frozenState(...)
                        let next_tail = arg_bytes(&w0.args, "nextTail")?;
                        let data = arg_bytes(&w0.args, "data")?;
                        check(data.len() > 244, "DRAW_DATA_LEN")?;
                        let proof = &data[0..240];
                        let j = u16::from_le_bytes(data[240..242].try_into().unwrap()) as usize;
                        let i = u16::from_le_bytes(data[242..244].try_into().unwrap()) as usize;
                        let frozen_bytes = &data[244..];
                        check(j < mgr.entries.len(), "DRAW_SHARD_INDEX")?;
                        check(
                            hex(&blake2b256(frozen_bytes)) == mgr.entries[j].ledger_hash,
                            "DRAW_LEDGER_HASH",
                        )?;
                        let ShardState::OpenOrFrozen {
                            stage: Stage::SFrozen,
                            records,
                            ..
                        } = decode_shard_state(Stage::SFrozen, frozen_bytes)?
                        else {
                            bail!("DRAW_FROZEN_DECODE");
                        };
                        check(i < records.len(), "DRAW_RECORD_INDEX")?;

                        let drawn_ticket = if let Some(acc) = accessor {
                            let utxo_daa = next
                                .get("manager")
                                .and_then(|m| m.get("utxoDaa"))
                                .and_then(|v| js_bigint(Some(v)).ok())
                                .unwrap_or(0);
                            let cur_daa = js_bigint(Some(&json!(b.daa)))?;
                            let ticket = draw_sample(
                                cid,
                                &tx.inputs[0].previous_outpoint,
                                utxo_daa,
                                cur_daa,
                                mgr.header.total_tickets,
                                proof,
                                acc,
                            )?;
                            let prev_cum = if j == 0 {
                                0
                            } else {
                                mgr.entries[j - 1].cum_tickets
                            };
                            let local_idx = ticket - prev_cum;
                            let rec_prev = if i == 0 { 0 } else { records[i - 1].cum_tickets };
                            check(
                                local_idx >= rec_prev && local_idx < records[i].cum_tickets,
                                "DRAW_WINNING_RECORD_MISMATCH",
                            )?;
                            Some(ticket)
                        } else {
                            None
                        };

                        let winner_pk = records[i].buyer_pk.clone();
                        let shard_cids: Vec<String> =
                            mgr.entries.iter().map(|e| e.shard_cid.clone()).collect();
                        let mut hdr = mgr.header.clone();
                        hdr.stage = Stage::MDispatch;
                        hdr.cursor = 0;
                        hdr.decision = Decision::Payout;
                        let next_mgr = ManagerState {
                            header: hdr,
                            entries: vec![],
                            winner_pk: Some(winner_pk.clone()),
                            shard_cids,
                        };
                        next.insert("winnerPk".into(), json!(winner_pk));
                        next.insert(
                            "draw".into(),
                            json!({
                                "txid": txid,
                                "shard": j,
                                "record": i,
                                "ticket": drawn_ticket,
                                "winnerPk": winner_pk,
                            }),
                        );
                        update_manager_node(next, &next_mgr, next_tail, txid, 0, &tx.outputs[0], b)?;
                        Ok(ReceiptMeta::simple("DRAW"))
                    }
                    2 => {
                        // REROLL: self-continuation with same state & tail
                        update_manager_node(next, &mgr, &w0.tail, txid, 0, &tx.outputs[0], b)?;
                        Ok(ReceiptMeta::simple("REROLL"))
                    }
                    _ => bail!("UNKNOWN_M_SEALED_ACTION {action}"),
                }
            }
            Stage::MDispatch => {
                // DISPATCH_BATCH: k shards from cursor..cursor+k
                let k = arg_int(&w0.args, "k")? as usize;
                let shard_tail = arg_bytes(&w0.args, "shardTail")?;
                let shard_meta = arg_bytes(&w0.args, "shardMeta")?;
                check((1..=K::MAX_BATCH).contains(&k), "DISPATCH_K")?;
                check(
                    (mgr.header.cursor as usize) + k <= mgr.header.opened_count as usize,
                    "DISPATCH_CURSOR_RANGE",
                )?;
                check(tx.inputs.len() > k && tx.outputs.len() > k, "DISPATCH_IO_COUNT")?;

                let winner_pk = mgr
                    .winner_pk
                    .clone()
                    .unwrap_or_else(|| K::ZERO32.to_string());
                let is_last = (mgr.header.cursor as usize) + k == mgr.header.opened_count as usize;
                let out_offset = if is_last { 0 } else { 1 };
                // When is_last == true, M_DISPATCH is destroyed and output 0 is creatorPk 4 KAS,
                // OR shards sit at 1..=k and creatorPk 4 KAS is at output 0. Keep shard outputs matched by index.
                let mut meta_pos = 0usize;
                for idx in 0..k {
                    let shard_idx = (mgr.header.cursor as usize) + idx;
                    let scid = &mgr.shard_cids[shard_idx];
                    let (next_shard_state, shard_out_idx) = if mgr.header.decision == Decision::Payout
                    {
                        check(meta_pos + 44 <= shard_meta.len(), "DISPATCH_PAYOUT_META")?;
                        let first_buyer_pk = hex(&shard_meta[meta_pos..meta_pos + 32]);
                        let vault_amount =
                            u64::from_le_bytes(shard_meta[meta_pos + 32..meta_pos + 40].try_into().unwrap())
                                as i128;
                        let shard_tickets =
                            u32::from_le_bytes(shard_meta[meta_pos + 40..meta_pos + 44].try_into().unwrap());
                        meta_pos += 44;
                        let st = ShardState::Payout {
                            version: K::VERSION,
                            first_buyer_pk,
                            winner_pk: winner_pk.clone(),
                            vault_amount,
                            manager_cid: cid.to_string(),
                            shard_tickets,
                        };
                        (st, 1 + idx)
                    } else {
                        check(meta_pos + 2 <= shard_meta.len(), "DISPATCH_REFUND_META")?;
                        let slen = u16::from_le_bytes(
                            shard_meta[meta_pos..meta_pos + 2].try_into().unwrap(),
                        ) as usize;
                        meta_pos += 2;
                        check(meta_pos + slen <= shard_meta.len(), "DISPATCH_REFUND_STATE")?;
                        let st = decode_shard_state(
                            Stage::SRefund,
                            &shard_meta[meta_pos..meta_pos + slen],
                        )?;
                        meta_pos += slen;
                        (st, 1 + idx)
                    };
                    let _ = out_offset;
                    let enc = encode_shard_state(&next_shard_state)?;
                    let redeem = script_for(&enc, shard_tail)?;
                    let (dec, _) = identify_redeem(&redeem, &self.hashes)?;
                    let expected_stage = if mgr.header.decision == Decision::Payout {
                        Stage::SPayout
                    } else {
                        Stage::SRefund
                    };
                    check(dec.stage() == expected_stage, "DISPATCH_SHARD_TEMPLATE")?;
                    let spk = redeem_p2sh(&redeem)?;
                    check(tx.outputs[shard_out_idx].spk == spk, "DISPATCH_SHARD_SPK")?;
                    let frozen_redeem_hex = if mgr.header.decision == Decision::Payout {
                        let from_prev = next
                            .get("shards")
                            .and_then(|s| s.get(scid))
                            .filter(|s| s.get("stage").and_then(|x| x.as_str()) == Some("S_FROZEN"))
                            .and_then(|s| s.get("redeemHex"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        from_prev.or_else(|| {
                            tx.inputs
                                .get(1 + idx)
                                .and_then(|inp| decode_spend(&inp.signature_script, &self.hashes).ok())
                                .filter(|w| w.state.stage() == Stage::SFrozen)
                                .map(|w| hex(&w.raw_redeem))
                        })
                    } else {
                        None
                    };
                    upsert_shard_node(
                        next,
                        scid,
                        &next_shard_state,
                        &redeem,
                        &Outpoint {
                            transaction_id: txid.to_string(),
                            index: shard_out_idx as i128,
                        },
                        tx.outputs[shard_out_idx].value,
                        &spk,
                        &b.daa,
                        None,
                        false,
                    );
                    if let Some(fhex) = frozen_redeem_hex
                        && let Some(shard_obj) = next
                            .get_mut("shards")
                            .and_then(|s| s.as_object_mut())
                            .and_then(|m| m.get_mut(scid))
                            .and_then(|v| v.as_object_mut())
                    {
                        shard_obj.insert("frozenRedeemHex".into(), json!(fhex));
                    }
                }

                if !is_last {
                    let mut hdr = mgr.header.clone();
                    hdr.cursor += k as u16;
                    let next_mgr = ManagerState {
                        header: hdr,
                        entries: vec![],
                        winner_pk: Some(winner_pk),
                        shard_cids: mgr.shard_cids.clone(),
                    };
                    update_manager_node(next, &next_mgr, &w0.tail, txid, 0, &tx.outputs[0], b)?;
                } else {
                    next.insert("manager".into(), Value::Null);
                }
                Ok(ReceiptMeta::simple("DISPATCH_BATCH"))
            }
            _ => bail!("UNEXPECTED_MANAGER_STAGE"),
        }
    }

    fn apply_shard_spend(
        &self,
        next: &mut Map<String, Value>,
        cid: &str,
        tx: &ParsedTx,
        txid: &str,
        b: &BlockRef,
    ) -> Result<ReceiptMeta> {
        let w0 = decode_spend(&tx.inputs[0].signature_script, &self.hashes)?;
        let ContractState::Shard(st) = w0.state else {
            bail!("EXPECTED_SHARD_INPUT");
        };
        let input_spk = redeem_p2sh(&w0.raw_redeem)?;
        let matched_scid = find_shard_by_outpoint_or_spk(
            next,
            &tx.inputs[0].previous_outpoint,
            &input_spk,
        )
        .ok_or_else(|| anyhow!("UNKNOWN_SHARD_FOR_ROUND {cid}"))?;

        match st {
            ShardState::OpenOrFrozen {
                stage: Stage::SOpen,
                version,
                manager_cid,
                first_buyer_pk,
                price,
                mut records,
            } => {
                let action = arg_int(&w0.args, "action")?;
                check(action == 1, "SHARD_STANDALONE_MUST_BE_BUY")?;
                let qty = arg_int(&w0.args, "qty")? as u32;
                let buyer_pk = hex(arg_bytes(&w0.args, "buyerPk")?);
                check(qty >= 1 && records.len() < K::MAX_RECORDS_PER_SHARD, "BUY_LIMITS")?;
                let prev_cum = records.last().map(|r| r.cum_tickets).unwrap_or(0);
                records.push(TicketRecord {
                    cum_tickets: prev_cum + qty,
                    buyer_pk: buyer_pk.clone(),
                });
                let record_index = records.len() - 1;
                let next_shard = ShardState::OpenOrFrozen {
                    stage: Stage::SOpen,
                    version,
                    manager_cid,
                    first_buyer_pk,
                    price,
                    records,
                };
                let next_redeem = script_for(&encode_shard_state(&next_shard)?, &w0.tail)?;
                let next_spk = redeem_p2sh(&next_redeem)?;
                check(tx.outputs.len() >= 2, "BUY_OUTPUTS")?;
                check(tx.outputs[0].spk == next_spk, "BUY_SHARD_OUT_SPK")?;
                let new_ticks = shard_tickets_from_state(&next_shard) as i128;
                upsert_shard_node(
                    next,
                    &matched_scid,
                    &next_shard,
                    &next_redeem,
                    &Outpoint {
                        transaction_id: txid.to_string(),
                        index: 0,
                    },
                    tx.outputs[0].value,
                    &next_spk,
                    &b.daa,
                    Some((
                        Outpoint {
                            transaction_id: txid.to_string(),
                            index: 1,
                        },
                        new_ticks * price,
                    )),
                    false,
                );
                record_wallet_role(next, &buyer_pk, "buyer");
                Ok(ReceiptMeta {
                    action: "BUY".into(),
                    shard_cid: Some(matched_scid),
                    record_index: Some(record_index),
                    rec_idx: None,
                })
            }
            ShardState::Payout {
                version,
                first_buyer_pk,
                winner_pk,
                vault_amount,
                manager_cid,
                shard_tickets,
            } => {
                let action = arg_int(&w0.args, "action")?;
                let st_keep = ShardState::Payout {
                    version,
                    first_buyer_pk,
                    winner_pk,
                    vault_amount,
                    manager_cid,
                    shard_tickets,
                };
                mark_shard_terminal(next, &matched_scid, &st_keep);
                Ok(ReceiptMeta {
                    action: if action == 1 {
                        "PAYOUT_EXECUTOR".into()
                    } else {
                        "PAYOUT_SELF".into()
                    },
                    shard_cid: Some(matched_scid),
                    record_index: None,
                    rec_idx: None,
                })
            }
            ShardState::Refund {
                version,
                first_buyer_pk,
                price,
                record_count,
                claimed_mask,
                records,
            } => {
                let rec_idx = arg_int(&w0.args, "recIdx")? as u8;
                let own_tail_bytes = arg_int(&w0.args, "ownTailBytes")?;
                check(rec_idx < record_count, "REFUND_REC_IDX")?;
                let bit = 1u8 << rec_idx;
                check((claimed_mask & bit) == 0, "REFUND_ALREADY_CLAIMED")?;
                let next_mask = claimed_mask | bit;
                let all_done = if record_count == 1 {
                    (next_mask & 1) == 1
                } else {
                    (next_mask & 3) == 3
                };
                let next_shard = ShardState::Refund {
                    version,
                    first_buyer_pk,
                    price,
                    record_count,
                    claimed_mask: next_mask,
                    records,
                };
                if all_done || own_tail_bytes == 0 {
                    mark_shard_terminal(next, &matched_scid, &next_shard);
                } else {
                    let next_redeem = script_for(&encode_shard_state(&next_shard)?, &w0.tail)?;
                    let next_spk = redeem_p2sh(&next_redeem)?;
                    check(tx.outputs.len() >= 2 && tx.outputs[0].spk == next_spk, "REFUND_OUT_SPK")?;
                    upsert_shard_node(
                        next,
                        &matched_scid,
                        &next_shard,
                        &next_redeem,
                        &Outpoint {
                            transaction_id: txid.to_string(),
                            index: 0,
                        },
                        tx.outputs[0].value,
                        &next_spk,
                        &b.daa,
                        None,
                        false,
                    );
                }
                Ok(ReceiptMeta {
                    action: "CLAIM_REFUND".into(),
                    shard_cid: Some(matched_scid),
                    record_index: None,
                    rec_idx: Some(rec_idx),
                })
            }
            _ => bail!("UNEXPECTED_STANDALONE_SHARD_STAGE"),
        }
    }
}

fn update_manager_node(
    next: &mut Map<String, Value>,
    m: &ManagerState,
    tail: &[u8],
    txid: &str,
    out_idx: i64,
    out: &ParsedOutput,
    b: &BlockRef,
) -> Result<()> {
    let redeem = script_for(&encode_manager_state(m)?, tail)?;
    let spk = redeem_p2sh(&redeem)?;
    check(out.spk == spk, "MANAGER_OUT_SPK")?;
    next.insert(
        "manager".into(),
        json!({
            "outpoint": {"transactionId": txid, "index": out_idx},
            "value": out.value.to_string(),
            "spk": spk_value(&spk),
            "utxoDaa": b.daa,
            "redeemHex": hex(&redeem),
            "state": m.to_json(),
        }),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn upsert_shard_node(
    next: &mut Map<String, Value>,
    shard_cid: &str,
    st: &ShardState,
    redeem: &[u8],
    outpoint: &Outpoint,
    value: i128,
    spk: &Spk,
    daa: &str,
    vault: Option<(Outpoint, i128)>,
    terminal: bool,
) {
    let shards = next
        .entry("shards")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .unwrap();
    let prev_vault = shards
        .get(shard_cid)
        .and_then(|v| v.get("vaultOutpoint"))
        .cloned();
    let prev_vault_amt = shards
        .get(shard_cid)
        .and_then(|v| v.get("vaultAmount"))
        .cloned();
    let prev_frozen_redeem = shards
        .get(shard_cid)
        .and_then(|v| v.get("frozenRedeemHex"))
        .filter(|v| !v.is_null())
        .cloned();
    let (v_op, v_amt) = match vault {
        Some((op, amt)) => (outpoint_json(&op), json!(amt.to_string())),
        None => (
            prev_vault.unwrap_or(Value::Null),
            prev_vault_amt.unwrap_or_else(|| {
                if let ShardState::Payout { vault_amount, .. } = st {
                    json!(vault_amount.to_string())
                } else {
                    Value::Null
                }
            }),
        ),
    };
    let first_buyer_pk = match st {
        ShardState::OpenOrFrozen { first_buyer_pk, .. } => first_buyer_pk.as_str(),
        ShardState::Payout { first_buyer_pk, .. } => first_buyer_pk.as_str(),
        ShardState::Refund { first_buyer_pk, .. } => first_buyer_pk.as_str(),
    };
    let tickets = shard_tickets_from_state(st);
    let mut entry = json!({
        "shardCid": shard_cid,
        "stage": st.stage().as_str(),
        "outpoint": outpoint_json(outpoint),
        "value": value.to_string(),
        "spk": spk_value(spk),
        "utxoDaa": daa,
        "redeemHex": hex(redeem),
        "firstBuyerPk": first_buyer_pk,
        "tickets": tickets,
        "vaultOutpoint": v_op,
        "vaultAmount": v_amt,
        "terminal": terminal,
        "state": st.to_json(),
    });
    if let Some(fhex) = prev_frozen_redeem
        && let Some(obj) = entry.as_object_mut()
    {
        obj.insert("frozenRedeemHex".into(), fhex);
    }
    shards.insert(shard_cid.to_string(), entry);
}

fn mark_shard_terminal(next: &mut Map<String, Value>, shard_cid: &str, st: &ShardState) {
    if let Some(shard) = next
        .get_mut("shards")
        .and_then(|s| s.as_object_mut())
        .and_then(|m| m.get_mut(shard_cid))
        .and_then(|v| v.as_object_mut())
    {
        shard.insert("terminal".into(), json!(true));
        shard.insert("outpoint".into(), Value::Null);
        shard.insert("stage".into(), json!(st.stage().as_str()));
        shard.insert("state".into(), st.to_json());
    }
}

fn find_shard_by_outpoint_or_spk(
    next: &Map<String, Value>,
    op: &Outpoint,
    spk: &Spk,
) -> Option<String> {
    let shards = next.get("shards")?.as_object()?;
    for (scid, s) in shards {
        if s.get("terminal").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        if let Some(sop) = s.get("outpoint").and_then(|o| outpoint_of(Some(o)).ok())
            && sop == *op
        {
            return Some(scid.clone());
        }
        if let Some(sspk) = s.get("spk")
            && spk_struct(sspk) == *spk
        {
            return Some(scid.clone());
        }
    }
    None
}

fn record_wallet_role(next: &mut Map<String, Value>, pk: &str, role: &str) {
    if pk.is_empty() || pk == K::ZERO32 {
        return;
    }
    let wallets = next
        .entry("wallets")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .unwrap();
    let arr = wallets
        .entry(pk.to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .unwrap();
    if !arr.iter().any(|x| x.as_str() == Some(role)) {
        arr.push(json!(role));
    }
}

fn refresh_round_head(
    next: &mut Map<String, Value>,
    txid: &str,
    tx_val: &Value,
    b: &BlockRef,
    block_hash: Option<Value>,
    fee: i128,
    meta: &ReceiptMeta,
) -> Result<()> {
    let old_step = next.get("step").and_then(|v| v.as_u64()).unwrap_or(1);
    next.insert("step".into(), json!(old_step + 1));
    let old_fees = js_bigint(next.get("fees")).unwrap_or(0);
    next.insert("fees".into(), json!((old_fees + fee).to_string()));
    next.insert("latestTxid".into(), json!(txid));
    next.insert("latestTransaction".into(), tx_val.clone());
    next.insert("accepting".into(), json!(b.hash));
    next.insert("acceptingDaa".into(), json!(b.daa));
    if let Some(h) = block_hash {
        next.insert("containing".into(), h);
    }

    let mgr_live = next.get("manager").filter(|v| !v.is_null()).cloned();
    let shards = next.get("shards").and_then(|s| s.as_object()).cloned().unwrap_or_default();
    let live_shard = shards
        .values()
        .find(|s| s.get("terminal").and_then(|v| v.as_bool()) != Some(true) && !s["outpoint"].is_null());

    if let Some(m) = mgr_live {
        next.insert("tip".into(), m["outpoint"].clone());
        next.insert("spk".into(), m["spk"].clone());
        next.insert("value".into(), m["value"].clone());
        next.insert("utxoDaa".into(), m["utxoDaa"].clone());
        next.insert("redeemHex".into(), m["redeemHex"].clone());
        if let Some(st) = m.get("state").and_then(|s| s.get("stage")).and_then(|x| x.as_str()) {
            next.insert("stage".into(), json!(st));
        }
    } else if let Some(s) = live_shard {
        // Manager destroyed; keep round LIVE on first remaining unsettled shard.
        next.insert("tip".into(), s["outpoint"].clone());
        next.insert("spk".into(), s["spk"].clone());
        next.insert("value".into(), s["value"].clone());
        next.insert("utxoDaa".into(), s["utxoDaa"].clone());
        next.insert("redeemHex".into(), s["redeemHex"].clone());
        next.insert("stage".into(), json!("SHARD_ONLY"));
    } else {
        // All manager & shards settled.
        next.insert("tip".into(), Value::Null);
        next.insert("utxoDaa".into(), json!(b.daa));
        if next.get("terminal").is_none_or(|v| v.is_null()) {
            let any_refund = shards
                .values()
                .any(|s| s.get("stage").and_then(|x| x.as_str()) == Some("S_REFUND"));
            let term = if shards.is_empty() {
                "EMPTY"
            } else if any_refund {
                "REFUNDED"
            } else {
                "PAID"
            };
            next.insert("terminal".into(), json!(term));
        }
        next.insert("stage".into(), json!("DONE"));
    }

    let mut receipt = json!({
        "txid": txid,
        "action": meta.action,
        "accepted": true,
        "candidate": false,
        "finalized": true,
        "acceptingBlock": b.hash,
        "acceptingDaa": b.daa,
        "feeSompi": fee.to_string(),
    });
    if let Some(obj) = receipt.as_object_mut() {
        if let Some(ref scid) = meta.shard_cid {
            obj.insert("shardCid".into(), json!(scid));
        }
        if let Some(idx) = meta.record_index {
            obj.insert("recordIndex".into(), json!(idx));
        }
        if let Some(ridx) = meta.rec_idx {
            obj.insert("recIdx".into(), json!(ridx));
        }
    }
    next.insert("_deltaReceipt".into(), receipt);
    Ok(())
}

/// Build v0.1.3.0-compatible summary fields (`profileId`, `asset`, `phase`, `subPhase`,
/// `settlementProgress`, `protocolData`) for a `kusdt-v2` round.
pub fn build_summary_fields(d: &Value, virtual_daa: Option<&str>) -> Value {
    let settings = &d["settings"];
    let kusdt_cid = settings
        .get("kusdtCid")
        .and_then(|v| v.as_str())
        .unwrap_or(K::MAINNET_KUSDT_CID);
    let profile_id = d
        .get("profileId")
        .and_then(|v| v.as_str())
        .unwrap_or(K::M_OPEN_HASH);
    let current_daa = virtual_daa
        .and_then(|s| s.parse::<i128>().ok())
        .unwrap_or(0);
    let close_daa = settings
        .get("closeDaa")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i128>().ok())
        .unwrap_or(i128::MAX);

    let shards_map = d.get("shards").and_then(|s| s.as_object());
    let settled_shards = shards_map
        .map(|m| {
            m.values()
                .filter(|s| s.get("terminal").and_then(|v| v.as_bool()) == Some(true))
                .count()
        })
        .unwrap_or(0);
    let live_shards: Vec<&Value> = shards_map
        .map(|m| {
            m.values()
                .filter(|s| s.get("terminal").and_then(|v| v.as_bool()) != Some(true))
                .collect()
        })
        .unwrap_or_default();

    let mgr_state = d.get("manager").and_then(|m| m.get("state")).filter(|v| !v.is_null());
    let opened_count = mgr_state
        .and_then(|m| m.get("openedCount"))
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| shards_map.map(|m| m.len() as u64).unwrap_or(0));
    let cursor = mgr_state
        .and_then(|m| m.get("cursor"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let mgr_total_tickets = mgr_state
        .and_then(|m| m.get("totalTickets"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let live_shard_tickets: u64 = shards_map
        .map(|m| {
            m.values()
                .filter_map(|s| s.get("tickets").and_then(|t| t.as_u64()))
                .sum()
        })
        .unwrap_or(0);
    let tickets_sold = mgr_total_tickets.max(live_shard_tickets);

    let terminal = d.get("terminal").and_then(|v| v.as_str());
    let mut winner_pk = d
        .get("winnerPk")
        .and_then(|v| v.as_str())
        .filter(|s| *s != K::ZERO32)
        .map(str::to_owned);

    let (phase, sub_phase, settlement_progress) = if let Some(term) = terminal {
        ("DONE", term, Value::Null)
    } else if let Some(ms) = mgr_state {
        match ms.get("stage").and_then(|v| v.as_str()) {
            Some("M_OPEN") => {
                if cursor > 0 || current_daa >= close_daa {
                    (
                        "CLOSING",
                        "collecting",
                        json!({"done": cursor, "total": opened_count, "labelKey": "COLLECT"}),
                    )
                } else {
                    ("OPEN", "m_open", Value::Null)
                }
            }
            Some("M_SEALED") => ("SEALED", "m_sealed", Value::Null),
            Some("M_DISPATCH") => {
                let total = opened_count.max((settled_shards + live_shards.len()) as u64);
                if ms.get("decision").and_then(|v| v.as_str()) == Some("REFUND") {
                    (
                        "REFUNDING",
                        "m_dispatch_refund",
                        json!({"done": settled_shards, "total": total, "labelKey": "REFUND"}),
                    )
                } else {
                    if let Some(w) = ms
                        .get("winnerPk")
                        .and_then(|v| v.as_str())
                        .filter(|s| *s != K::ZERO32)
                    {
                        winner_pk = Some(w.to_string());
                    }
                    (
                        "SETTLING",
                        "m_dispatch_payout",
                        json!({"done": settled_shards, "total": total, "labelKey": "PAYOUT"}),
                    )
                }
            }
            _ => ("OPEN", "m_open", Value::Null),
        }
    } else if !live_shards.is_empty() {
        let has_refund = live_shards
            .iter()
            .any(|s| s.get("stage").and_then(|v| v.as_str()) == Some("S_REFUND"));
        let total = settled_shards + live_shards.len();
        if has_refund {
            (
                "REFUNDING",
                "s_refund",
                json!({"done": settled_shards, "total": total, "labelKey": "REFUND"}),
            )
        } else {
            (
                "SETTLING",
                "s_payout",
                json!({"done": settled_shards, "total": total, "labelKey": "PAYOUT"}),
            )
        }
    } else {
        ("OPEN", "m_open", Value::Null)
    };

    let shards_summary: Vec<Value> = shards_map
        .map(|m| {
            m.iter()
                .map(|(scid, s)| {
                    json!({
                        "shardCid": scid,
                        "terminal": s.get("terminal").and_then(|v| v.as_bool()).unwrap_or(false),
                        "firstBuyerPk": s.get("firstBuyerPk").cloned().unwrap_or(Value::Null),
                        "tickets": s.get("tickets").cloned().unwrap_or(Value::Null),
                        "vaultAmount": s.get("vaultAmount").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    json!({
        "protocolId": K::PROTOCOL_ID,
        "profileId": profile_id,
        "asset": {
            "kind": "KCC20",
            "symbol": "kUSDT",
            "decimals": 6,
            "tokenCovenantId": kusdt_cid,
        },
        "phase": phase,
        "subPhase": sub_phase,
        "terminal": d.get("terminal").cloned().unwrap_or(Value::Null),
        "ticketsSold": tickets_sold,
        "ticketCap": K::MAX_SHARDS * K::MAX_RECORDS_PER_SHARD,
        "ticketPrice": settings.get("ticketPrice").cloned().unwrap_or(Value::Null),
        "settlementProgress": settlement_progress,
        "protocolData": {
            "openedCount": opened_count,
            "cursor": cursor,
            "maxBatch": K::MAX_BATCH,
            "minDrawTickets": K::MIN_DRAW_TICKETS,
            "closeDaa": settings.get("closeDaa").cloned().unwrap_or(Value::Null),
            "creatorPk": settings.get("creatorPk").cloned().unwrap_or(Value::Null),
            "winnerPk": winner_pk.map(|w| json!(w)).unwrap_or(Value::Null),
            "shards": shards_summary,
        }
    })
}

/// Collect wallet roles for `pk` in a `kusdt-v2` round (`buyer`, `creator`, `firstBuyerDeposit`).
pub fn wallet_roles_in_round(d: &Value, pk: &str) -> Vec<String> {
    let mut roles = BTreeSet::new();
    if let Some(arr) = d
        .get("wallets")
        .and_then(|w| w.get(pk))
        .and_then(|a| a.as_array())
    {
        for r in arr {
            if let Some(s) = r.as_str() {
                roles.insert(s.to_string());
            }
        }
    }
    if d.pointer("/settings/creatorPk").and_then(|v| v.as_str()) == Some(pk) {
        roles.insert("creator".into());
    }
    if let Some(shards) = d.get("shards").and_then(|s| s.as_object()) {
        for s in shards.values() {
            if s.get("firstBuyerPk").and_then(|v| v.as_str()) == Some(pk) {
                roles.insert("firstBuyerDeposit".into());
            }
            if let Some(recs) = s.pointer("/state/records").and_then(|r| r.as_array()) {
                for rec in recs {
                    if rec.get("buyerPk").and_then(|v| v.as_str()) == Some(pk) {
                        roles.insert("buyer".into());
                    }
                }
            }
        }
    }
    roles.into_iter().collect()
}

/// Look up live UTXO + `redeemHex` by `covenantId` (either manager `cid` or a shard `shardCid`).
pub fn find_live_covenant(d: &Value, target_cid: &str, prefix: &str) -> Option<Value> {
    if d.get("cid").and_then(|v| v.as_str()) == Some(target_cid) {
        let mgr = d.get("manager").filter(|m| !m.is_null())?;
        let op = mgr.get("outpoint").filter(|o| !o.is_null())?;
        let spk = spk_struct(mgr.get("spk").unwrap_or(&Value::Null));
        let address = spk_to_address(spk.version as i64, &spk.script, prefix);
        return Some(json!({
            "role": "manager",
            "roundCid": target_cid,
            "covenantId": target_cid,
            "stage": mgr.pointer("/state/stage").cloned().unwrap_or_else(|| d["stage"].clone()),
            "outpoint": op,
            "value": mgr["value"],
            "daaScore": mgr["utxoDaa"],
            "spk": mgr["spk"],
            "address": address,
            "redeemHex": mgr["redeemHex"],
        }));
    }
    let shard = d
        .get("shards")
        .and_then(|s| s.as_object())
        .and_then(|m| m.get(target_cid))?;
    if shard.get("terminal").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    let op = shard.get("outpoint").filter(|o| !o.is_null())?;
    let spk = spk_struct(shard.get("spk").unwrap_or(&Value::Null));
    let address = spk_to_address(spk.version as i64, &spk.script, prefix);
    Some(json!({
        "role": "shard",
        "roundCid": d["cid"],
        "covenantId": target_cid,
        "stage": shard["stage"],
        "outpoint": op,
        "value": shard["value"],
        "daaScore": shard["utxoDaa"],
        "spk": shard["spk"],
        "address": address,
        "redeemHex": shard["redeemHex"],
    }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveUtxoTarget {
    pub outpoint: Value,
    pub spk: Value,
    pub address: String,
    pub utxo_daa: Option<String>,
    pub value: Option<String>,
    pub is_manager: bool,
    pub shard_cid: Option<String>,
}

/// Active mutable `(outpoint, address)` watch set for a `LIVE` `kusdt-v2` round:
/// - `manager` when non-null with a valid outpoint
/// - non-terminal shards that can be spent independently without consuming manager:
///   - `S_OPEN` with `recordCount < 256`
///   - `S_PAYOUT` / `S_REFUND`
/// Explicitly excludes `S_FROZEN`, `terminal == true`, and full (`>= 256` records) `S_OPEN`.
pub fn active_watch_targets(d: &Value, prefix: &str) -> Vec<ActiveUtxoTarget> {
    let mut out = Vec::new();
    if let Some(mgr) = d.get("manager").filter(|m| !m.is_null())
        && let Some(op) = mgr.get("outpoint").filter(|o| !o.is_null())
        && let Some(spk_val) = mgr.get("spk").filter(|s| !s.is_null())
    {
        let spk = spk_struct(spk_val);
        if let Some(addr) = spk_to_address(spk.version as i64, &spk.script, prefix)
            .filter(|a| !a.is_empty())
        {
            out.push(ActiveUtxoTarget {
                outpoint: op.clone(),
                spk: spk_val.clone(),
                address: addr,
                utxo_daa: mgr
                    .get("utxoDaa")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                value: mgr.get("value").and_then(|v| v.as_str()).map(str::to_string),
                is_manager: true,
                shard_cid: None,
            });
        }
    }
    if let Some(shards) = d.get("shards").and_then(|s| s.as_object()) {
        for (scid, shard) in shards {
            if shard.get("terminal").and_then(|v| v.as_bool()) == Some(true) {
                continue;
            }
            let Some(op) = shard.get("outpoint").filter(|o| !o.is_null()) else {
                continue;
            };
            let stage = shard.get("stage").and_then(|v| v.as_str()).unwrap_or("");
            let eligible = match stage {
                "S_OPEN" => {
                    let rec_count = shard
                        .pointer("/state/records")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .or_else(|| {
                            shard
                                .pointer("/state/recordCount")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as usize)
                        })
                        .unwrap_or(0);
                    rec_count < K::MAX_RECORDS_PER_SHARD
                }
                "S_PAYOUT" | "S_REFUND" => true,
                _ => false,
            };
            if !eligible {
                continue;
            }
            let Some(spk_val) = shard.get("spk").filter(|s| !s.is_null()) else {
                continue;
            };
            let spk = spk_struct(spk_val);
            if let Some(addr) = spk_to_address(spk.version as i64, &spk.script, prefix)
                .filter(|a| !a.is_empty())
            {
                out.push(ActiveUtxoTarget {
                    outpoint: op.clone(),
                    spk: spk_val.clone(),
                    address: addr,
                    utxo_daa: shard
                        .get("utxoDaa")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    value: shard
                        .get("value")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    is_manager: false,
                    shard_cid: Some(scid.clone()),
                });
            }
        }
    }
    out
}

fn is_shard_level_action(action: &str) -> bool {
    matches!(
        action,
        "OPEN_SHARD" | "BUY" | "CLAIM_REFUND" | "PAYOUT_EXECUTOR" | "PAYOUT_SELF"
    )
}

pub fn receipt_needs_enrichment(r: &Value) -> bool {
    let Some(action) = r.get("action").and_then(|v| v.as_str()) else {
        return false;
    };
    if !is_shard_level_action(action) {
        return false;
    }
    if r.get("shardCid").and_then(|v| v.as_str()).is_none() {
        return true;
    }
    match action {
        "OPEN_SHARD" | "BUY" => r.get("recordIndex").is_none(),
        "CLAIM_REFUND" => r.get("recIdx").is_none(),
        _ => false,
    }
}

/// Enrich stored `kusdt-v2` receipts with `shardCid`, `recordIndex`, and `recIdx` from
/// `transitions` (unpruned `previous`/`next` or `latestTransaction`) or single-shard fallback.
pub fn enrich_kusdt_v2_receipts(
    round_data: &Value,
    raw_receipts: Vec<Value>,
    transitions: &[crate::store::TransitionRow],
) -> Vec<Value> {
    let mut receipts = if raw_receipts.is_empty() && !transitions.is_empty() {
        transitions
            .iter()
            .map(|tr| {
                json!({
                    "txid": tr.txid,
                    "action": tr.kind,
                    "accepted": true,
                    "candidate": false,
                    "finalized": tr.status == "FINAL",
                    "acceptingBlock": tr.accepting,
                    "acceptingDaa": tr.accepting_daa,
                    "feeSompi": "0",
                })
            })
            .collect()
    } else {
        raw_receipts
    };

    let shards_obj = round_data.get("shards").and_then(|s| s.as_object());
    let single_shard = shards_obj.filter(|m| m.len() == 1).and_then(|m| {
        let (scid, s) = m.iter().next()?;
        Some((scid.as_str(), s))
    });
    let hashes = TemplateHashes::default();

    for receipt in &mut receipts {
        if !receipt_needs_enrichment(receipt) {
            continue;
        }
        let Some(obj) = receipt.as_object_mut() else {
            continue;
        };
        let txid = obj
            .get("txid")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let action = obj
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // 1. Try matching transition's unpruned `previous`/`next` or `latestTransaction`.
        if let Some(tr) = transitions.iter().find(|t| t.txid == txid) {
            if let Some(latest_tx) = tr.next.get("latestTransaction")
                && let Some(sig0) = latest_tx
                    .pointer("/inputs/0/signatureScript")
                    .and_then(|v| v.as_str())
                && let Ok(w0) = decode_spend(sig0, &hashes)
            {
                if action == "CLAIM_REFUND"
                    && obj.get("recIdx").is_none()
                    && let Ok(ridx) = arg_int(&w0.args, "recIdx")
                {
                    obj.insert("recIdx".into(), json!(ridx));
                }
                if obj.get("shardCid").is_none()
                    && w0.state.stage().is_shard()
                    && let Ok(in_spk) = redeem_p2sh(&w0.raw_redeem)
                    && let Some(prev_shards) = tr.previous.get("shards").and_then(|s| s.as_object())
                {
                    for (scid, sv) in prev_shards {
                        if sv.get("spk").is_some_and(|x| spk_struct(x) == in_spk) {
                            obj.insert("shardCid".into(), json!(scid));
                            break;
                        }
                    }
                }
            }

            if let (Some(prev_shards), Some(next_shards)) = (
                tr.previous.get("shards").and_then(|s| s.as_object()),
                tr.next.get("shards").and_then(|s| s.as_object()),
            ) {
                if action == "OPEN_SHARD" && obj.get("shardCid").is_none() {
                    for scid in next_shards.keys() {
                        if !prev_shards.contains_key(scid) {
                            obj.insert("shardCid".into(), json!(scid));
                            break;
                        }
                    }
                } else if action == "BUY" {
                    for (scid, ns) in next_shards {
                        let new_len = ns
                            .pointer("/state/records")
                            .and_then(|r| r.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0);
                        let old_len = prev_shards
                            .get(scid)
                            .and_then(|ps| ps.pointer("/state/records"))
                            .and_then(|r| r.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0);
                        if new_len > old_len && new_len > 0 {
                            if obj.get("shardCid").is_none() {
                                obj.insert("shardCid".into(), json!(scid));
                            }
                            if obj.get("recordIndex").is_none() {
                                obj.insert("recordIndex".into(), json!(new_len - 1));
                            }
                            break;
                        }
                    }
                } else if action == "CLAIM_REFUND" && obj.get("recIdx").is_none() {
                    for (scid, ns) in next_shards {
                        let new_mask = ns
                            .pointer("/state/claimedMask")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let old_mask = prev_shards
                            .get(scid)
                            .and_then(|ps| ps.pointer("/state/claimedMask"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let diff = new_mask ^ old_mask;
                        if diff != 0 {
                            if obj.get("shardCid").is_none() {
                                obj.insert("shardCid".into(), json!(scid));
                            }
                            obj.insert("recIdx".into(), json!(diff.trailing_zeros()));
                            break;
                        }
                    }
                }
            }
        }

        // 2. Fallback for already-compacted historical single-shard rounds.
        if let Some((scid, s)) = single_shard {
            if obj.get("shardCid").is_none() {
                obj.insert("shardCid".into(), json!(scid));
            }
            if action == "OPEN_SHARD" && obj.get("recordIndex").is_none() {
                obj.insert("recordIndex".into(), json!(0));
            }
            if action == "CLAIM_REFUND" && obj.get("recIdx").is_none() {
                let rec_count = s
                    .pointer("/state/recordCount")
                    .and_then(|v| v.as_u64())
                    .or_else(|| {
                        s.pointer("/state/records")
                            .and_then(|r| r.as_array())
                            .map(|a| a.len() as u64)
                    })
                    .unwrap_or(0);
                if rec_count == 1 {
                    obj.insert("recIdx".into(), json!(0));
                }
            }
        } else if action == "OPEN_SHARD" && obj.get("recordIndex").is_none() {
            obj.insert("recordIndex".into(), json!(0));
        }
    }

    receipts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_maps_phases_and_roles() {
        let round = json!({
            "cid": "aa".repeat(32),
            "profileId": K::M_OPEN_HASH,
            "terminal": null,
            "winnerPk": null,
            "settings": {
                "kusdtCid": K::MAINNET_KUSDT_CID,
                "creatorPk": "11".repeat(32),
                "ticketPrice": "1000000",
                "closeDaa": "5000",
            },
            "manager": {
                "outpoint": {"transactionId": "bb".repeat(32), "index": 0},
                "value": "400000000",
                "utxoDaa": "4000",
                "spk": {"version": 0, "script": "aa20" },
                "redeemHex": "00",
                "state": {
                    "stage": "M_OPEN",
                    "openedCount": 2,
                    "cursor": 1,
                    "totalTickets": 3,
                }
            },
            "shards": {
                "cc": {
                    "shardCid": "cc",
                    "stage": "S_FROZEN",
                    "terminal": false,
                    "firstBuyerPk": "22".repeat(32),
                    "tickets": 3,
                    "vaultAmount": "3000000",
                    "outpoint": {"transactionId": "dd".repeat(32), "index": 1},
                    "value": "200000000",
                    "utxoDaa": "4100",
                    "spk": {"version": 0, "script": "bb20"},
                    "redeemHex": "01",
                    "state": {
                        "records": [{"cumTickets": 3, "buyerPk": "22".repeat(32)}]
                    }
                }
            },
            "wallets": {}
        });
        let s = build_summary_fields(&round, Some("4500"));
        assert_eq!(s["phase"], "CLOSING");
        assert_eq!(s["subPhase"], "collecting");
        assert_eq!(s["settlementProgress"]["done"], 1);
        assert_eq!(s["settlementProgress"]["total"], 2);
        assert_eq!(s["asset"]["symbol"], "kUSDT");
        assert_eq!(s["protocolData"]["maxBatch"], 8);

        let roles_creator = wallet_roles_in_round(&round, &"11".repeat(32));
        assert_eq!(roles_creator, vec!["creator"]);
        let roles_buyer = wallet_roles_in_round(&round, &"22".repeat(32));
        assert_eq!(roles_buyer, vec!["buyer", "firstBuyerDeposit"]);
    }

    #[test]
    fn genesis_requires_payload_and_populates_settings_and_watch_addresses() {
        let plugin = KusdtV2::create(&json!({}), Some("tn10")).unwrap();
        assert_eq!(plugin.watch_addresses(), vec![plugin.network.registry.to_string()]);

        let init = ManagerState {
            header: K::ManagerHeader {
                stage: Stage::MOpen,
                version: K::VERSION,
                creator_pk: "11".repeat(32),
                price: K::PRICE_MIN,
                close_daa: 630_000_000,
                opened_count: 0,
                cursor: 0,
                total_tickets: 0,
                decision: Decision::Undecided,
                reserved: 0,
            },
            entries: vec![],
            winner_pk: None,
            shard_cids: vec![],
        };
        let tail = pinned_m_open_tail().unwrap();
        let state_bytes = encode_manager_state(&init).unwrap();
        let redeem = script_for(&state_bytes, &tail).unwrap();
        let spk = redeem_p2sh(&redeem).unwrap();
        let origin = Outpoint {
            transaction_id: "aa".repeat(32),
            index: 0,
        };
        let cid = covenant_id(
            &origin,
            &[crate::core::covenant::AuthorizedOutput {
                index: 0,
                value: K::MANAGER_DEPOSIT,
                spk: &spk,
            }],
        )
        .unwrap();

        let mut payload = Vec::new();
        payload.extend_from_slice(K::GENESIS_PAYLOAD_MAGIC);
        payload.extend_from_slice(&unhex_len(K::M_OPEN_HASH, 32).unwrap());
        payload.extend_from_slice(&state_bytes);

        let tx_with_payload = json!({
            "version": 1,
            "lockTime": "0",
            "payload": hex(&payload),
            "inputs": [{
                "previousOutpoint": {"transactionId": origin.transaction_id, "index": 0},
                "signatureScript": "",
                "verboseData": {"utxoEntry": {"amount": "420100000"}}
            }],
            "outputs": [
                {
                    "value": K::MANAGER_DEPOSIT.to_string(),
                    "scriptPublicKey": {"version": spk.version, "script": spk.script},
                    "covenant": {"covenantId": cid, "authorizingInput": 0}
                },
                {
                    "value": "20000000",
                    "scriptPublicKey": {"version": 0, "script": "51"}
                }
            ],
            "verboseData": {"transactionId": "bb".repeat(32), "blockHash": "cc".repeat(32)}
        });

        assert!(plugin.inspect_genesis(&tx_with_payload).0);
        let round = plugin
            .genesis(
                &tx_with_payload,
                &BlockRef {
                    hash: "dd".repeat(32),
                    daa: "630000001".into(),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(round["settings"]["creatorPk"], "11".repeat(32));
        assert_eq!(round["settings"]["ticketPrice"], K::PRICE_MIN.to_string());
        assert_eq!(round["settings"]["closeDaa"], "630000000");
        assert_eq!(round["manager"]["redeemHex"], hex(&redeem));
        assert_eq!(round["manager"]["state"]["stage"], "M_OPEN");

        // Without payload (e.g. Rolling v1 or legacy tx), must be rejected cleanly.
        let mut tx_no_payload = tx_with_payload.clone();
        tx_no_payload.as_object_mut().unwrap().remove("payload");
        assert!(!plugin.inspect_genesis(&tx_no_payload).0);
    }

    #[test]
    fn open_shard_derives_shard_cid_from_input1_and_updates_summary() {
        let s_open_artifact: Value = serde_json::from_str(
            &std::fs::read_to_string("/root/kaspa/kaswin/contracts/kusdt-v2/artifacts/s_open.json")
                .unwrap(),
        )
        .unwrap();
        let bc: Vec<u8> = s_open_artifact["contracts"]["KusdtShardOpen"]["compiled"]["bytecode"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u8)
            .collect();
        let (_, s_open_tail) = K::split_redeem_script(&bc).unwrap();
        let s_open_state = ShardState::OpenOrFrozen {
            stage: Stage::SOpen,
            version: K::VERSION,
            manager_cid: "906a35ad9cb415cd56a6bf5d136aa3928ca07c017b40701c227704e36ae30eaa".into(),
            first_buyer_pk: "11".repeat(32),
            price: K::PRICE_MIN,
            records: vec![TicketRecord {
                cum_tickets: 1,
                buyer_pk: "11".repeat(32),
            }],
        };
        let input0 = Outpoint {
            transaction_id: "26a63c6e28ba0a42089eb1195a0d63026b8674e4aa59c3906bc6a19fe826d283"
                .into(),
            index: 0,
        };
        let input1 = Outpoint {
            transaction_id: "9c111f586c97bd264727fb2f0dfdea73c29b0c555780ea483e524bbb4e96d4c0"
                .into(),
            index: 1,
        };
        let cid_from_0 = shard_genesis_cid(&input0, 1, &s_open_state, &s_open_tail).unwrap();
        let cid_from_1 = shard_genesis_cid(&input1, 1, &s_open_state, &s_open_tail).unwrap();
        assert_ne!(cid_from_0, cid_from_1);
        assert_eq!(cid_from_1.len(), 64);
    }

    #[test]
    fn enrich_receipts_recovers_shard_and_record_indices() {
        let round_data = json!({
            "cid": "90".repeat(32),
            "shards": {
                "8f".repeat(32): {
                    "shardCid": "8f".repeat(32),
                    "stage": "S_REFUND",
                    "terminal": true,
                    "state": {
                        "recordCount": 1,
                        "claimedMask": 1,
                        "records": [{"cumTickets": 1, "buyerPk": "11".repeat(32)}]
                    }
                }
            }
        });
        let raw_receipts = vec![
            json!({"txid": "aa".repeat(32), "action": "GENESIS", "feeSompi": "6000000"}),
            json!({"txid": "bb".repeat(32), "action": "OPEN_SHARD", "feeSompi": "6284300"}),
            json!({"txid": "cc".repeat(32), "action": "CLAIM_REFUND", "feeSompi": "2866700"}),
        ];
        let enriched = enrich_kusdt_v2_receipts(&round_data, raw_receipts, &[]);
        assert_eq!(enriched[0].get("shardCid"), None);
        assert_eq!(enriched[1]["shardCid"], "8f".repeat(32));
        assert_eq!(enriched[1]["recordIndex"], 0);
        assert_eq!(enriched[2]["shardCid"], "8f".repeat(32));
        assert_eq!(enriched[2]["recIdx"], 0);

        // Multi-shard BUY diff recovery from unpruned transition
        let multi_round = json!({
            "cid": "90".repeat(32),
            "shards": {
                "11".repeat(32): {},
                "22".repeat(32): {},
            }
        });
        let buy_txid = "dd".repeat(32);
        let tr = crate::store::TransitionRow {
            txid: buy_txid.clone(),
            round_id: "aa".repeat(32),
            contract: Some("kusdt-v2@1.0.0".into()),
            seq: 2,
            kind: "BUY".into(),
            status: "ACCEPTED".into(),
            accepting: "ee".repeat(32),
            accepting_daa: "100".into(),
            accepting_blue_score: "100".into(),
            containing: None,
            previous: json!({
                "shards": {
                    "11".repeat(32): {"state": {"records": [{"cumTickets": 1}]}},
                    "22".repeat(32): {"state": {"records": [{"cumTickets": 1}]}},
                }
            }),
            next: json!({
                "shards": {
                    "11".repeat(32): {"state": {"records": [{"cumTickets": 1}]}},
                    "22".repeat(32): {"state": {"records": [{"cumTickets": 1}, {"cumTickets": 3}]}},
                }
            }),
            evidence: json!({}),
            observed_at: 1,
        };
        let multi_enriched = enrich_kusdt_v2_receipts(
            &multi_round,
            vec![json!({"txid": buy_txid, "action": "BUY", "feeSompi": "20000"})],
            &[tr],
        );
        assert_eq!(multi_enriched[0]["shardCid"], "22".repeat(32));
        assert_eq!(multi_enriched[0]["recordIndex"], 1);
    }

    fn load_artifact_tail(file: &str, contract_key: &str) -> Vec<u8> {
        let path = format!("/root/kaspa/kaswin/contracts/kusdt-v2/artifacts/{file}.json");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let bc: Vec<u8> = v["contracts"][contract_key]["compiled"]["bytecode"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as u8)
            .collect();
        let (_, tail) = K::split_redeem_script(&bc).unwrap();
        tail
    }

    fn push_bytes(out: &mut Vec<u8>, data: &[u8]) {
        let n = data.len();
        if n == 0 {
            out.push(0x00);
        } else if n <= 75 {
            out.push(n as u8);
            out.extend_from_slice(data);
        } else if n <= 0xff {
            out.push(0x4c);
            out.push(n as u8);
            out.extend_from_slice(data);
        } else {
            out.push(0x4d);
            out.extend_from_slice(&(n as u16).to_le_bytes());
            out.extend_from_slice(data);
        }
    }

    fn push_int(out: &mut Vec<u8>, n: i64) {
        if n == 0 {
            out.push(0x00);
        } else if (1..=16).contains(&n) {
            out.push(0x50 + n as u8);
        } else {
            let b = n.to_le_bytes();
            let mut len = 8;
            while len > 1 && b[len - 1] == 0 {
                len -= 1;
            }
            push_bytes(out, &b[..len]);
        }
    }

    #[test]
    fn draw_records_hit_info_with_real_artifacts() {
        use crate::core::kusdt_v2::ManagerHeader;
        let m_sealed_tail = load_artifact_tail("m_sealed", "KusdtManagerSealed");
        let m_dispatch_tail = load_artifact_tail("m_dispatch", "KusdtManagerDispatch");
        let cid = "aa".repeat(32);
        let scid = "bb".repeat(32);
        let creator_pk = "11".repeat(32);
        let buyer0_pk = "22".repeat(32);
        let buyer1_pk = "33".repeat(32);

        let frozen_state = ShardState::OpenOrFrozen {
            stage: Stage::SFrozen,
            version: K::VERSION,
            manager_cid: cid.clone(),
            first_buyer_pk: buyer0_pk.clone(),
            price: K::PRICE_MIN,
            records: vec![
                TicketRecord {
                    cum_tickets: 1,
                    buyer_pk: buyer0_pk.clone(),
                },
                TicketRecord {
                    cum_tickets: 3,
                    buyer_pk: buyer1_pk.clone(),
                },
            ],
        };
        let frozen_bytes = encode_shard_state(&frozen_state).unwrap();
        let frozen_hash = hex(&blake2b256(&frozen_bytes));

        let mgr_state = ManagerState {
            header: ManagerHeader {
                stage: Stage::MSealed,
                version: K::VERSION,
                creator_pk: creator_pk.clone(),
                price: K::PRICE_MIN,
                close_daa: 900,
                opened_count: 1,
                cursor: 0,
                total_tickets: 3,
                decision: Decision::Undecided,
                reserved: 0,
            },
            entries: vec![ManagerEntry {
                shard_cid: scid.clone(),
                ledger_hash: frozen_hash,
                cum_tickets: 3,
            }],
            winner_pk: None,
            shard_cids: vec![],
        };
        let mgr_redeem = script_for(&encode_manager_state(&mgr_state).unwrap(), &m_sealed_tail).unwrap();
        let mgr_spk = redeem_p2sh(&mgr_redeem).unwrap();
        let mgr_op = Outpoint {
            transaction_id: "cc".repeat(32),
            index: 0,
        };

        // Build 240B Pass-A proof
        let mut proof = vec![0u8; 240];
        proof[0..32].fill(0x11);
        proof[32..64].fill(0x22);
        proof[64..96].fill(0x33);
        proof[96..104].copy_from_slice(&1u64.to_le_bytes());
        proof[104..112].copy_from_slice(&1000u64.to_le_bytes());
        proof[112..120].copy_from_slice(&200u64.to_le_bytes());
        proof[120..152].fill(0x44);
        proof[152..184].fill(0x55);
        proof[184..216].fill(0x66);
        proof[216..224].copy_from_slice(&2u64.to_le_bytes());
        proof[224..232].copy_from_slice(&999u64.to_le_bytes());
        proof[232..240].copy_from_slice(&199u64.to_le_bytes());
        let utxo_daa = 1000 - K::DRAW_DELAY_DAA;

        let seq = {
            let ctx1 = crate::core::hashes::domain_hash("SeqCommitMergesetContext", &proof[216..240]).unwrap();
            let r1 = crate::core::hashes::domain_hash("SeqCommitmentMerkleBranchHash", &crate::core::bytes::cat(&[&ctx1, &proof[184..216]])).unwrap();
            let m1 = crate::core::hashes::domain_hash("SeqCommitmentMerkleBranchHash", &crate::core::bytes::cat(&[&proof[152..184], &r1])).unwrap();
            let seq1 = crate::core::hashes::domain_hash("SeqCommitmentMerkleBranchHash", &crate::core::bytes::cat(&[&proof[120..152], &m1])).unwrap();
            let ctx0 = crate::core::hashes::domain_hash("SeqCommitMergesetContext", &proof[96..120]).unwrap();
            let r0 = crate::core::hashes::domain_hash("SeqCommitmentMerkleBranchHash", &crate::core::bytes::cat(&[&ctx0, &proof[64..96]])).unwrap();
            let m0 = crate::core::hashes::domain_hash("SeqCommitmentMerkleBranchHash", &crate::core::bytes::cat(&[&proof[32..64], &r0])).unwrap();
            crate::core::hashes::domain_hash("SeqCommitmentMerkleBranchHash", &crate::core::bytes::cat(&[&seq1, &m0])).unwrap()
        };
        let acc = Accessor {
            block_hash: hex(&proof[0..32]),
            sequence_commitment: hex(&seq),
        };
        let ticket = draw_sample(&cid, &mgr_op, utxo_daa, 1000, 3, &proof, &acc).unwrap();
        let rec_idx: u16 = if ticket < 1 { 0 } else { 1 };
        let expected_winner = if rec_idx == 0 {
            buyer0_pk.clone()
        } else {
            buyer1_pk.clone()
        };

        let mut data_arg = Vec::new();
        data_arg.extend_from_slice(&proof);
        data_arg.extend_from_slice(&0u16.to_le_bytes());
        data_arg.extend_from_slice(&rec_idx.to_le_bytes());
        data_arg.extend_from_slice(&frozen_bytes);

        let mut sig_script = Vec::new();
        push_int(&mut sig_script, 1); // action = 1 (DRAW)
        push_int(&mut sig_script, m_sealed_tail.len() as i64);
        push_bytes(&mut sig_script, &m_dispatch_tail);
        push_bytes(&mut sig_script, &data_arg);
        push_bytes(&mut sig_script, &mgr_redeem);

        let next_mgr = ManagerState {
            header: ManagerHeader {
                stage: Stage::MDispatch,
                version: K::VERSION,
                creator_pk: creator_pk.clone(),
                price: K::PRICE_MIN,
                close_daa: 900,
                opened_count: 1,
                cursor: 0,
                total_tickets: 3,
                decision: Decision::Payout,
                reserved: 0,
            },
            entries: vec![],
            winner_pk: Some(expected_winner.clone()),
            shard_cids: vec![scid.clone()],
        };
        let next_mgr_redeem = script_for(&encode_manager_state(&next_mgr).unwrap(), &m_dispatch_tail).unwrap();
        let next_mgr_spk = redeem_p2sh(&next_mgr_redeem).unwrap();

        let round = json!({
            "cid": cid,
            "profileId": K::M_OPEN_HASH,
            "stage": "M_SEALED",
            "manager": {
                "outpoint": {"transactionId": mgr_op.transaction_id, "index": 0},
                "value": "400000000",
                "spk": {"version": mgr_spk.version, "script": mgr_spk.script},
                "utxoDaa": utxo_daa.to_string(),
                "redeemHex": hex(&mgr_redeem),
            },
            "shards": {},
            "wallets": {},
        });
        let draw_txid = "dd".repeat(32);
        let tx = json!({
            "version": 1,
            "lockTime": "0",
            "inputs": [{
                "previousOutpoint": {"transactionId": mgr_op.transaction_id, "index": 0},
                "signatureScript": hex(&sig_script),
                "verboseData": {"utxoEntry": {"amount": "400000000"}}
            }],
            "outputs": [{
                "value": "399990000",
                "scriptPublicKey": {"version": next_mgr_spk.version, "script": next_mgr_spk.script}
            }],
            "verboseData": {
                "transactionId": draw_txid,
                "blockHash": "ee".repeat(32),
            }
        });
        let plugin = KusdtV2::create(&json!({}), Some("mainnet")).unwrap();
        let b = BlockRef {
            hash: "ee".repeat(32),
            daa: "1000".into(),
        };
        let next = plugin.advance_with_accessor(&round, &tx, &b, Some(&acc)).unwrap();
        assert_eq!(next["draw"]["txid"], draw_txid);
        assert_eq!(next["draw"]["shard"], 0);
        assert_eq!(next["draw"]["record"], rec_idx);
        assert_eq!(next["draw"]["ticket"], ticket);
        assert_eq!(next["draw"]["winnerPk"], expected_winner);
    }

    #[test]
    fn dispatch_payout_and_payout_self_preserve_frozen_redeem_hex() {
        use crate::core::kusdt_v2::ManagerHeader;
        let m_dispatch_tail = load_artifact_tail("m_dispatch", "KusdtManagerDispatch");
        let s_frozen_tail = load_artifact_tail("s_frozen", "KusdtShardFrozen");
        let s_payout_tail = load_artifact_tail("s_payout", "KusdtShardPayout");

        let cid = "aa".repeat(32);
        let scid = "bb".repeat(32);
        let creator_pk = "11".repeat(32);
        let first_buyer_pk = "22".repeat(32);
        let winner_pk = "33".repeat(32);

        let frozen_state = ShardState::OpenOrFrozen {
            stage: Stage::SFrozen,
            version: K::VERSION,
            manager_cid: cid.clone(),
            first_buyer_pk: first_buyer_pk.clone(),
            price: K::PRICE_MIN,
            records: vec![TicketRecord {
                cum_tickets: 2,
                buyer_pk: winner_pk.clone(),
            }],
        };
        let frozen_redeem = script_for(&encode_shard_state(&frozen_state).unwrap(), &s_frozen_tail).unwrap();
        let frozen_redeem_hex = hex(&frozen_redeem);
        let frozen_spk = redeem_p2sh(&frozen_redeem).unwrap();

        let mgr_state = ManagerState {
            header: ManagerHeader {
                stage: Stage::MDispatch,
                version: K::VERSION,
                creator_pk: creator_pk.clone(),
                price: K::PRICE_MIN,
                close_daa: 900,
                opened_count: 1,
                cursor: 0,
                total_tickets: 2,
                decision: Decision::Payout,
                reserved: 0,
            },
            entries: vec![],
            winner_pk: Some(winner_pk.clone()),
            shard_cids: vec![scid.clone()],
        };
        let mgr_redeem = script_for(&encode_manager_state(&mgr_state).unwrap(), &m_dispatch_tail).unwrap();
        let mgr_spk = redeem_p2sh(&mgr_redeem).unwrap();

        let payout_state = ShardState::Payout {
            version: K::VERSION,
            first_buyer_pk: first_buyer_pk.clone(),
            winner_pk: winner_pk.clone(),
            vault_amount: 2 * K::PRICE_MIN,
            manager_cid: cid.clone(),
            shard_tickets: 2,
        };
        let payout_redeem = script_for(&encode_shard_state(&payout_state).unwrap(), &s_payout_tail).unwrap();
        let payout_spk = redeem_p2sh(&payout_redeem).unwrap();

        let mut shard_meta = Vec::new();
        shard_meta.extend_from_slice(&unhex_len(&first_buyer_pk, 32).unwrap());
        shard_meta.extend_from_slice(&((2 * K::PRICE_MIN) as u64).to_le_bytes());
        shard_meta.extend_from_slice(&2u32.to_le_bytes());

        let mut mgr_sig = Vec::new();
        push_int(&mut mgr_sig, 1); // k = 1
        push_int(&mut mgr_sig, m_dispatch_tail.len() as i64);
        push_bytes(&mut mgr_sig, &s_payout_tail);
        push_bytes(&mut mgr_sig, &shard_meta);
        push_bytes(&mut mgr_sig, &mgr_redeem);

        let mut shard_frozen_sig = Vec::new();
        push_int(&mut shard_frozen_sig, 1);
        push_bytes(&mut shard_frozen_sig, &unhex_len(&winner_pk, 32).unwrap());
        push_bytes(&mut shard_frozen_sig, &s_payout_tail);
        push_bytes(&mut shard_frozen_sig, &frozen_redeem);

        let round = json!({
            "cid": cid,
            "profileId": K::M_OPEN_HASH,
            "stage": "M_DISPATCH",
            "winnerPk": winner_pk,
            "manager": {
                "outpoint": {"transactionId": "cc".repeat(32), "index": 0},
                "value": "400000000",
                "spk": {"version": mgr_spk.version, "script": mgr_spk.script},
                "utxoDaa": "1000",
                "redeemHex": hex(&mgr_redeem),
            },
            "shards": {
                scid.clone(): {
                    "shardCid": scid,
                    "stage": "S_FROZEN",
                    "outpoint": {"transactionId": "dd".repeat(32), "index": 1},
                    "value": "200000000",
                    "spk": {"version": frozen_spk.version, "script": frozen_spk.script},
                    "utxoDaa": "950",
                    "redeemHex": frozen_redeem_hex,
                    "firstBuyerPk": first_buyer_pk,
                    "tickets": 2,
                    "terminal": false,
                }
            },
            "wallets": {},
        });

        let dispatch_txid = "ee".repeat(32);
        let dispatch_tx = json!({
            "version": 1,
            "lockTime": "0",
            "inputs": [
                {
                    "previousOutpoint": {"transactionId": "cc".repeat(32), "index": 0},
                    "signatureScript": hex(&mgr_sig),
                    "verboseData": {"utxoEntry": {"amount": "400000000"}}
                },
                {
                    "previousOutpoint": {"transactionId": "dd".repeat(32), "index": 1},
                    "signatureScript": hex(&shard_frozen_sig),
                    "verboseData": {"utxoEntry": {"amount": "200000000"}}
                }
            ],
            "outputs": [
                {
                    "value": "399900000",
                    "scriptPublicKey": {"version": 0, "script": "aa20".to_string() + &"00".repeat(32) + "87"}
                },
                {
                    "value": "199900000",
                    "scriptPublicKey": {"version": payout_spk.version, "script": payout_spk.script}
                }
            ],
            "verboseData": {"transactionId": dispatch_txid, "blockHash": "ff".repeat(32)}
        });

        let plugin = KusdtV2::create(&json!({}), Some("mainnet")).unwrap();
        let b1 = BlockRef {
            hash: "ff".repeat(32),
            daa: "1010".into(),
        };
        let after_dispatch = plugin.advance_with_accessor(&round, &dispatch_tx, &b1, None).unwrap();
        assert_eq!(after_dispatch["shards"][&scid]["stage"], "S_PAYOUT");
        assert_eq!(
            after_dispatch["shards"][&scid]["frozenRedeemHex"],
            frozen_redeem_hex
        );

        let mut payout_sig = Vec::new();
        push_int(&mut payout_sig, 2); // action = 2 (PAYOUT_SELF)
        push_bytes(&mut payout_sig, &[]);
        push_bytes(&mut payout_sig, &payout_redeem);

        let payout_txid = "99".repeat(32);
        let payout_tx = json!({
            "version": 1,
            "lockTime": "0",
            "inputs": [{
                "previousOutpoint": {"transactionId": dispatch_txid, "index": 1},
                "signatureScript": hex(&payout_sig),
                "verboseData": {"utxoEntry": {"amount": "199900000"}}
            }],
            "outputs": [{
                "value": "199800000",
                "scriptPublicKey": {"version": 0, "script": "aa20".to_string() + &"00".repeat(32) + "87"}
            }],
            "verboseData": {"transactionId": payout_txid, "blockHash": "88".repeat(32)}
        });
        let b2 = BlockRef {
            hash: "88".repeat(32),
            daa: "1020".into(),
        };
        let after_payout = plugin.advance_with_accessor(&after_dispatch, &payout_tx, &b2, None).unwrap();
        assert_eq!(after_payout["shards"][&scid]["terminal"], true);
        assert_eq!(after_payout["shards"][&scid]["stage"], "S_PAYOUT");
        assert_eq!(
            after_payout["shards"][&scid]["frozenRedeemHex"],
            frozen_redeem_hex
        );
        assert_eq!(after_payout["terminal"], "PAID");
        assert_eq!(after_payout["acceptingDaa"], "1020");
        assert_eq!(after_payout["utxoDaa"], "1020");
        assert_eq!(after_payout["_deltaReceipt"]["action"], "PAYOUT_SELF");
        assert_eq!(after_payout["_deltaReceipt"]["shardCid"], scid);
    }

    #[test]
    fn active_watch_targets_filters_by_stage_and_capacity() {
        let mgr_spk = json!({"version": 0, "script": format!("aa20{}87", "10".repeat(32))});
        let open_lt_spk = json!({"version": 0, "script": format!("aa20{}87", "20".repeat(32))});
        let open_full_spk = json!({"version": 0, "script": format!("aa20{}87", "30".repeat(32))});
        let frozen_spk = json!({"version": 0, "script": format!("aa20{}87", "40".repeat(32))});
        let payout_spk = json!({"version": 0, "script": format!("aa20{}87", "50".repeat(32))});
        let refund_spk = json!({"version": 0, "script": format!("aa20{}87", "60".repeat(32))});
        let term_spk = json!({"version": 0, "script": format!("aa20{}87", "70".repeat(32))});

        let full_records: Vec<Value> = (0..256)
            .map(|i| json!({"cumTickets": i + 1, "buyerPk": "11".repeat(32)}))
            .collect();

        // 1. M_OPEN: Manager + <256 S_OPEN; excludes full (256) S_OPEN
        let m_open = json!({
            "manager": {
                "outpoint": {"transactionId": "01".repeat(32), "index": 0},
                "spk": mgr_spk,
                "utxoDaa": "100",
                "value": "400000000",
            },
            "shards": {
                "s_open_lt": {
                    "stage": "S_OPEN",
                    "terminal": false,
                    "outpoint": {"transactionId": "02".repeat(32), "index": 1},
                    "spk": open_lt_spk,
                    "state": {"records": [{"cumTickets": 1, "buyerPk": "11".repeat(32)}]},
                },
                "s_open_256": {
                    "stage": "S_OPEN",
                    "terminal": false,
                    "outpoint": {"transactionId": "03".repeat(32), "index": 1},
                    "spk": open_full_spk,
                    "state": {"records": full_records},
                },
            }
        });
        let t1 = active_watch_targets(&m_open, "kaspa");
        assert_eq!(t1.len(), 2);
        assert!(t1.iter().any(|t| t.is_manager));
        assert!(t1.iter().any(|t| t.shard_cid.as_deref() == Some("s_open_lt")));
        assert!(!t1.iter().any(|t| t.shard_cid.as_deref() == Some("s_open_256")));

        // 2. M_SEALED: Manager only (0 shards, S_FROZEN excluded)
        let m_sealed = json!({
            "manager": {
                "outpoint": {"transactionId": "04".repeat(32), "index": 0},
                "spk": mgr_spk,
            },
            "shards": {
                "s_frozen_1": {
                    "stage": "S_FROZEN",
                    "terminal": false,
                    "outpoint": {"transactionId": "05".repeat(32), "index": 1},
                    "spk": frozen_spk,
                },
            }
        });
        let t2 = active_watch_targets(&m_sealed, "kaspa");
        assert_eq!(t2.len(), 1);
        assert!(t2[0].is_manager);

        // 3. M_DISPATCH / shard-only: excludes S_FROZEN and terminal shards; includes all live S_PAYOUT/S_REFUND
        let dispatch = json!({
            "manager": null,
            "shards": {
                "s_frozen_2": {
                    "stage": "S_FROZEN",
                    "terminal": false,
                    "outpoint": {"transactionId": "06".repeat(32), "index": 1},
                    "spk": frozen_spk,
                },
                "s_payout_live": {
                    "stage": "S_PAYOUT",
                    "terminal": false,
                    "outpoint": {"transactionId": "07".repeat(32), "index": 1},
                    "spk": payout_spk,
                },
                "s_refund_live": {
                    "stage": "S_REFUND",
                    "terminal": false,
                    "outpoint": {"transactionId": "08".repeat(32), "index": 2},
                    "spk": refund_spk,
                },
                "s_payout_term": {
                    "stage": "S_PAYOUT",
                    "terminal": true,
                    "outpoint": null,
                    "spk": term_spk,
                },
            }
        });
        let t3 = active_watch_targets(&dispatch, "kaspa");
        assert_eq!(t3.len(), 2);
        assert!(t3.iter().any(|t| t.shard_cid.as_deref() == Some("s_payout_live")));
        assert!(t3.iter().any(|t| t.shard_cid.as_deref() == Some("s_refund_live")));
    }
}
