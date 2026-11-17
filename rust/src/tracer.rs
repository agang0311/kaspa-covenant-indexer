//! TXID tracing uses REST only to locate a lineage; the engine revalidates acceptance and rules on the node.
use crate::{
    engine::Engine,
    normalize::ZERO_HASH,
    util::{is_hash, now_ms},
};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub async fn track(engine: &Arc<Engine>, txid: &str) -> Result<Value> {
    let id = txid.trim().to_lowercase();
    if !is_hash(&id) {
        bail!("INVALID_TXID: 请输入 64 位十六进制交易哈希");
    }
    let mut cur = id.clone();
    let mut genesis = None;
    let mut cid = None;
    let mut hops = 0;
    let http = reqwest::Client::new();
    for hop in 0..64 {
        let round = engine
            .store
            .db()
            .round(&cur)?
            .filter(|r| r.status != "ROLLED_BACK");
        let tr = engine.store.db().any_active_transition_by_txid(&cur)?;
        let round = match (round, tr) {
            (Some(r), _) => Some(r),
            (_, Some(t)) => engine.store.db().round(&t.round_id)?,
            _ => None,
        };
        if let Some(r) = round {
            genesis = Some(r.id.clone());
            cid = r.data["cid"].as_str().map(str::to_owned);
            break;
        }
        let Some(base) = &engine.config.rest_url else {
            bail!("REST_DISABLED: 未索引交易的回溯需要本网络 REST 提示源");
        };
        let response = http
            .get(format!("{}/transactions/{cur}", base.trim_end_matches('/')))
            .timeout(Duration::from_secs(4))
            .send()
            .await;
        let raw: Value = match response {
            Ok(r) if r.status().is_success() => r.json().await?,
            Ok(r) if hop == 0 => bail!("链上未查到该交易 (HTTP {})", r.status()),
            Err(e) if hop == 0 => return Err(e.into()),
            _ => break,
        };
        hops += 1;
        if let Some(outputs) = raw["outputs"].as_array() {
            for o in outputs {
                if let Some(c) = o["covenant_id"]
                    .as_str()
                    .or_else(|| o["covenant"]["covenantId"].as_str())
                {
                    cid = Some(c.to_owned());
                    break;
                }
            }
        }
        let payload = raw["payload"].as_str().unwrap_or("");
        if payload.starts_with("4b415357494e5f47454e45534953")
            || payload.starts_with("4b415357494e5f524f4c4c494e475f47454e455349535f5631")
            || payload.contains("4b573231")
        {
            genesis = Some(cur);
            break;
        }
        let prev = raw["inputs"][0]["previous_outpoint_hash"]
            .as_str()
            .or_else(|| raw["inputs"][0]["previousOutpoint"]["transactionId"].as_str());
        match prev {
            Some(p) if is_hash(p) && p != ZERO_HASH => cur = p.to_owned(),
            _ if cid.is_some() => {
                genesis = Some(cur);
                break;
            }
            _ => bail!("NOT_COVENANT_TX: 无可识别契约谱系"),
        }
    }
    let Some(genesis) = genesis else {
        bail!("GENESIS_NOT_RESOLVED");
    };
    let existing = match &cid {
        Some(c) => engine.store.db().by_cid(c)?.into_iter().next(),
        None => engine.store.db().round(&genesis)?,
    };
    if let Some(r) = existing {
        let known = engine.store.db().active_transition(&r.id, &id)?.is_some() || r.id == id;
        if r.status == "STALE" || !known {
            // Serialize mutation with state transitions; never update a round from an HTTP task.
            let rid = r.id.clone();
            engine.run(format!("track:{rid}"), move |e| async move {
                let current = e.store.db().round(&rid)?;
                if current.as_ref().is_some_and(|r| r.status == "STALE") {
                    e.store.db().set_round_status(&rid, "LIVE", now_ms())?;
                }
                e.resolve_spend(&rid, None).await.map(|_| ())
            });
            engine.idle().await;
        }
        let updated = engine.store.db().round(&r.id)?.unwrap_or(r);
        let advanced = !known
            && engine
                .store
                .db()
                .active_transition(&updated.id, &id)?
                .is_some();
        return Ok(
            json!({"status":if advanced {"ADVANCED"} else {"ALREADY_INDEXED"},"action":if advanced {"STATE_MACHINE_ADVANCED"} else {"DEDUPLICATED_BY_CID"},
            "message":if advanced {"已核验并推进轮次状态"} else {"已定位到索引中的轮次"},"queriedTxid":id,"genesisTxid":genesis,"roundId":updated.id,
            "cid":updated.data["cid"],"contract":updated.contract,"phase":updated.data["state"]["phase"],"terminal":updated.data["terminal"],
            "indexStatus":updated.status,"transitionsCount":engine.store.db().transition_count(&updated.id)?,"traceHops":hops}),
        );
    }
    engine.run_genesis(&genesis, json!({"eventAt":now_ms()}), true);
    Ok(
        json!({"status":"TRACKED_NEW","action":"CATCHUP_TRIGGERED","message":"已提交创世提示，后台将通过节点核验接受状态和合约规则", "queriedTxid":id,"genesisTxid":genesis,"cid":cid,"traceHops":hops}),
    )
}
