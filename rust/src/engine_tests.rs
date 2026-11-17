use super::*;

#[tokio::test]
async fn accepted_witness_hydration_failure_does_not_fall_back() {
    let (e,_server,_dir)=crate::checks::engine_fixture(Arc::new(|req|match req["method"].as_str().unwrap_or("") {
        "getVirtualChainFromBlockV2"=>Some(json!({"params":{"addedChainBlockHashes":["aa".repeat(32)],"removedChainBlockHashes":[],"chainBlockAcceptedTransactions":[{"chainBlockHeader":{"hash":"aa".repeat(32),"daaScore":"100","blueScore":"100"},"acceptedTransactions":[{"inputs":[],"outputs":[],"payload":"","verboseData":{"transactionId":"dd".repeat(32),"blockHash":"cc".repeat(32)}}]}]}})),
        "getBlock" if req["params"]["includeTransactions"]==true=>Some(json!({"error":{"message":"CONTAINING_BODY_UNAVAILABLE"}})),
        "getBlock"=>Some(json!({"params":{"block":{"header":{"hash":"aa".repeat(32),"daaScore":"100","blueScore":"100"},"verboseData":{"isChainBlock":true,"selectedParentHash":"bb".repeat(32)}}}})),
        _=>None,
    })).await;
    let acc = Acc {
        hash: "aa".repeat(32),
        daa: 100,
        blue_score: 100,
        is_chain: true,
        selected_parent: Some("bb".repeat(32)),
    };
    let candidate = Candidate {
        txid: "dd".repeat(32),
        tx: json!({"inputs":[],"outputs":[],"payload":""}),
        source: "rest-hint".into(),
        containing: None,
        start: None,
        accepting: None,
        recovered: None,
    };
    let result = e.bind_accepted(candidate, &acc).await;
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("CONTAINING_BODY_UNAVAILABLE")
    );
    assert_eq!(e.store.db().count_rounds(false).unwrap(), 0);
    e.node.close().await;
}

#[tokio::test]
async fn missing_header_and_age_do_not_prove_finality() {
    let (e, _server, _dir) = crate::checks::engine_fixture(Arc::new(|req| {
        if req["method"] == "getBlock" {
            Some(json!({"error":{"message":"cannot find header"}}))
        } else {
            None
        }
    }))
    .await;
    let id = "dd".repeat(32);
    let hash = "aa".repeat(32);
    let key = e.contracts().for_genesis(None)[0].key.clone();
    let next = json!({"id":id,"tip":null,"terminal":"PAID"});
    let evidence = json!({});
    e.store
        .tx(|db| {
            db.put_round(&next, None, now_ms() - 7_200_000, &key)?;
            db.add_transition(&NewTransition {
                txid: &id,
                round_id: &id,
                contract: &key,
                seq: 0,
                kind: "DRAW_AND_PAY",
                status: "ACCEPTED",
                accepting: &hash,
                accepting_daa: "100",
                accepting_blue_score: "100",
                containing: None,
                previous: None,
                next: &next,
                evidence: &evidence,
                observed_at: now_ms() - 7_200_000,
            })
        })
        .unwrap();
    e.check_finality().await.unwrap();
    assert_eq!(
        e.store.db().latest_active(&id).unwrap().unwrap().status,
        "ACCEPTED"
    );
    assert_eq!(e.store.db().pending_count(100).unwrap(), 0);
    e.node.close().await;
}

#[test]
fn rolling_v1_genesis_auto_discovery_without_manual_track() {
    let rules = crate::contracts::rolling_v1::RollingV1::create(&json!({}), Some("tn10")).unwrap();
    let watches = rules.watch_addresses();
    assert!(!watches.is_empty());
    let reg_addr = watches[0].clone();
    let hit = crate::normalize::Utxo {
        outpoint: json!({"transactionId": "11".repeat(32), "index": 0}),
        amount: 100_000_000,
        spk: json!({"version": 0, "script": "aa"}),
        block_daa_score: 624_000_000,
        covenant_id: None,
        address: Some(reg_addr),
    };
    assert_eq!(rules.discover(&hit).as_deref(), Some("11".repeat(32).as_str()));

    let miss = crate::normalize::Utxo {
        address: Some("kaspatest:qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2q8w9j".into()),
        ..hit
    };
    assert!(rules.discover(&miss).is_none());
}

#[test]
fn rolling_v1_export_round_text_with_sharded_preimages() {
    let sh = "78".repeat(32);
    let round = json!({
        "id": "aa".repeat(32),
        "cid": "bb".repeat(32),
        "module": "open",
        "step": 2,
        "buys": 1,
        "bought": 2,
        "value": "200000000",
        "fees": "1000",
        "terminal": null,
        "utxoDaa": "624500000",
        "accepting": "cc".repeat(32),
        "latestTxid": "dd".repeat(32),
        "origin": {"transactionId": "ee".repeat(32), "index": 0},
        "tip": {"transactionId": "dd".repeat(32), "index": 0},
        "spk": {"version": 0, "script": "51"},
        "redeemHex": "5152",
        "settings": {"ticketPrice": "100000000", "ticketCap": 10, "minTickets": 2, "closeEligibleDaa": "624500100"},
        "state": {
            "indices": [{"shard": 0, "start": 0, "end": 1, "root": "12".repeat(32)}],
            "records": [{"key": "34".repeat(32), "start": 1, "end": 2}],
        },
        "preimages": {
            sh.clone(): "abcd"
        },
        "frozen": [{
            "scriptHash": sh,
            "outpoint": {"transactionId": "ff".repeat(32), "index": 1},
            "value": "50000000",
            "utxoDaa": "624500000",
            "spk": {"version": 0, "script": "52"}
        }],
    });
    let (text, historical_shards) =
        crate::contracts::rolling_v1::export_round_text(&round, Some("624500050")).unwrap();
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["cid"], "bb".repeat(32));
    assert_eq!(parsed["profileId"], crate::contracts::rolling_v1::PROFILE_ID);
    assert_eq!(parsed["manager"]["redeemScript"]["$bytes"], "5152");
    assert_eq!(parsed["frozen"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["frozen"][0]["redeemScript"]["$bytes"], "abcd");
    assert!(historical_shards.get("shards").is_some());
}

#[tokio::test]
async fn rolling_v1_rollback_restores_previous_and_final_prunes_bloat() {
    let (e, _server, _dir) = crate::checks::engine_fixture(Arc::new(|_| None)).await;
    let id = "ab".repeat(32);
    let cid = "cd".repeat(32);
    let contract = format!("{}@1.0.0", crate::contracts::rolling_v1::PLUGIN_ID);
    let prev = json!({
        "id": id,
        "cid": cid,
        "step": 0,
        "tip": null,
        "value": "100000000",
        "spk": {"version": 0, "script": "51"}
    });
    let next_bloated = json!({
        "id": id,
        "cid": cid,
        "step": 1,
        "tip": null,
        "value": "200000000",
        "spk": {"version": 0, "script": "51"},
        "receipts": [{"txid": "11".repeat(32)}],
        "history": [{"action": "BUY"}],
        "latestTransaction": {"inputs": [], "outputs": []},
        "genesisTransaction": {"inputs": [], "outputs": []},
        "preimages": [{"shard": 0}],
        "redeemHex": "001122"
    });
    let tx1 = "12".repeat(32);
    let tx2 = "34".repeat(32);
    let acc1 = "56".repeat(32);
    let evidence = json!({});

    // 1. Insert ACCEPTED transition and verify rollback restores `prev`.
    e.store
        .tx(|db| {
            db.put_round(&next_bloated, None, now_ms(), &contract)?;
            db.add_transition(&NewTransition {
                txid: &tx1,
                round_id: &id,
                contract: &contract,
                seq: 0,
                kind: "BUY",
                status: "ACCEPTED",
                accepting: &acc1,
                accepting_daa: "100",
                accepting_blue_score: "100",
                containing: None,
                previous: Some(&prev),
                next: &next_bloated,
                evidence: &evidence,
                observed_at: now_ms(),
            })
        })
        .unwrap();
    let tr0 = e.store.db().transition(&id, 0).unwrap().unwrap();
    e.rollback(&tr0).await.unwrap();
    assert_eq!(e.store.db().transition(&id, 0).unwrap().unwrap().status, "ROLLED_BACK");
    assert_eq!(e.store.db().round(&id).unwrap().unwrap().data["step"], 0);

    // 2. Insert new ACCEPTED transition and finalize -> previous cleared & bloat pruned.
    e.store
        .tx(|db| {
            db.put_round(&next_bloated, None, now_ms(), &contract)?;
            db.add_transition(&NewTransition {
                txid: &tx2,
                round_id: &id,
                contract: &contract,
                seq: 1,
                kind: "BUY",
                status: "ACCEPTED",
                accepting: &acc1,
                accepting_daa: "101",
                accepting_blue_score: "101",
                containing: None,
                previous: Some(&prev),
                next: &next_bloated,
                evidence: &evidence,
                observed_at: now_ms(),
            })
        })
        .unwrap();
    e.store
        .tx(|db| db.set_transition_status(&id, 1, "FINAL"))
        .unwrap();
    let tr1 = e.store.db().transition(&id, 1).unwrap().unwrap();
    assert_eq!(tr1.status, "FINAL");
    assert!(tr1.previous.is_null());
    assert!(tr1.next.get("receipts").is_none());
    assert!(tr1.next.get("history").is_none());
    assert!(tr1.next.get("latestTransaction").is_none());
    assert!(tr1.next.get("preimages").is_none());
    assert_eq!(tr1.next["step"], 1);
    e.node.close().await;
}
