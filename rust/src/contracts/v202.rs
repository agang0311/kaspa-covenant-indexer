//! Kaswin V2.0.2 contract plugin (Profile ed0387c9…): native port of `contracts/kaswin-v202.mjs` +
//! `contracts/v2-rounds.mjs`. The Profile, templates, ABI and CID derivation are network neutral; the only per-network
//! fact is the registry address, injected as network context.
use super::registry::{ScopeEntry, registration_of, registry_scope};
use crate::core::accepted::interpret_accepted;
use crate::core::artifacts::{EMBEDDED, LoadedBundle, load_bundle};
use crate::core::bytes::{hex, unhex};
use crate::core::covenant::{Outpoint, Spk};
use crate::core::genesis::{GENESIS_TAG, GenesisEvidence, verify_genesis_announcement};
use crate::core::state::{self as S, Ledger};
use crate::core::transaction::{Binding, Input, Output, Transaction};
use crate::networks::{Network, network_config, network_of_genesis};
use crate::normalize::{Utxo, jeq, same_spk, wire_spk};
use crate::util::{is_nullish, js_bigint, js_string, truthy};
use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value, json};
use std::sync::{Arc, OnceLock};

pub const PROFILE_ID: &str = S::APPROVED_PROFILE_ID;
pub const REGISTRATION_SOMPI: &str = "20000000";
pub const PLUGIN_ID: &str = "kaswin-v2.0.2";

static BUNDLE: OnceLock<Arc<LoadedBundle>> = OnceLock::new();

pub fn bundle() -> Result<Arc<LoadedBundle>> {
    if let Some(b) = BUNDLE.get() {
        return Ok(b.clone());
    }
    let b = Arc::new(load_bundle(&EMBEDDED)?);
    Ok(BUNDLE.get_or_init(|| b).clone())
}

/// `actionOf(tx)`: label only (first opcode of input 0's signature script), never used for filtering.
pub fn action_of(tx: &Value) -> String {
    let script = tx
        .get("inputs")
        .and_then(|i| i.get(0))
        .and_then(|i| i.get("signatureScript"))
        .and_then(|s| s.as_str());
    let Some(s) = script else {
        return "UNKNOWN".into();
    };
    if s.len() < 2 || !s[..2].bytes().all(|c| c.is_ascii_hexdigit()) {
        return "UNKNOWN".into();
    }
    // Same table as the JS object literal (later duplicate keys win: 84 -> DRAW_AND_PAY).
    match u8::from_str_radix(&s[..2], 16).unwrap() {
        0x51 => "BUY",
        0x52 => "CLOSE",
        0x53 | 0x54 => "DRAW_AND_PAY",
        0x55 | 0x5a => "REFUND",
        0x59 => "TIMEOUT_REFUND",
        _ => "UNKNOWN",
    }
    .into()
}

pub struct V202 {
    bundle: Arc<LoadedBundle>,
    pub network: &'static Network,
    scope: Vec<ScopeEntry>,
    watch: Vec<String>,
    prefix: String,
    required_deposit: Option<Value>,
    initial_state_guards: Option<Map<String, Value>>,
    pub config: Value,
}

fn outpoint_of(v: Option<&Value>) -> Result<Outpoint> {
    let Some(v) = v.filter(|v| v.is_object()) else {
        bail!("Cannot read properties of undefined (reading 'transactionId')")
    };
    let transaction_id = match v.get("transactionId") {
        Some(Value::String(s)) => s.clone(),
        _ => "?".into(), // fails the canonical-hex check exactly where the TS core would
    };
    let index = v
        .get("index")
        .and_then(|i| {
            i.as_i64()
                .map(|x| x as i128)
                .or_else(|| i.as_u64().map(|x| x as i128))
        })
        .unwrap_or(i128::MIN);
    Ok(Outpoint {
        transaction_id,
        index,
    })
}

fn spk_struct(v: &Value) -> Spk {
    Spk {
        version: v
            .get("version")
            .and_then(|x| x.as_i64())
            .map(|x| x as i128)
            .unwrap_or(-1),
        script: v
            .get("script")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

fn spk_value(s: &Spk) -> Value {
    json!({"version": s.version as i64, "script": s.script})
}

fn ledger_bytes(v: Option<&Value>) -> Result<Vec<u8>> {
    let s = v.and_then(|x| x.as_str()).unwrap_or("");
    if s.len() % 2 != 0
        || !s.bytes().all(|c| c.is_ascii_hexdigit())
        || v.map(|x| !x.is_string()).unwrap_or(true)
    {
        bail!("V2_HEX");
    }
    Ok(hex::decode(s)?)
}

fn state_json(s: &Ledger) -> Value {
    json!({
        "phase": s.phase as i64,
        "config": config_json(&s.config),
        "sold": s.sold as i64,
        "purchaseCount": s.purchase_count as i64,
        "cursor": s.cursor as i64,
        "ownerKey": s.owner_key,
    })
}

fn config_json(c: &S::Config) -> Value {
    json!({
        "ticketPrice": c.ticket_price.to_string(),
        "ticketCap": c.ticket_cap as i64,
        "purchaseCap": c.purchase_cap as i64,
        "minTickets": c.min_tickets as i64,
        "closeEligibleDaa": c.close_eligible_daa.to_string(),
    })
}

/// Block reference handed to plugins: accepting chain block hash and its DAA score as a decimal string.
pub struct BlockRef {
    pub hash: String,
    pub daa: String,
}

impl V202 {
    pub fn create(options: &Value, network: Option<&str>) -> Result<V202> {
        let profile_id = options
            .get("profileId")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !crate::util::is_hash(profile_id) || profile_id == "00".repeat(32) {
            bail!("V202_PROFILE_PIN_REQUIRED");
        }
        let net = match network {
            Some(n) => network_config(n)?,
            None => match options.get("networkGenesis").and_then(|g| g.as_str()) {
                Some(g) => network_of_genesis(g)
                    .map(Ok)
                    .unwrap_or_else(|| network_config(g))?,
                None => network_config("tn10")?,
            },
        };
        let registry: Vec<String> = match options.get("registryAddresses") {
            None | Some(Value::Null) => vec![net.registry.to_string()],
            Some(Value::Array(a)) => a.iter().map(|x| js_string(Some(x))).collect(),
            Some(_) => bail!("REGISTRY_ADDRESSES"),
        };
        let registration = options
            .get("registrationSompi")
            .filter(|v| !v.is_null())
            .map(|v| js_string(Some(v)))
            .unwrap_or_else(|| REGISTRATION_SOMPI.into());
        if registration != REGISTRATION_SOMPI {
            bail!("V202_REGISTRATION_AMOUNT_PIN");
        }
        let bundle = bundle()?;
        if bundle.profile.id() != profile_id
            || bundle.pins["contractVersion"] != json!(S::CONTRACT_VERSION)
        {
            bail!("V202_UNPINNED_PROFILE");
        }
        let scope = if registry.is_empty() {
            vec![]
        } else {
            registry_scope(&registry, net.key)?
        };
        let mut watch: Vec<String> = vec![];
        for s in &scope {
            if !watch.contains(&s.address) {
                watch.push(s.address.clone());
            }
        }
        let prefix = format!("{}{}", hex(GENESIS_TAG.as_bytes()), bundle.profile.id());
        let required_deposit = options
            .get("requiredDeposit")
            .filter(|v| truthy(Some(v)))
            .cloned();
        let initial_state_guards = options
            .get("initialStateGuards")
            .and_then(|g| g.as_object())
            .cloned();
        let config = json!({
            "network": net.key,
            "networkGenesis": net.genesis,
            "registryAddresses": watch,
            "registrationSompi": registration,
            "requiredDeposit": required_deposit.as_ref().map(|v| js_string(Some(v))).unwrap_or_else(|| S::DEPOSIT.to_string()),
            "initialStateGuards": initial_state_guards.clone().map(Value::Object).unwrap_or(Value::Null),
            "allowedCreators": options.get("allowedCreators").filter(|v| truthy(Some(v))).cloned().unwrap_or(json!([])),
            "profileId": bundle.profile.id(),
            "contractVersion": S::CONTRACT_VERSION,
            "coreSha256": Value::Null,
            "implementation": "rust-native",
            "bundleSha256": bundle.bundle_digest,
            "repoDir": Value::Null,
        });
        Ok(V202 {
            bundle,
            network: net,
            scope,
            watch,
            prefix,
            required_deposit,
            initial_state_guards,
            config,
        })
    }

    pub fn profile_id(&self) -> &str {
        self.bundle.profile.id()
    }
    pub fn version(&self) -> String {
        self.bundle.profile.id()[..16].to_string()
    }
    pub fn watch_addresses(&self) -> Vec<String> {
        self.watch.clone()
    }

    pub fn discover(&self, u: &Utxo) -> Option<String> {
        let addr_ok = u
            .address
            .as_ref()
            .map(|a| self.watch.contains(a))
            .unwrap_or(false);
        let cid_ok = u
            .covenant_id
            .as_ref()
            .map(|c| c.is_empty() || c == S::ZERO)
            .unwrap_or(true);
        if addr_ok && u.amount == 20_000_000 && u.outpoint_index() == Some(1) && cid_ok {
            Some(u.outpoint_txid().to_string())
        } else {
            None
        }
    }

    pub fn candidate_genesis(&self, tx: &Value) -> bool {
        tx.get("payload")
            .and_then(|p| p.as_str())
            .map(|p| p.starts_with(&self.prefix))
            .unwrap_or(false)
    }

    fn evidence_ledger(&self, tx: &Value, payload: &str) -> Result<Ledger> {
        let output = &tx["outputs"][0];
        let binding = &output["covenant"];
        let cid = binding
            .get("covenantId")
            .and_then(|c| c.as_str())
            .map(|c| c.to_string());
        let indices: Vec<usize> = tx
            .get("outputs")
            .and_then(|o| o.as_array())
            .map(|a| {
                a.iter()
                    .enumerate()
                    .filter(|(_, v)| {
                        jeq(
                            v.get("covenant").and_then(|c| c.get("covenantId")),
                            binding.get("covenantId"),
                        )
                    })
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default();
        let origin = outpoint_of(
            tx.get("inputs")
                .and_then(|i| i.get(0))
                .and_then(|i| i.get("previousOutpoint")),
        )?;
        let value = js_bigint(output.get("value"))?;
        let spk = spk_struct(&wire_spk(
            output.get("scriptPublicKey").unwrap_or(&Value::Null),
        )?);
        let auth = binding
            .get("authorizingInput")
            .and_then(|a| a.as_i64())
            .map(|a| a as i128)
            .unwrap_or(i128::MIN);
        verify_genesis_announcement(
            &GenesisEvidence {
                accepted: true,
                payload,
                authorizing_outpoint: &origin,
                output_index: 0,
                value,
                spk: &spk,
                covenant_id: cid.as_deref().unwrap_or("?"),
                authorizing_input: auth,
                covenant_output_indices: indices,
            },
            &self.bundle.profile,
        )
    }

    /// Routing/template check only; the engine establishes acceptance and hydrates funding before genesis().
    pub fn inspect_genesis(&self, tx: &Value) -> (bool, String) {
        let payload = match tx.get("payload") {
            Some(Value::String(p))
                if p.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) =>
            {
                p.as_str()
            }
            _ => return (false, "GENESIS_PAYLOAD_FORMAT".into()),
        };
        if payload.len() != self.prefix.len() + S::HEADER_BYTES * 2
            || !payload.starts_with(&self.prefix)
        {
            return (false, "V202_TAG_PROFILE_OR_LENGTH".into());
        }
        let binding = tx
            .get("outputs")
            .and_then(|o| o.get(0))
            .and_then(|o| o.get("covenant"));
        if !truthy(binding)
            || !jeq(
                binding.and_then(|b| b.get("authorizingInput")),
                Some(&json!(0)),
            )
        {
            return (false, "GENESIS_BINDING".into());
        }
        let r = (|| -> Result<Option<String>> {
            let s = self.evidence_ledger(tx, payload)?;
            let output = &tx["outputs"][0];
            if let Some(dep) = &self.required_deposit
                && js_bigint(output.get("value"))? != js_bigint(Some(dep))?
            {
                return Ok(Some("DEPOSIT_GUARD_MISMATCH".into()));
            }
            if let Some(guards) = &self.initial_state_guards {
                for (k, v) in guards {
                    if v.is_null() || v.as_str() == Some("") {
                        continue;
                    }
                    let actual: Option<String> = match k.as_str() {
                        "phase" => Some(s.phase.to_string()),
                        "sold" => Some(s.sold.to_string()),
                        "purchaseCount" => Some(s.purchase_count.to_string()),
                        "cursor" => Some(s.cursor.to_string()),
                        "ownerKey" => Some(s.owner_key.clone()),
                        "config" => Some("[object Object]".into()),
                        "directory" => Some(
                            s.directory
                                .iter()
                                .map(|b| b.to_string())
                                .collect::<Vec<_>>()
                                .join(","),
                        ),
                        "ticketPrice" => Some(s.config.ticket_price.to_string()),
                        "ticketCap" => Some(s.config.ticket_cap.to_string()),
                        "purchaseCap" => Some(s.config.purchase_cap.to_string()),
                        "minTickets" => Some(s.config.min_tickets.to_string()),
                        "closeEligibleDaa" => Some(s.config.close_eligible_daa.to_string()),
                        _ => None,
                    };
                    if let Some(a) = actual
                        && a != js_string(Some(v))
                    {
                        return Ok(Some(format!("GUARD_{k}_MISMATCH")));
                    }
                }
            }
            Ok(None)
        })();
        match r {
            Ok(None) => (true, String::new()),
            Ok(Some(reason)) => (false, reason),
            Err(e) => (false, e.to_string()),
        }
    }

    fn summary(&self, r: Map<String, Value>) -> Result<Value> {
        let s = S::decode_ledger(&ledger_bytes(r.get("ledger"))?)?;
        let records = S::records(&s)?;
        let mut out = r;
        out.insert("state".into(), state_json(&s));
        out.insert(
            "purchases".into(),
            Value::Array(
                records
                    .iter()
                    .map(|x| json!({"end": x.end as i64, "key": x.key, "count": x.count as i64}))
                    .collect(),
            ),
        );
        out.insert("profileId".into(), json!(self.profile_id()));
        out.insert("requiresIndependentVerification".into(), json!(true));
        Ok(Value::Object(out))
    }

    /// null = not this contract; Err = unknown (retry), never silently drop.
    pub fn genesis(&self, t: &Value, b: &BlockRef) -> Result<Option<Value>> {
        let Some(payload) = t
            .get("payload")
            .and_then(|p| p.as_str())
            .filter(|p| p.starts_with(&self.prefix))
        else {
            return Ok(None);
        };
        let o = t.get("outputs").and_then(|o| o.get(0));
        let c = o.and_then(|o| o.get("covenant"));
        if !truthy(c) {
            return Ok(None);
        }
        let (o, c) = (o.unwrap(), c.unwrap());
        let Ok(s) = self.evidence_ledger(t, payload) else {
            return Ok(None);
        };
        let id = t["verboseData"]["transactionId"].clone();
        let candidate = registration_of(t, &self.scope, &s.owner_key, REGISTRATION_SOMPI)?;
        let mut registration = Value::Null;
        if let Some(cand) = candidate {
            let inputs = t["inputs"].as_array().cloned().unwrap_or_default();
            if !(1..=8).contains(&inputs.len()) {
                bail!("V2_GENESIS_FUNDING_COUNT");
            }
            for i in &inputs {
                let u = i.get("verboseData").and_then(|v| v.get("utxoEntry"));
                let ok = truthy(u)
                    && !is_nullish(u.and_then(|u| u.get("amount")))
                    && !is_nullish(u.and_then(|u| u.get("scriptPublicKey")))
                    && u.and_then(|u| u.get("isCoinbase"))
                        .map(|b| b.is_boolean())
                        .unwrap_or(false)
                    && i.get("signatureScript")
                        .map(|s| s.is_string())
                        .unwrap_or(false);
                if !ok {
                    bail!("V2_FUNDING_CONTEXT_MISSING");
                }
            }
            let owner_spk = json!({"version": 0, "script": format!("20{}ac", s.owner_key)});
            let sig_re = |sig: &str| {
                sig.len() == 132
                    && sig[..2].eq_ignore_ascii_case("41")
                    && sig[130..].eq_ignore_ascii_case("01")
                    && sig.bytes().all(|c| c.is_ascii_hexdigit())
            };
            let mut own_funding = true;
            for i in &inputs {
                let u = &i["verboseData"]["utxoEntry"];
                let cid_ok = !truthy(u.get("covenantId"))
                    || u.get("covenantId").and_then(|c| c.as_str()) == Some(S::ZERO);
                let ok = sig_re(i["signatureScript"].as_str().unwrap_or(""))
                    && u.get("isCoinbase") == Some(&json!(false))
                    && cid_ok
                    && js_bigint(u.get("amount"))? > 0
                    && same_spk(&wire_spk(&u["scriptPublicKey"])?, &owner_spk);
                if !ok {
                    own_funding = false;
                    break;
                }
            }
            if own_funding {
                let outs = t["outputs"].as_array().cloned().unwrap_or_default();
                let only_first_cov = outs
                    .iter()
                    .enumerate()
                    .all(|(i, v)| i == 0 || !truthy(v.get("covenant")));
                let mut out_sum: i128 = 0;
                for v in &outs {
                    out_sum += js_bigint(v.get("value"))?;
                }
                let mut in_sum: i128 = 0;
                for i in &inputs {
                    in_sum += js_bigint(i["verboseData"]["utxoEntry"].get("amount"))?;
                }
                if only_first_cov && out_sum < in_sum {
                    registration = cand;
                }
            }
        }
        let origin = t["inputs"][0]["previousOutpoint"].clone();
        // `undefined` fields are dropped by JSON.stringify in the JS build: only write keys that exist.
        let block_hash = t["verboseData"].get("blockHash").cloned();
        let tx_json = t.clone();
        let mut r = Map::new();
        r.insert("id".into(), id.clone());
        r.insert("genesisTxid".into(), id.clone());
        if !registration.is_null() {
            let mut creation = Map::new();
            creation.insert("schema".into(), json!("KASWIN_V2"));
            creation.insert("genesisTxid".into(), id.clone());
            creation.insert("acceptingBlock".into(), json!(b.hash));
            creation.insert("acceptingDaa".into(), json!(b.daa));
            if let Some(h) = &block_hash {
                creation.insert("containingBlock".into(), h.clone());
            }
            creation.insert("profileId".into(), json!(self.profile_id()));
            creation.insert("networkGenesis".into(), json!(self.network.genesis));
            creation.insert("cid".into(), c["covenantId"].clone());
            creation.insert("origin".into(), origin.clone());
            creation.insert("config".into(), config_json(&s.config));
            creation.insert("registration".into(), registration);
            creation.insert("genesisTransaction".into(), tx_json.clone());
            creation.insert("verifiedV2Genesis".into(), json!(true));
            r.insert("creation".into(), Value::Object(creation));
        }
        r.insert("genesisAccepting".into(), json!(b.hash));
        if let Some(h) = &block_hash {
            r.insert("genesisContaining".into(), h.clone());
        }
        r.insert("genesisTransaction".into(), tx_json.clone());
        r.insert("latestTxid".into(), id.clone());
        r.insert("latestTransaction".into(), tx_json);
        r.insert("accepting".into(), json!(b.hash));
        if let Some(h) = block_hash {
            r.insert("containing".into(), h);
        }
        r.insert("tip".into(), json!({"transactionId": id, "index": 0}));
        r.insert("origin".into(), origin);
        r.insert("cid".into(), c["covenantId"].clone());
        r.insert("ledger".into(), json!(hex(&S::encode_ledger(&s)?)));
        r.insert("spk".into(), wire_spk(&o["scriptPublicKey"])?);
        r.insert("value".into(), json!(js_string(o.get("value"))));
        r.insert("utxoDaa".into(), json!(b.daa));
        r.insert("terminal".into(), Value::Null);
        Ok(Some(self.summary(r)?))
    }

    fn snapshot(&self, r: &Value, daa: &str) -> Result<S::Snapshot> {
        Ok(S::Snapshot {
            ledger: ledger_bytes(r.get("ledger"))?,
            tip: outpoint_of(r.get("tip"))?,
            script_public_key: spk_struct(r.get("spk").unwrap_or(&Value::Null)),
            covenant_id: r
                .get("cid")
                .and_then(|c| c.as_str())
                .unwrap_or("?")
                .to_string(),
            value: js_bigint(r.get("value"))?,
            utxo_daa: js_bigint(r.get("utxoDaa"))?,
            current_daa: js_bigint(Some(&json!(daa)))?,
            origin: match r.get("origin") {
                Some(o) if truthy(Some(o)) => Some(outpoint_of(Some(o))?),
                _ => None,
            },
        })
    }

    /// Err = the accepted spend does not follow these rules (the engine marks the round STALE).
    pub fn advance(&self, r: &Value, t: &Value, b: &BlockRef) -> Result<Value> {
        let input0 = t.get("inputs").and_then(|i| i.get(0));
        let prev = input0.and_then(|i| i.get("previousOutpoint"));
        let tip = r.get("tip");
        let same_outpoint = jeq(
            prev.and_then(|p| p.get("transactionId")),
            tip.and_then(|p| p.get("transactionId")),
        ) && jeq(
            prev.and_then(|p| p.get("index")),
            tip.and_then(|p| p.get("index")),
        );
        if !(r.get("profileId").and_then(|p| p.as_str()) == Some(self.profile_id())
            && truthy(tip)
            && same_outpoint)
        {
            bail!("V2_ROUND_INPUT");
        }
        let x = self.snapshot(r, &b.daa)?;
        let known = input0
            .and_then(|i| i.get("verboseData"))
            .and_then(|v| v.get("utxoEntry"));
        if let Some(k) = known.filter(|k| truthy(Some(k))) {
            let ok = js_bigint(k.get("amount"))? == x.value
                && same_spk(
                    &wire_spk(k.get("scriptPublicKey").unwrap_or(&Value::Null))?,
                    r.get("spk").unwrap_or(&Value::Null),
                )
                && jeq(k.get("covenantId"), r.get("cid"))
                && (is_nullish(k.get("blockDaaScore"))
                    || js_bigint(k.get("blockDaaScore"))? <= x.utxo_daa);
            if !ok {
                bail!("V2_SPENT_CONTEXT_MISMATCH");
            }
        }
        let inputs = t
            .get("inputs")
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default();
        if inputs.iter().skip(1).any(|i| {
            jeq(
                i.get("verboseData")
                    .and_then(|v| v.get("utxoEntry"))
                    .and_then(|u| u.get("covenantId")),
                r.get("cid"),
            )
        }) {
            bail!("V2_MULTIPLE_FAMILY_INPUTS");
        }
        let int_of = |v: Option<&Value>| {
            v.and_then(|x| {
                x.as_i64()
                    .map(|n| n as i128)
                    .or_else(|| x.as_u64().map(|n| n as i128))
            })
            .unwrap_or(i128::MIN)
        };
        let mut tx_inputs = Vec::with_capacity(inputs.len());
        for i in &inputs {
            let sig = match i.get("signatureScript") {
                Some(Value::String(s)) => s.to_lowercase(),
                _ => bail!("i.signatureScript.toLowerCase is not a function"),
            };
            tx_inputs.push(Input {
                previous_outpoint: outpoint_of(i.get("previousOutpoint"))?,
                signature_script: sig,
                sequence: js_bigint(i.get("sequence"))?,
                compute_budget: if is_nullish(i.get("computeBudget")) {
                    0
                } else {
                    int_of(i.get("computeBudget"))
                },
            });
        }
        let mut tx_outputs = vec![];
        for o in t
            .get("outputs")
            .and_then(|o| o.as_array())
            .cloned()
            .unwrap_or_default()
        {
            let value = js_bigint(o.get("value"))?;
            let spk = spk_struct(&wire_spk(o.get("scriptPublicKey").unwrap_or(&Value::Null))?);
            let covenant = match o.get("covenant") {
                Some(c) if !c.is_null() => Some(Binding {
                    covenant_id: c
                        .get("covenantId")
                        .and_then(|x| x.as_str())
                        .unwrap_or("?")
                        .to_string(),
                    authorizing_input: int_of(c.get("authorizingInput")),
                }),
                _ => None,
            };
            tx_outputs.push(Output {
                value,
                script_public_key: spk,
                covenant,
            });
        }
        let tx = Transaction {
            version: int_of(t.get("version")),
            inputs: tx_inputs,
            outputs: tx_outputs,
            lock_time: js_bigint(t.get("lockTime"))?,
            subnetwork_id: t
                .get("subnetworkId")
                .and_then(|s| s.as_str())
                .unwrap_or("?")
                .to_string(),
            gas: js_bigint(t.get("gas"))?,
            payload: match t.get("payload") {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(s)) => s.clone(),
                Some(_) => "?".into(),
            },
            storage_mass: if is_nullish(t.get("storageMass")) {
                0
            } else {
                js_bigint(t.get("storageMass"))?
            },
        };
        let mut amounts: Vec<Option<i128>> = vec![];
        for (n, i) in inputs.iter().enumerate() {
            amounts.push(if n == 0 {
                Some(x.value)
            } else {
                match i
                    .get("verboseData")
                    .and_then(|v| v.get("utxoEntry"))
                    .and_then(|u| u.get("amount"))
                {
                    None | Some(Value::Null) => None,
                    a => Some(js_bigint(a)?),
                }
            });
        }
        let all: Option<Vec<i128>> = amounts.into_iter().collect();
        let result = interpret_accepted(&x, &self.bundle.profile, &tx, all.as_deref())?;
        let id = t["verboseData"]["transactionId"].clone();
        let mut next = r.as_object().cloned().unwrap_or_default();
        next.insert("latestTxid".into(), id.clone());
        next.insert("latestTransaction".into(), t.clone());
        next.insert("accepting".into(), json!(b.hash));
        match t["verboseData"].get("blockHash") {
            Some(h) => {
                next.insert("containing".into(), h.clone());
            }
            None => {
                next.shift_remove("containing");
            }
        }
        next.insert(
            "terminal".into(),
            result.terminal.map(|x| json!(x)).unwrap_or(Value::Null),
        );
        let has_next = result.next.is_some();
        next.insert(
            "tip".into(),
            if has_next {
                json!({"transactionId": id, "index": 0})
            } else {
                Value::Null
            },
        );
        if let Some(n) = &result.next {
            next.insert("ledger".into(), json!(hex(&S::encode_ledger(n)?)));
            next.insert("spk".into(), spk_value(&tx.outputs[0].script_public_key));
            next.insert("value".into(), json!(tx.outputs[0].value.to_string()));
        } else {
            next.insert("value".into(), json!("0"));
        }
        next.insert("utxoDaa".into(), json!(b.daa));
        next.insert(
            "actualFee".into(),
            result
                .fee
                .map(|f| json!(f.to_string()))
                .unwrap_or(Value::Null),
        );
        if let Some(a) = result.timeout_anchor_daa {
            next.insert("timeoutAnchorDaa".into(), json!(a));
        }
        let _ = result.action;
        let next_v = Value::Object(next);
        if has_next {
            S::verify_snapshot(&self.snapshot(&next_v, &b.daa)?, &self.bundle.profile)?;
        }
        self.summary(next_v.as_object().cloned().unwrap())
    }
}

#[allow(dead_code)]
pub fn ensure_hex(s: &str) -> Result<Vec<u8>> {
    unhex(s).map_err(|e| anyhow!(e))
}
