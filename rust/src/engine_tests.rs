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
    // Live rounds (terminal: null) omit historicalShards because frozen preimages are embedded in roundText.
    assert!(historical_shards.is_null());

    // Settled non-EMPTY rounds include historicalShards.
    let mut settled = round.clone();
    settled["terminal"] = json!("PAID");
    let (_settled_text, settled_shards) =
        crate::contracts::rolling_v1::export_round_text(&settled, Some("624500050")).unwrap();
    assert_eq!(settled_shards["shards"].as_array().unwrap().len(), 1);
    assert_eq!(settled_shards["shards"][0]["redeemHex"], "abcd");
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

#[tokio::test]
async fn kusdt_v2_multi_utxo_subscription_diff_unsubscribes_full_or_frozen_shards() {
    use crate::address::spk_to_address;
    use crate::contracts::kusdt_v2 as KV2;

    let mgr_spk = json!({"version": 0, "script": format!("aa20{}87", "10".repeat(32))});
    let shard_a_spk = json!({"version": 0, "script": format!("aa20{}87", "20".repeat(32))});
    let shard_b_spk = json!({"version": 0, "script": format!("aa20{}87", "30".repeat(32))});
    let shard_b_frozen_spk = json!({"version": 0, "script": format!("aa20{}87", "40".repeat(32))});

    let mgr_addr = spk_to_address(0, mgr_spk["script"].as_str().unwrap(), "kaspa").unwrap();
    let shard_a_addr = spk_to_address(0, shard_a_spk["script"].as_str().unwrap(), "kaspa").unwrap();
    let shard_b_addr = spk_to_address(0, shard_b_spk["script"].as_str().unwrap(), "kaspa").unwrap();
    let shard_b_frozen_addr =
        spk_to_address(0, shard_b_frozen_spk["script"].as_str().unwrap(), "kaspa").unwrap();

    let (e, _server, _dir) = crate::checks::engine_fixture(Arc::new(|req| {
        match req["method"].as_str().unwrap_or("") {
            "getUtxosByAddresses" => Some(json!({"params": {"entries": []}})),
            "getBlockDagInfo" => Some(json!({"params": {"virtualDaaScore": "1000"}})),
            _ => None,
        }
    }))
    .await;

    let id = "aa".repeat(32);
    let initial = json!({
        "id": id,
        "cid": "90".repeat(32),
        "tip": {"transactionId": "01".repeat(32), "index": 0},
        "spk": mgr_spk,
        "value": "400000000",
        "utxoDaa": "1000",
        "accepting": "bb".repeat(32),
        "manager": {
            "outpoint": {"transactionId": "01".repeat(32), "index": 0},
            "spk": mgr_spk,
            "utxoDaa": "1000",
            "value": "400000000",
        },
        "shards": {
            "sa": {
                "stage": "S_OPEN",
                "terminal": false,
                "outpoint": {"transactionId": "02".repeat(32), "index": 1},
                "spk": shard_a_spk,
                "utxoDaa": "1000",
                "value": "200000000",
                "state": {"records": [{"cumTickets": 1, "buyerPk": "11".repeat(32)}]},
            },
            "sb": {
                "stage": "S_OPEN",
                "terminal": false,
                "outpoint": {"transactionId": "03".repeat(32), "index": 1},
                "spk": shard_b_spk,
                "utxoDaa": "1000",
                "value": "200000000",
                "state": {"records": [{"cumTickets": 1, "buyerPk": "11".repeat(32)}]},
            }
        }
    });
    e.store
        .tx(|db| db.put_round(&initial, Some(&mgr_addr), now_ms(), "kusdt-v2@1.0.0"))
        .unwrap();

    let rec = e.store.db().round(&id).unwrap().unwrap();
    let initial_addrs = Engine::unique_addrs(&e.round_active_targets(&rec));
    e.subscribe_many(&initial_addrs).await.unwrap();
    let subs0 = e.subscriptions();
    assert!(subs0.contains(&mgr_addr));
    assert!(subs0.contains(&shard_a_addr));
    assert!(subs0.contains(&shard_b_addr));

    // Next state: shard A reaches 256 records (must unsubscribe), shard B transitions to S_FROZEN (must unsubscribe).
    let full_records: Vec<Value> = (0..256)
        .map(|i| json!({"cumTickets": i + 1, "buyerPk": "11".repeat(32)}))
        .collect();
    let next_state = json!({
        "id": id,
        "cid": "90".repeat(32),
        "tip": {"transactionId": "04".repeat(32), "index": 0},
        "spk": mgr_spk,
        "value": "400000000",
        "utxoDaa": "1010",
        "accepting": "cc".repeat(32),
        "manager": {
            "outpoint": {"transactionId": "04".repeat(32), "index": 0},
            "spk": mgr_spk,
            "utxoDaa": "1010",
            "value": "400000000",
        },
        "shards": {
            "sa": {
                "stage": "S_OPEN",
                "terminal": false,
                "outpoint": {"transactionId": "05".repeat(32), "index": 1},
                "spk": shard_a_spk,
                "utxoDaa": "1010",
                "value": "200000000",
                "state": {"records": full_records},
            },
            "sb": {
                "stage": "S_FROZEN",
                "terminal": false,
                "outpoint": {"transactionId": "06".repeat(32), "index": 1},
                "spk": shard_b_frozen_spk,
                "utxoDaa": "1010",
                "value": "200000000",
                "state": {"records": [{"cumTickets": 1, "buyerPk": "11".repeat(32)}]},
            }
        }
    });
    let next_addrs = Engine::unique_addrs(&KV2::active_watch_targets(&next_state, "kaspa"));
    let (to_sub, to_unsub) = Engine::diff_subscription_addrs(&initial_addrs, &next_addrs);
    assert!(to_sub.is_empty());
    assert!(to_unsub.contains(&shard_a_addr));
    assert!(to_unsub.contains(&shard_b_addr));
    assert!(!to_unsub.contains(&mgr_addr));
    e.unsubscribe_many(&to_unsub).await.unwrap();
    let subs1 = e.subscriptions();
    assert_eq!(subs1, vec![mgr_addr]);
    assert!(!subs1.contains(&shard_b_frozen_addr));
    e.node.close().await;
}

#[tokio::test]
async fn kusdt_v2_resolve_spend_advances_non_first_payout_shard_and_unsubscribes() {
    use crate::address::spk_to_address;
    use crate::core::bytes::hex;
    use crate::core::kusdt_v2::{self as K, ShardState, Stage, encode_shard_state, redeem_p2sh};
    use crate::core::state::script_for;

    let s_payout_artifact: Value = serde_json::from_str(
        &std::fs::read_to_string("/root/kaspa/kaswin/contracts/kusdt-v2/artifacts/s_payout.json")
            .unwrap(),
    )
    .unwrap();
    let bc: Vec<u8> = s_payout_artifact["contracts"]["KusdtShardPayout"]["compiled"]["bytecode"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u8)
        .collect();
    let (_, s_payout_tail) = K::split_redeem_script(&bc).unwrap();

    let cid = "90".repeat(32);
    let scid_a = "11".repeat(32);
    let scid_b = "22".repeat(32);
    let first_buyer_pk = "33".repeat(32);
    let winner_pk = "44".repeat(32);

    let st_a = ShardState::Payout {
        version: K::VERSION,
        first_buyer_pk: first_buyer_pk.clone(),
        winner_pk: winner_pk.clone(),
        vault_amount: 1_000_000,
        manager_cid: cid.clone(),
        shard_tickets: 1,
    };
    let redeem_a = script_for(&encode_shard_state(&st_a).unwrap(), &s_payout_tail).unwrap();
    let spk_a = redeem_p2sh(&redeem_a).unwrap();
    let addr_a = spk_to_address(spk_a.version as i64, &spk_a.script, "kaspa").unwrap();

    let st_b = ShardState::Payout {
        version: K::VERSION,
        first_buyer_pk: first_buyer_pk.clone(),
        winner_pk: winner_pk.clone(),
        vault_amount: 2_000_000,
        manager_cid: cid.clone(),
        shard_tickets: 2,
    };
    let redeem_b = script_for(&encode_shard_state(&st_b).unwrap(), &s_payout_tail).unwrap();
    let spk_b = redeem_p2sh(&redeem_b).unwrap();
    let addr_b = spk_to_address(spk_b.version as i64, &spk_b.script, "kaspa").unwrap();

    let op_a_txid = "0a".repeat(32);
    let op_b_txid = "0b".repeat(32);
    let payout_b_txid = "0c".repeat(32);
    let blk_hash = "dd".repeat(32);
    let parent_hash = "ee".repeat(32);

    // Build PAYOUT_SELF signatureScript for shard B: action=2, kcc20Suffix="", redeem_b
    let mut sig_b = Vec::new();
    sig_b.push(0x52); // OP_2 (action = 2)
    sig_b.push(0x00); // empty bytes (kcc20Suffix)
    let n = redeem_b.len();
    if n <= 0xff {
        sig_b.push(0x4c);
        sig_b.push(n as u8);
    } else {
        sig_b.push(0x4d);
        sig_b.extend_from_slice(&(n as u16).to_le_bytes());
    }
    sig_b.extend_from_slice(&redeem_b);

    let payout_b_tx = json!({
        "version": 1,
        "lockTime": "0",
        "inputs": [{
            "previousOutpoint": {"transactionId": op_b_txid, "index": 1},
            "signatureScript": hex(&sig_b),
            "sequence": "0",
            "verboseData": {"utxoEntry": {"amount": "200000000"}}
        }],
        "outputs": [{
            "value": "199900000",
            "scriptPublicKey": {"version": 0, "scriptPublicKey": format!("aa20{}87", "00".repeat(32))}
        }],
        "verboseData": {"transactionId": payout_b_txid, "blockHash": blk_hash}
    });

    let addr_a_clone = addr_a.clone();
    let spk_a_script = spk_a.script.clone();
    let op_a_txid_clone = op_a_txid.clone();
    let blk_hash_clone = blk_hash.clone();
    let parent_hash_clone = parent_hash.clone();
    let payout_b_tx_clone = payout_b_tx.clone();

    let fake = crate::checks::fake(
        "engine",
        "mainnet",
        Arc::new(move |req| match req["method"].as_str().unwrap_or("") {
            // Only shard A is live; shard B has been spent!
            "getUtxosByAddresses" => Some(json!({
                "params": {
                    "entries": [{
                        "address": addr_a_clone,
                        "outpoint": {"transactionId": op_a_txid_clone, "index": 1},
                        "utxoEntry": {
                            "amount": "200000000",
                            "scriptPublicKey": {"version": 0, "scriptPublicKey": spk_a_script},
                            "blockDaaScore": "100",
                            "isCoinbase": false
                        }
                    }]
                }
            })),
            "getBlockDagInfo" => Some(json!({"params": {"virtualDaaScore": "101", "sink": blk_hash_clone}})),
            "getBlock" => {
                let h = req["params"]["hash"].as_str().unwrap_or("");
                if h == blk_hash_clone {
                    Some(json!({
                        "params": {
                            "block": {
                                "header": {"hash": blk_hash_clone, "daaScore": "101", "blueScore": "101"},
                                "verboseData": {"isChainBlock": true, "selectedParentHash": parent_hash_clone},
                                "transactions": [payout_b_tx_clone]
                            }
                        }
                    }))
                } else {
                    Some(json!({
                        "params": {
                            "block": {
                                "header": {"hash": h, "daaScore": "100", "blueScore": "100"},
                                "verboseData": {"isChainBlock": true, "selectedParentHash": parent_hash_clone},
                                "transactions": []
                            }
                        }
                    }))
                }
            }
            "getVirtualChainFromBlockV2" => Some(json!({
                "params": {
                    "addedChainBlockHashes": [blk_hash_clone],
                    "removedChainBlockHashes": [],
                    "chainBlockAcceptedTransactions": [{
                        "chainBlockHeader": {"hash": blk_hash_clone, "daaScore": "101", "blueScore": "101"},
                        "acceptedTransactions": [payout_b_tx_clone]
                    }]
                }
            })),
            "getMempoolEntriesByAddresses" => Some(json!({"params": {"entries": []}})),
            _ => None,
        }),
    )
    .await;
    let (node, _) = crate::dual_node::DualNode::open(
        vec![fake.url.clone()],
        crate::checks::opener(),
        None,
    )
    .await
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("contracts.json");
    std::fs::write(
        &cfg_path,
        serde_json::to_string(&json!({
            "contracts": [{
                "module": "kusdt-v2",
                "retired": false,
                "options": {}
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let store = Arc::new(
        crate::store::Store::open(
            dir.path().join("test.sqlite").to_str().unwrap(),
            None,
            "mainnet",
        )
        .unwrap(),
    );
    let contracts = Arc::new(
        crate::contracts::load_contracts(
            Some(cfg_path.to_str().unwrap()),
            &crate::contracts::LoadCtx {
                network: "mainnet",
                registry_addresses: None,
            },
        )
        .unwrap(),
    );
    let e = Engine::new(
        store,
        contracts,
        node,
        &crate::networks::MAINNET,
        Default::default(),
    );
    let _server = fake;
    let _dir = dir;

    let round_id = "aa".repeat(32);
    let round_data = json!({
        "id": round_id,
        "cid": cid,
        "profileId": K::M_OPEN_HASH,
        "stage": "SHARD_ONLY",
        "tip": {"transactionId": op_a_txid, "index": 1},
        "spk": {"version": spk_a.version, "script": spk_a.script},
        "value": "200000000",
        "utxoDaa": "100",
        "accepting": parent_hash,
        "manager": null,
        "shards": {
            scid_a.clone(): {
                "shardCid": scid_a,
                "stage": Stage::SPayout.as_str(),
                "outpoint": {"transactionId": op_a_txid, "index": 1},
                "value": "200000000",
                "spk": {"version": spk_a.version, "script": spk_a.script},
                "utxoDaa": "100",
                "redeemHex": hex(&redeem_a),
                "firstBuyerPk": first_buyer_pk,
                "tickets": 1,
                "terminal": false,
            },
            scid_b.clone(): {
                "shardCid": scid_b,
                "stage": Stage::SPayout.as_str(),
                "outpoint": {"transactionId": op_b_txid, "index": 1},
                "value": "200000000",
                "spk": {"version": spk_b.version, "script": spk_b.script},
                "utxoDaa": "100",
                "redeemHex": hex(&redeem_b),
                "firstBuyerPk": first_buyer_pk,
                "tickets": 2,
                "terminal": false,
            }
        },
        "wallets": {}
    });
    e.store
        .tx(|db| db.put_round(&round_data, Some(&addr_a), now_ms(), "kusdt-v2@1.0.0"))
        .unwrap();
    e.subscribe_many(&[addr_a.clone(), addr_b.clone()])
        .await
        .unwrap();
    assert!(e.subscriptions().contains(&addr_a));
    assert!(e.subscriptions().contains(&addr_b));

    let res = e.resolve_spend(&round_id, None).await.unwrap();
    assert_eq!(res, "APPLIED");
    let updated = e.store.db().round(&round_id).unwrap().unwrap();
    assert_eq!(updated.data["shards"][&scid_b]["terminal"], true);
    assert_eq!(updated.data["shards"][&scid_a]["terminal"], false);
    // Shard B address must be unsubscribed, while Shard A stays subscribed
    let subs = e.subscriptions();
    assert!(subs.contains(&addr_a));
    assert!(!subs.contains(&addr_b));
    e.node.close().await;
}
