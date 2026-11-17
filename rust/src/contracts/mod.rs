//! Contract plugins: the engine tracks a covenant UTXO lineage generically; a plugin owns the rules of ONE pinned
//! contract version. This build ships native Kaswin V2.0.2 and TN10 Rolling v1 plugins. Old rounds keep the plugin key
//! that created them; a round whose plugin is not loaded is never re-interpreted with another one.
pub mod registry;
pub mod rolling_v1;
pub mod v202;

use crate::core::rolling::Accessor;
use crate::networks::Network;
use crate::normalize::Utxo;
use crate::util::{is_hash, js_string, truthy};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::sync::Arc;
pub use v202::BlockRef;

pub enum ContractRules {
    V202(v202::V202),
    RollingV1(rolling_v1::RollingV1),
}

impl ContractRules {
    pub fn network(&self) -> &'static Network {
        match self {
            ContractRules::V202(r) => r.network,
            ContractRules::RollingV1(r) => r.network,
        }
    }
    pub fn config(&self) -> &Value {
        match self {
            ContractRules::V202(r) => &r.config,
            ContractRules::RollingV1(r) => &r.config,
        }
    }
    pub fn profile_id(&self) -> &str {
        match self {
            ContractRules::V202(r) => r.profile_id(),
            ContractRules::RollingV1(r) => r.profile_id(),
        }
    }
    pub fn watch_addresses(&self) -> Vec<String> {
        match self {
            ContractRules::V202(r) => r.watch_addresses(),
            ContractRules::RollingV1(r) => r.watch_addresses(),
        }
    }
    pub fn discover(&self, u: &Utxo) -> Option<String> {
        match self {
            ContractRules::V202(r) => r.discover(u),
            ContractRules::RollingV1(r) => r.discover(u),
        }
    }
    pub fn candidate_genesis(&self, tx: &Value) -> bool {
        match self {
            ContractRules::V202(r) => r.candidate_genesis(tx),
            ContractRules::RollingV1(r) => r.candidate_genesis(tx),
        }
    }
    pub fn inspect_genesis(&self, tx: &Value) -> (bool, String) {
        match self {
            ContractRules::V202(r) => r.inspect_genesis(tx),
            ContractRules::RollingV1(r) => r.inspect_genesis(tx),
        }
    }
    pub fn genesis(&self, tx: &Value, b: &BlockRef) -> Result<Option<Value>> {
        match self {
            ContractRules::V202(r) => r.genesis(tx, b),
            ContractRules::RollingV1(r) => r.genesis(tx, b),
        }
    }
    pub fn advance(&self, round: &Value, tx: &Value, b: &BlockRef) -> Result<Value> {
        match self {
            ContractRules::V202(r) => r.advance(round, tx, b),
            ContractRules::RollingV1(r) => r.advance(round, tx, b),
        }
    }
    pub fn advance_with_accessor(
        &self,
        round: &Value,
        tx: &Value,
        b: &BlockRef,
        accessor: Option<&Accessor>,
    ) -> Result<Value> {
        match self {
            ContractRules::V202(r) => r.advance(round, tx, b),
            ContractRules::RollingV1(r) => r.advance_with_accessor(round, tx, b, accessor),
        }
    }
    pub fn kind_of(&self, tx: &Value) -> String {
        match self {
            ContractRules::V202(_) => v202::action_of(tx),
            ContractRules::RollingV1(_) => rolling_v1::action_of(tx),
        }
    }
}

pub struct Plugin {
    pub id: String,
    pub version: String,
    pub key: String,
    pub retired: bool,
    pub module: String,
    pub pinned: bool,
    pub rules: ContractRules,
}

impl Plugin {
    pub fn profile_id(&self) -> &str {
        self.rules.profile_id()
    }
    pub fn watch_addresses(&self) -> Vec<String> {
        self.rules.watch_addresses()
    }
    pub fn discover(&self, u: &Utxo) -> Option<String> {
        self.rules.discover(u)
    }
    pub fn candidate_genesis(&self, tx: &Value) -> bool {
        self.rules.candidate_genesis(tx)
    }
    pub fn inspect_genesis(&self, tx: &Value) -> (bool, String) {
        self.rules.inspect_genesis(tx)
    }
    pub fn genesis(&self, tx: &Value, b: &BlockRef) -> Result<Option<Value>> {
        self.rules.genesis(tx, b)
    }
    pub fn advance(&self, round: &Value, tx: &Value, b: &BlockRef) -> Result<Value> {
        self.rules.advance(round, tx, b)
    }
    pub fn advance_with_accessor(
        &self,
        round: &Value,
        tx: &Value,
        b: &BlockRef,
        accessor: Option<&Accessor>,
    ) -> Result<Value> {
        self.rules.advance_with_accessor(round, tx, b, accessor)
    }
    pub fn kind_of(&self, tx: &Value) -> String {
        self.rules.kind_of(tx)
    }
    pub fn is_rolling(&self) -> bool {
        matches!(self.rules, ContractRules::RollingV1(_))
    }
    pub fn describe(&self) -> Value {
        json!({
            "key": self.key, "profileId": self.profile_id(), "retired": self.retired, "module": self.module,
            "moduleSha256": Value::Null, "native": true,
            "network": self.rules.network().key, "networkGenesis": self.rules.network().genesis,
            "watchAddresses": self.watch_addresses(), "config": self.rules.config(),
            "configurable": ["registryAddresses", "registrationSompi", "requiredDeposit", "allowedCreators", "initialStateGuards"],
        })
    }
}

pub struct ContractRegistry {
    plugins: Vec<Arc<Plugin>>,
}

impl ContractRegistry {
    pub fn new(plugins: Vec<Plugin>) -> Result<ContractRegistry> {
        let mut out: Vec<Arc<Plugin>> = vec![];
        for p in plugins {
            if out.iter().any(|x| x.key == p.key) {
                bail!("CONTRACT_DUPLICATE {}", p.key);
            }
            out.push(Arc::new(p));
        }
        Ok(ContractRegistry { plugins: out })
    }
    pub fn get(&self, key: &str) -> Result<Arc<Plugin>> {
        self.plugins
            .iter()
            .find(|p| p.key == key)
            .cloned()
            .ok_or_else(|| anyhow!("CONTRACT_NOT_LOADED {key}"))
    }
    pub fn has(&self, key: &str) -> bool {
        self.plugins.iter().any(|p| p.key == key)
    }
    /// Only plugins still accepting new rounds are asked about Genesis; retired ones still verify their old rounds.
    pub fn for_genesis(&self, only: Option<&str>) -> Vec<Arc<Plugin>> {
        self.plugins
            .iter()
            .filter(|p| match only {
                Some(o) => p.key == o || p.id == o,
                None => !p.retired,
            })
            .cloned()
            .collect()
    }
    pub fn watch_addresses(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for p in self.plugins.iter().filter(|p| !p.retired) {
            for a in p.watch_addresses() {
                if !out.contains(&a) {
                    out.push(a);
                }
            }
        }
        out
    }
    pub fn describe(&self) -> Value {
        Value::Array(self.plugins.iter().map(|p| p.describe()).collect())
    }
}

/// `checkRound`: engine-required round fields.
pub fn check_round(r: Value, key: &str) -> Result<Value> {
    let bad = |m: &str| anyhow!("CONTRACT_ROUND_INVALID {key}: {m}");
    if !r
        .get("id")
        .and_then(|i| i.as_str())
        .map(is_hash)
        .unwrap_or(false)
    {
        return Err(bad("id"));
    }
    let tip = r.get("tip").cloned().unwrap_or(Value::Null);
    if !tip.is_null()
        && !(tip
            .get("transactionId")
            .map(|t| t.is_string())
            .unwrap_or(false)
            && tip
                .get("index")
                .map(|i| i.is_i64() || i.is_u64())
                .unwrap_or(false))
    {
        return Err(bad("tip"));
    }
    let has_tip = truthy(Some(&tip));
    if has_tip && !truthy(r.get("spk")) {
        return Err(bad("spk"));
    }
    if has_tip {
        let v = js_string(r.get("value"));
        if v.is_empty() || !v.bytes().all(|c| c.is_ascii_digit()) {
            return Err(bad("value"));
        }
    }
    if has_tip
        && let Some(c) = r.get("cid").filter(|c| !c.is_null())
        && !c.as_str().map(is_hash).unwrap_or(false)
    {
        return Err(bad("cid"));
    }
    if !has_tip && !truthy(r.get("terminal")) {
        return Err(bad("terminal without reason"));
    }
    Ok(r)
}

/// Network context injected into every plugin created by [`load_contracts`].
pub struct LoadCtx<'a> {
    pub network: &'a str,
    pub registry_addresses: Option<Vec<String>>,
}

/// Built-in default config (used when no --contracts file exists).
pub const DEFAULT_CONTRACTS_JSON: &str = r#"{
  "_comment": "Kaswin V2.0.2 (Profile ed0387c9) + TN10 Rolling v1 (Profile b56bcd9c).",
  "contracts": [
    {
      "module": "kaswin-v202.mjs",
      "retired": false,
      "options": {
        "profileId": "ed0387c9e77bb4e0a8f1f4654a326c316d83663f455283ec6a046a095c94856d",
        "registrationSompi": "20000000",
        "requiredDeposit": "400000000",
        "initialStateGuards": {"phase": "1", "sold": "0", "purchaseCount": "0", "cursor": "0"}
      }
    },
    {
      "module": "rolling-v1",
      "networks": ["tn10"],
      "retired": false,
      "options": {
        "profileId": "b56bcd9cb654610b238a32bae25c41128ebd2837cc7a91b3ea835f8a102fc4e5"
      }
    }
  ]
}"#;

/// Module names that select the native V2.0.2 plugin. `kaswin-v202.mjs` keeps existing JS config files usable.
pub fn is_native_v202(module: &str) -> bool {
    matches!(
        module.trim_start_matches("./"),
        "kaswin-v202.mjs" | "kaswin-v2.0.2" | "native:kaswin-v2.0.2"
    )
}

/// Module names that select the native TN10 Rolling v1 plugin.
pub fn is_native_rolling_v1(module: &str) -> bool {
    matches!(
        module.trim_start_matches("./"),
        "rolling-v1" | "rolling-v1.mjs" | "native:rolling-v1"
    )
}

pub fn load_contracts_json(text: &str, ctx: &LoadCtx) -> Result<ContractRegistry> {
    let cfg: Value = serde_json::from_str(text)?;
    let list = cfg
        .get("contracts")
        .and_then(|c| c.as_array())
        .filter(|a| !a.is_empty())
        .ok_or_else(|| anyhow!("CONTRACTS_CONFIG_EMPTY"))?;
    let mut plugins = vec![];
    for (i, c) in list.iter().enumerate() {
        if let Some(nets) = c.get("networks").and_then(|n| n.as_array()) {
            let matches_net = nets
                .iter()
                .filter_map(|v| v.as_str())
                .any(|n| n == ctx.network);
            if !matches_net {
                continue;
            }
        }
        let module = c.get("module").and_then(|m| m.as_str()).unwrap_or("");
        let mut options = c.get("options").cloned().unwrap_or(json!({}));
        let o = crate::util::obj_mut(&mut options);
        if is_native_v202(module)
            && let Some(r) = &ctx.registry_addresses
        {
            o.insert("registryAddresses".into(), json!(r));
        }
        if is_native_v202(module) {
            let rules = v202::V202::create(&options, Some(ctx.network)).map_err(|e| anyhow!("{e}"))?;
            let version = rules.version();
            plugins.push(Plugin {
                id: v202::PLUGIN_ID.into(),
                key: format!("{}@{}", v202::PLUGIN_ID, version),
                version,
                retired: c.get("retired") == Some(&json!(true)),
                module: module.to_string(),
                pinned: c.get("sha256").map(|s| truthy(Some(s))).unwrap_or(false),
                rules: ContractRules::V202(rules),
            });
        } else if is_native_rolling_v1(module) {
            let rules = rolling_v1::RollingV1::create(&options, Some(ctx.network))
                .map_err(|e| anyhow!("{e}"))?;
            let version = rules.version();
            plugins.push(Plugin {
                id: rolling_v1::PLUGIN_ID.into(),
                key: format!("{}@{}", rolling_v1::PLUGIN_ID, version),
                version,
                retired: c.get("retired") == Some(&json!(true)),
                module: module.to_string(),
                pinned: c.get("sha256").map(|s| truthy(Some(s))).unwrap_or(false),
                rules: ContractRules::RollingV1(rules),
            });
        } else {
            bail!(
                "CONTRACT_MODULE_NOT_SUPPORTED contracts[{i}] {module}: this Rust build only ships native Kaswin V2.0.2 and Rolling v1 plugins (JS plugin modules cannot be loaded)"
            );
        }
    }
    let _ = plugins.iter().map(|p| p.pinned).count();
    ContractRegistry::new(plugins)
}

pub fn load_contracts(path: Option<&str>, ctx: &LoadCtx) -> Result<ContractRegistry> {
    match path {
        Some(p) => load_contracts_json(
            &std::fs::read_to_string(p).map_err(|e| anyhow!("CONTRACTS_CONFIG_READ {p}: {e}"))?,
            ctx,
        ),
        None => load_contracts_json(DEFAULT_CONTRACTS_JSON, ctx),
    }
}
