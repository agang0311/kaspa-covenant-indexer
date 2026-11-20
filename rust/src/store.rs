//! Single-writer SQLite cache with the SAME schema as the JS build (`src/store.mjs`), so either build can open the
//! other's database. Chain + covenant rules stay the source of truth. Every mutation that changes a round also appends
//! the relay outbox row in the same transaction. One Store is bound to ONE network (persisted in meta, fail closed).
use crate::util::{js_string, now_ms};
use anyhow::{Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::{Value, json};
use std::sync::{Mutex, MutexGuard};

pub fn enc(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

fn dec(s: Option<String>) -> Value {
    match s {
        None => Value::Null,
        Some(s) => serde_json::from_str(&s).unwrap_or(Value::Null),
    }
}

fn enc_compressed(v: &Value) -> Vec<u8> {
    use flate2::{Compression, write::ZlibEncoder};
    let mut e = ZlibEncoder::new(Vec::new(), Compression::fast());
    if serde_json::to_writer(&mut e, v).is_ok()
        && let Ok(bytes) = e.finish()
    {
        return bytes;
    }
    enc(v).into_bytes()
}

fn dec_ref(v: rusqlite::types::ValueRef<'_>) -> Value {
    use flate2::read::ZlibDecoder;
    use rusqlite::types::ValueRef;
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Text(b) => serde_json::from_slice(b).unwrap_or(Value::Null),
        ValueRef::Blob(b) => {
            let mut d = ZlibDecoder::new(b);
            serde_json::from_reader(&mut d)
                .unwrap_or_else(|_| serde_json::from_slice(b).unwrap_or(Value::Null))
        }
        _ => Value::Null,
    }
}

#[derive(Clone, Debug)]
pub struct RoundRow {
    pub id: String,
    pub contract: Option<String>,
    pub data: Value,
    pub address: Option<String>,
    pub status: String,
    pub live_seen_at: Option<i64>,
    pub updated_at: i64,
}

#[derive(Clone, Debug)]
pub struct TransitionEventRow {
    pub txid: String,
    pub seq: i64,
    pub kind: String,
    pub status: String,
    pub accepting: String,
    pub accepting_daa: String,
    pub observed_at: i64,
}

#[derive(Clone, Debug)]
pub struct UnfinalizedHead {
    pub txid: String,
    pub round_id: String,
    pub seq: i64,
    pub accepting: String,
    pub accepting_blue_score: String,
}

#[derive(Clone, Debug)]
pub struct TransitionRow {
    pub txid: String,
    pub round_id: String,
    pub contract: Option<String>,
    pub seq: i64,
    pub kind: String,
    pub status: String,
    pub accepting: String,
    pub accepting_daa: String,
    pub accepting_blue_score: String,
    pub containing: Option<String>,
    pub previous: Value,
    pub next: Value,
    pub evidence: Value,
    pub observed_at: i64,
}

impl TransitionRow {
    pub fn to_json(&self) -> Value {
        json!({"txid": self.txid, "roundId": self.round_id, "contract": self.contract, "seq": self.seq, "kind": self.kind,
            "status": self.status, "accepting": self.accepting, "acceptingDaa": self.accepting_daa,
            "acceptingBlueScore": self.accepting_blue_score, "containing": self.containing, "previous": self.previous,
            "next": self.next, "evidence": self.evidence, "observedAt": self.observed_at})
    }
}

#[derive(Clone, Debug)]
pub struct Checkpoint {
    pub at: i64,
    pub sink: String,
    pub virtual_daa: String,
    pub sink_blue_score: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Discovery {
    pub txid: String,
    pub hint: Value,
    pub status: String,
    pub attempts: i64,
    pub error: Option<String>,
}

/// A transition to insert (seq allocated inside the write transaction).
pub struct NewTransition<'a> {
    pub txid: &'a str,
    pub round_id: &'a str,
    pub contract: &'a str,
    pub seq: i64,
    pub kind: &'a str,
    pub status: &'a str,
    pub accepting: &'a str,
    pub accepting_daa: &'a str,
    pub accepting_blue_score: &'a str,
    pub containing: Option<&'a str>,
    pub previous: Option<&'a Value>,
    pub next: &'a Value,
    pub evidence: &'a Value,
    pub observed_at: i64,
}

pub struct Db(Connection);

pub struct Store {
    inner: Mutex<Db>,
    pub network: String,
    pub path: String,
}

fn row_round(r: &Row) -> rusqlite::Result<RoundRow> {
    Ok(RoundRow {
        id: r.get("id")?,
        contract: r.get("contract")?,
        data: dec(r.get("data")?),
        address: r.get("address")?,
        status: r.get("status")?,
        live_seen_at: r.get("live_seen_at")?,
        updated_at: r.get("updated_at")?,
    })
}

fn row_transition(r: &Row) -> rusqlite::Result<TransitionRow> {
    Ok(TransitionRow {
        txid: r.get("txid")?,
        round_id: r.get("round_id")?,
        contract: r.get("contract")?,
        seq: r.get("seq")?,
        kind: r.get("kind")?,
        status: r.get("status")?,
        accepting: r.get("accepting")?,
        accepting_daa: r.get("accepting_daa")?,
        accepting_blue_score: r.get("accepting_blue_score")?,
        containing: r.get("containing")?,
        previous: dec_ref(r.get_ref("previous")?),
        next: dec(r.get("next")?),
        evidence: dec(r.get("evidence")?),
        observed_at: r.get("observed_at")?,
    })
}

impl Store {
    pub fn open(path: &str, default_contract: Option<&str>, network: &str) -> Result<Store> {
        let conn = Connection::open(path)?;
        let av: i64 = conn
            .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
            .unwrap_or(0);
        if av != 2 {
            let _ = conn.execute_batch("PRAGMA auto_vacuum=INCREMENTAL; VACUUM;");
        }
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA cache_size=-1024; PRAGMA temp_store=FILE;
      CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS rounds(
        id TEXT PRIMARY KEY, contract TEXT, data TEXT NOT NULL, address TEXT, status TEXT NOT NULL,
        live_seen_at INTEGER, updated_at INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS transitions(
        txid TEXT NOT NULL, round_id TEXT NOT NULL REFERENCES rounds(id), contract TEXT, seq INTEGER NOT NULL,
        kind TEXT NOT NULL, status TEXT NOT NULL, accepting TEXT NOT NULL, accepting_daa TEXT NOT NULL,
        accepting_blue_score TEXT NOT NULL, containing TEXT, previous TEXT, next TEXT NOT NULL,
        evidence TEXT NOT NULL, observed_at INTEGER NOT NULL,
        PRIMARY KEY(round_id, seq));
      CREATE INDEX IF NOT EXISTS transitions_status ON transitions(status);
      CREATE TABLE IF NOT EXISTS checkpoints(at INTEGER NOT NULL, sink TEXT NOT NULL, virtual_daa TEXT NOT NULL, sink_blue_score TEXT);
      CREATE INDEX IF NOT EXISTS checkpoints_at ON checkpoints(at);
      CREATE TABLE IF NOT EXISTS outbox(id INTEGER PRIMARY KEY AUTOINCREMENT, payload TEXT NOT NULL, created_at INTEGER NOT NULL, delivered_at INTEGER);
      CREATE TABLE IF NOT EXISTS discoveries(txid TEXT PRIMARY KEY, hint TEXT NOT NULL, status TEXT NOT NULL,
        attempts INTEGER NOT NULL DEFAULT 0, error TEXT, updated_at INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS journal(id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, kind TEXT NOT NULL, data TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS round_receipts(
        round_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        receipt TEXT NOT NULL,
        PRIMARY KEY(round_id, seq)
      );
      CREATE TABLE IF NOT EXISTS round_preimages(
        round_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        hash TEXT NOT NULL,
        redeem_hex TEXT NOT NULL,
        PRIMARY KEY(round_id, hash)
      );
      CREATE INDEX IF NOT EXISTS round_preimages_round_seq ON round_preimages(round_id, seq);",
        )?;
        let db = Db(conn);
        // Network binding: claim an empty DB, or fail closed when the file belongs to a different network.
        match db.get_meta("network")? {
            Value::Null => db.set_meta("network", &json!(network))?,
            Value::String(b) if b == network => {}
            other => bail!(
                "NETWORK_MISMATCH_DB {} != {}",
                js_string(Some(&other)),
                network
            ),
        }
        // Migration from the single-contract schema.
        for t in ["rounds", "transitions"] {
            let mut st = db.0.prepare(&format!("PRAGMA table_info({t})"))?;
            let has: bool = st
                .query_map([], |r| r.get::<_, String>("name"))?
                .filter_map(|x| x.ok())
                .any(|n| n == "contract");
            if !has {
                db.0.execute_batch(&format!("ALTER TABLE {t} ADD COLUMN contract TEXT"))?;
            }
        }
        db.0.execute_batch(
            "CREATE INDEX IF NOT EXISTS rounds_cid ON rounds(json_extract(data,'$.cid'))",
        )?;
        let orphans: i64 = db.0.query_row(
            "SELECT COUNT(*) n FROM rounds WHERE contract IS NULL",
            [],
            |r| r.get(0),
        )?;
        if orphans > 0 {
            let Some(dc) = default_contract else {
                bail!("LEGACY_ROUNDS_NEED_DEFAULT_CONTRACT {orphans}")
            };
            db.0.execute("UPDATE rounds SET contract=? WHERE contract IS NULL", [dc])?;
            db.0.execute(
                "UPDATE transitions SET contract=? WHERE contract IS NULL",
                [dc],
            )?;
        }
        // One accepted transaction is applied to a round at most once (rolled-back rows stay as history).
        let dup: Option<(String, String)> = db
            .0
            .query_row(
                "SELECT round_id, txid FROM transitions WHERE status!='ROLLED_BACK' GROUP BY round_id, txid HAVING COUNT(*)>1 LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((r, t)) = dup {
            bail!("ACTIVE_DUPLICATE_TRANSITION {r} {t}: resolve manually before upgrading");
        }
        db.0.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS transitions_active_txid ON transitions(round_id, txid) WHERE status!='ROLLED_BACK'")?;
        let store = Store {
            inner: Mutex::new(db),
            network: network.to_string(),
            path: path.to_string(),
        };
        let migrated = store.tx(|db| db.migrate_round_extras())?;
        let free_pages: i64 = store
            .db()
            .0
            .query_row("PRAGMA freelist_count", [], |r| r.get(0))
            .unwrap_or(0);
        if migrated > 0 || free_pages > 256 {
            let _ = run_incremental_vacuum(&store.db().0, 0);
        }
        Ok(store)
    }

    /// Lock for a short, synchronous sequence of statements. Never hold across an await.
    pub fn db(&self) -> MutexGuard<'_, Db> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `BEGIN IMMEDIATE` … `COMMIT` (ROLLBACK on error).
    pub fn tx<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> Result<T> {
        let db = self.db();
        db.0.execute_batch("BEGIN IMMEDIATE")?;
        match f(&db) {
            Ok(v) => {
                db.0.execute_batch("COMMIT")?;
                Ok(v)
            }
            Err(e) => {
                let _ = db.0.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
}

impl Db {
    pub fn conn(&self) -> &Connection {
        &self.0
    }

    pub fn get_meta(&self, key: &str) -> Result<Value> {
        let v: Option<String> = self
            .0
            .prepare_cached("SELECT value FROM meta WHERE key=?")?
            .query_row([key], |r| r.get(0))
            .optional()?;
        Ok(dec(v))
    }
    pub fn set_meta(&self, key: &str, value: &Value) -> Result<()> {
        self.0
            .prepare_cached("INSERT INTO meta(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")?
            .execute(params![key, enc(value)])?;
        Ok(())
    }

    pub fn discovery(&self, id: &str) -> Result<Option<Discovery>> {
        Ok(self
            .0
            .prepare_cached("SELECT txid,hint,status,attempts,error FROM discoveries WHERE txid=?")?
            .query_row([id], |r| {
                Ok(Discovery {
                    txid: r.get(0)?,
                    hint: dec(r.get(1)?),
                    status: r.get(2)?,
                    attempts: r.get(3)?,
                    error: r.get(4)?,
                })
            })
            .optional()?)
    }
    /// Merge a discovery hint (JS spread semantics: keys present in `hint`, including nulls, override).
    pub fn discover_task(&self, id: &str, hint: &Value, now: i64) -> Result<Discovery> {
        let old = self.discovery(id)?;
        let mut merged = old
            .as_ref()
            .and_then(|o| o.hint.as_object().cloned())
            .unwrap_or_default();
        if let Some(h) = hint.as_object() {
            for (k, v) in h {
                merged.insert(k.clone(), v.clone());
            }
        }
        let old_event = old
            .as_ref()
            .and_then(|o| o.hint.get("eventAt"))
            .filter(|v| !v.is_null())
            .cloned();
        let event_at = old_event
            .or_else(|| hint.get("eventAt").filter(|v| !v.is_null()).cloned())
            .unwrap_or(json!(now));
        merged.insert("eventAt".into(), event_at);
        let changed = old
            .as_ref()
            .map(|o| js_string(o.hint.get("daa")) != js_string(merged.get("daa")))
            .unwrap_or(false);
        let status = match old.as_ref().map(|o| o.status.as_str()) {
            Some("APPLIED") => "APPLIED",
            Some("IGNORED") if !changed => "IGNORED",
            _ => "PENDING",
        };
        self.0
            .prepare_cached(
                "INSERT INTO discoveries(txid,hint,status,updated_at) VALUES(?,?,?,?)
      ON CONFLICT(txid) DO UPDATE SET hint=excluded.hint,status=excluded.status,updated_at=excluded.updated_at",
            )?
            .execute(params![id, enc(&Value::Object(merged)), status, now])?;
        self.discovery(id)?
            .ok_or_else(|| anyhow!("DISCOVERY_WRITE_LOST"))
    }
    pub fn discovery_result(
        &self,
        id: &str,
        status: &str,
        now: i64,
        error: Option<&str>,
    ) -> Result<()> {
        self.0
            .prepare_cached("UPDATE discoveries SET status=?,error=?,attempts=attempts+1,updated_at=? WHERE txid=?")?
            .execute(params![status, error, now, id])?;
        Ok(())
    }
    pub fn force_discovery_pending(&self, id: &str, clear_error: bool) -> Result<()> {
        if clear_error {
            self.0.execute(
                "UPDATE discoveries SET status='PENDING', attempts=0, error=NULL WHERE txid=?",
                [id],
            )?;
        } else {
            self.0.execute(
                "UPDATE discoveries SET attempts=0, status='PENDING' WHERE txid=?",
                [id],
            )?;
        }
        Ok(())
    }
    pub fn pending_discoveries(&self) -> Result<Vec<(String, Value)>> {
        let mut st = self.0.prepare_cached("SELECT txid,hint FROM discoveries WHERE status='PENDING' ORDER BY updated_at LIMIT 100")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, dec(r.get(1)?))))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn pending_discovery_count(&self) -> Result<i64> {
        Ok(self.0.query_row(
            "SELECT COUNT(*) n FROM discoveries WHERE status='PENDING'",
            [],
            |r| r.get(0),
        )?)
    }
    pub fn reset_ignored_discoveries(&self) -> Result<usize> {
        Ok(self.0.execute(
            "UPDATE discoveries SET status='PENDING', error=NULL WHERE status='IGNORED'",
            [],
        )?)
    }

    pub fn round(&self, id: &str) -> Result<Option<RoundRow>> {
        Ok(self
            .0
            .prepare_cached("SELECT * FROM rounds WHERE id=?")?
            .query_row([id], row_round)
            .optional()?)
    }
    pub fn round_headers(&self) -> Result<Vec<(String, Option<String>, Option<String>, String)>> {
        let mut st = self
            .0
            .prepare_cached("SELECT id, contract, address, status FROM rounds")?;
        Ok(st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn rounds(&self, live_only: bool, active_only: bool) -> Result<Vec<RoundRow>> {
        let wh = if live_only {
            "WHERE status='LIVE'"
        } else if active_only {
            "WHERE status IN ('LIVE', 'STALE') AND json_extract(data,'$.tip') IS NOT NULL"
        } else {
            ""
        };
        let mut st = self
            .0
            .prepare_cached(&format!("SELECT * FROM rounds {wh} ORDER BY id"))?;
        Ok(st
            .query_map([], row_round)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn count_rounds(&self, live_only: bool) -> Result<i64> {
        let q = if live_only {
            "SELECT COUNT(*) FROM rounds WHERE status='LIVE'"
        } else {
            "SELECT COUNT(*) FROM rounds"
        };
        Ok(self.0.query_row(q, [], |r| r.get(0))?)
    }
    pub fn by_cid(&self, cid: &str) -> Result<Vec<RoundRow>> {
        let mut st = self.0.prepare_cached(
            "SELECT * FROM rounds WHERE (json_extract(data,'$.cid')=? OR id=? OR json_extract(data,'$.genesisTxid')=?) AND status!='ROLLED_BACK' LIMIT 2",
        )?;
        Ok(st
            .query_map([cid, cid, cid], row_round)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn cid_page(
        &self,
        after: &str,
        limit: i64,
        category: Option<&str>,
        phase: Option<i64>,
        terminal: Option<bool>,
    ) -> Result<Vec<RoundRow>> {
        // Same rule as the API categoryOf: Kaswin V2.0.2 is the only supported contract family. '.' is literal in LIKE.
        let bucket =
            "CASE WHEN status='STALE' OR contract NOT LIKE 'kaswin-v2.0.2@%' THEN 'unknown'
      WHEN status='TERMINAL' THEN 'close'
      WHEN json_extract(data,'$.state.phase')=1 THEN 'open'
      WHEN json_extract(data,'$.state.phase')=2 THEN 'sealed'
      WHEN json_extract(data,'$.state.phase')=5 THEN 'close' ELSE 'unknown' END";
        let sql = format!(
            "SELECT * FROM rounds WHERE json_extract(data,'$.cid')>? AND status!='ROLLED_BACK'
      AND (? IS NULL OR ({bucket})=?)
      AND (? IS NULL OR json_extract(data,'$.state.phase')=?)
      AND (? IS NULL OR (status='TERMINAL')=?)
      ORDER BY json_extract(data,'$.cid'),id LIMIT ?"
        );
        let t = terminal.map(|b| b as i64);
        let mut st = self.0.prepare_cached(&sql)?;
        Ok(st
            .query_map(
                params![after, category, category, phase, phase, t, t, limit],
                row_round,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn put_round(
        &self,
        round: &Value,
        address: Option<&str>,
        now: i64,
        contract: &str,
    ) -> Result<()> {
        if contract.is_empty() {
            bail!("ROUND_WITHOUT_CONTRACT");
        }
        let id = round
            .get("id")
            .and_then(|i| i.as_str())
            .ok_or_else(|| anyhow!("ROUND_WITHOUT_ID"))?;
        let has_tip = crate::util::truthy(round.get("tip"));
        let status = if has_tip { "LIVE" } else { "TERMINAL" };
        let old: Option<Option<String>> = self
            .0
            .prepare_cached("SELECT contract FROM rounds WHERE id=?")?
            .query_row([id], |r| r.get(0))
            .optional()?;
        if let Some(old) = old
            && old.as_deref() != Some(contract)
        {
            bail!("ROUND_CONTRACT_CANNOT_CHANGE");
        }
        self.0
            .prepare_cached(
                "INSERT INTO rounds(id,contract,data,address,status,live_seen_at,updated_at) VALUES(?,?,?,?,?,NULL,?)
      ON CONFLICT(id) DO UPDATE SET data=excluded.data,address=excluded.address,status=excluded.status,
        live_seen_at=CASE WHEN json_extract(rounds.data,'$.tip.transactionId')=json_extract(excluded.data,'$.tip.transactionId')
          AND json_extract(rounds.data,'$.tip.index')=json_extract(excluded.data,'$.tip.index') THEN rounds.live_seen_at ELSE NULL END,
        updated_at=excluded.updated_at",
            )?
            .execute(params![id, contract, enc(round), if has_tip { address } else { None }, status, now])?;
        Ok(())
    }
    pub fn set_round_status(&self, id: &str, status: &str, now: i64) -> Result<()> {
        self.0
            .prepare_cached("UPDATE rounds SET status=?, updated_at=? WHERE id=?")?
            .execute(params![status, now, id])?;
        Ok(())
    }
    pub fn mark_round_rolled_back(&self, id: &str, now: i64) -> Result<()> {
        self.0.execute(
            "UPDATE rounds SET status='ROLLED_BACK', address=NULL, updated_at=? WHERE id=?",
            params![now, id],
        )?;
        Ok(())
    }
    pub fn mark_live_seen(&self, id: &str, at: i64) -> Result<()> {
        self.0
            .prepare_cached("UPDATE rounds SET live_seen_at=? WHERE id=?")?
            .execute(params![at, id])?;
        self.set_meta(&format!("tipSpent:{id}"), &Value::Null)
    }
    /// The node showed this round's indexed tip as spent before the successor was resolved. Bound to that outpoint.
    pub fn mark_tip_spent(&self, id: &str, tip: &Value, at: i64) -> Result<()> {
        let key = format!("tipSpent:{id}");
        if !crate::util::truthy(Some(&self.get_meta(&key)?)) {
            self.set_meta(&key, &json!({"tip": tip, "at": at}))?;
        }
        Ok(())
    }
    pub fn tip_spent(&self, id: &str, tip: &Value) -> Result<Value> {
        let m = self.get_meta(&format!("tipSpent:{id}"))?;
        use crate::normalize::jeq;
        let mt = m.get("tip");
        if crate::util::truthy(Some(&m))
            && crate::util::truthy(Some(tip))
            && jeq(
                mt.and_then(|t| t.get("transactionId")),
                tip.get("transactionId"),
            )
            && jeq(mt.and_then(|t| t.get("index")), tip.get("index"))
        {
            Ok(m.get("at").cloned().unwrap_or(Value::Null))
        } else {
            Ok(Value::Null)
        }
    }

    pub fn active_transition(&self, round_id: &str, txid: &str) -> Result<Option<TransitionRow>> {
        Ok(self
            .0
            .prepare_cached(
                "SELECT * FROM transitions WHERE round_id=? AND txid=? AND status!='ROLLED_BACK'",
            )?
            .query_row([round_id, txid], row_transition)
            .optional()?)
    }
    pub fn has_rolled_back_transition(&self, round_id: &str, txid: &str) -> Result<bool> {
        let hit: Option<i64> = self
            .0
            .prepare_cached(
                "SELECT 1 FROM transitions WHERE round_id=? AND txid=? AND status='ROLLED_BACK' LIMIT 1",
            )?
            .query_row([round_id, txid], |r| r.get(0))
            .optional()?;
        Ok(hit.is_some())
    }
    pub fn any_active_transition_by_txid(&self, txid: &str) -> Result<Option<TransitionRow>> {
        Ok(self
            .0
            .prepare_cached(
                "SELECT * FROM transitions WHERE txid=? AND status!='ROLLED_BACK' LIMIT 1",
            )?
            .query_row([txid], row_transition)
            .optional()?)
    }
    pub fn last_seq(&self, round_id: &str) -> Result<i64> {
        let s: Option<i64> = self
            .0
            .prepare_cached("SELECT MAX(seq) s FROM transitions WHERE round_id=?")?
            .query_row([round_id], |r| r.get(0))?;
        Ok(s.unwrap_or(-1))
    }
    pub fn add_transition(&self, t: &NewTransition) -> Result<()> {
        let is_final = t.status == "FINAL";
        let prev_enc: Option<Vec<u8>> = if is_final {
            None
        } else {
            t.previous
                .filter(|p| crate::util::truthy(Some(p)))
                .map(enc_compressed)
        };
        let next_enc = if is_final {
            compact_final_next(t.next, t.contract)
        } else {
            compact_active_next(t.next)
        };
        self.0
            .prepare_cached(
                "INSERT INTO transitions(txid,round_id,contract,seq,kind,status,accepting,accepting_daa,accepting_blue_score,containing,previous,next,evidence,observed_at)
      VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            )?
            .execute(params![
                t.txid,
                t.round_id,
                t.contract,
                t.seq,
                t.kind,
                t.status,
                t.accepting,
                t.accepting_daa,
                t.accepting_blue_score,
                t.containing,
                prev_enc,
                next_enc,
                enc(t.evidence),
                t.observed_at
            ])?;
        Ok(())
    }
    pub fn latest_active(&self, round_id: &str) -> Result<Option<TransitionRow>> {
        Ok(self
            .0
            .prepare_cached("SELECT * FROM transitions WHERE round_id=? AND status!='ROLLED_BACK' ORDER BY seq DESC LIMIT 1")?
            .query_row([round_id], row_transition)
            .optional()?)
    }
    pub fn unfinalized(&self) -> Result<Vec<TransitionRow>> {
        let mut st = self.0.prepare_cached(
            "SELECT * FROM transitions WHERE status='ACCEPTED' ORDER BY round_id, seq",
        )?;
        Ok(st
            .query_map([], row_transition)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn unfinalized_heads(&self) -> Result<Vec<UnfinalizedHead>> {
        let mut st = self.0.prepare_cached(
            "SELECT txid, round_id, seq, accepting, accepting_blue_score FROM transitions WHERE status='ACCEPTED' ORDER BY round_id, seq",
        )?;
        Ok(st
            .query_map([], |r| {
                Ok(UnfinalizedHead {
                    txid: r.get(0)?,
                    round_id: r.get(1)?,
                    seq: r.get(2)?,
                    accepting: r.get(3)?,
                    accepting_blue_score: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn transition(&self, round_id: &str, seq: i64) -> Result<Option<TransitionRow>> {
        Ok(self
            .0
            .prepare_cached("SELECT * FROM transitions WHERE round_id=? AND seq=?")?
            .query_row(params![round_id, seq], row_transition)
            .optional()?)
    }
    pub fn transitions(&self, round_id: &str) -> Result<Vec<TransitionRow>> {
        let mut st = self
            .0
            .prepare_cached("SELECT * FROM transitions WHERE round_id=? ORDER BY seq")?;
        Ok(st
            .query_map([round_id], row_transition)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn rolled_back_transitions(&self, round_id: &str) -> Result<Vec<TransitionRow>> {
        let mut st = self.0.prepare_cached(
            "SELECT * FROM transitions WHERE round_id=? AND status='ROLLED_BACK' ORDER BY seq DESC",
        )?;
        Ok(st
            .query_map([round_id], row_transition)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn transition_count(&self, round_id: &str) -> Result<i64> {
        Ok(self
            .0
            .prepare_cached("SELECT COUNT(*) FROM transitions WHERE round_id=?")?
            .query_row([round_id], |r| r.get(0))?)
    }
    pub fn transition_events(
        &self,
        round_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TransitionEventRow>> {
        let mut st = self.0.prepare_cached(
            "SELECT txid, seq, kind, status, accepting, accepting_daa, observed_at FROM transitions WHERE round_id=? ORDER BY seq LIMIT ? OFFSET ?",
        )?;
        Ok(st
            .query_map(params![round_id, limit, offset], |r| {
                Ok(TransitionEventRow {
                    txid: r.get(0)?,
                    seq: r.get(1)?,
                    kind: r.get(2)?,
                    status: r.get(3)?,
                    accepting: r.get(4)?,
                    accepting_daa: r.get(5)?,
                    observed_at: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn set_transition_status(&self, round_id: &str, seq: i64, status: &str) -> Result<()> {
        if matches!(status, "FINAL" | "ROLLED_BACK") {
            let row: Option<(Option<String>, String)> = self
                .0
                .prepare_cached("SELECT contract, next FROM transitions WHERE round_id=? AND seq=?")?
                .query_row(params![round_id, seq], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            if let Some((contract, next_raw)) = row {
                let contract = contract.unwrap_or_default();
                let next_compact = serde_json::from_str::<Value>(&next_raw)
                    .map(|v| compact_final_next(&v, &contract))
                    .unwrap_or(next_raw);
                self.0
                    .prepare_cached("UPDATE transitions SET status=?, previous=NULL, next=? WHERE round_id=? AND seq=?")?
                    .execute(params![status, next_compact, round_id, seq])?;
                return Ok(());
            }
        }
        self.0
            .prepare_cached("UPDATE transitions SET status=? WHERE round_id=? AND seq=?")?
            .execute(params![status, round_id, seq])?;
        Ok(())
    }
    pub fn active_after(&self, round_id: &str, seq: i64) -> Result<Vec<TransitionRow>> {
        let mut st = self.0.prepare_cached("SELECT * FROM transitions WHERE round_id=? AND seq>=? AND status!='ROLLED_BACK' ORDER BY seq DESC")?;
        Ok(st
            .query_map(params![round_id, seq], row_transition)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Cheap change detector of the transitions table (count, max rowid, accepted count).
    pub fn transitions_fingerprint(&self) -> Result<String> {
        Ok(self.0.query_row(
            "SELECT COUNT(*) n, MAX(rowid) m, SUM(status='ACCEPTED') a FROM transitions",
            [],
            |r| {
                Ok(format!(
                    "{}:{}:{}",
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?
                        .map(|x| x.to_string())
                        .unwrap_or("null".into()),
                    r.get::<_, Option<i64>>(2)?
                        .map(|x| x.to_string())
                        .unwrap_or("null".into())
                ))
            },
        )?)
    }

    pub fn add_checkpoint(&self, c: &Checkpoint, keep: i64) -> Result<()> {
        self.0
            .prepare_cached(
                "INSERT INTO checkpoints(at,sink,virtual_daa,sink_blue_score) VALUES(?,?,?,?)",
            )?
            .execute(params![c.at, c.sink, c.virtual_daa, c.sink_blue_score])?;
        self.0
            .prepare_cached("DELETE FROM checkpoints WHERE at < (SELECT MIN(at) FROM (SELECT at FROM checkpoints ORDER BY at DESC LIMIT ?))")?
            .execute([keep])?;
        Ok(())
    }
    /// Newest checkpoints first, all recorded strictly before `before`.
    pub fn checkpoints_before(&self, before: i64, limit: i64) -> Result<Vec<Checkpoint>> {
        let mut st = self.0.prepare_cached("SELECT at,sink,virtual_daa,sink_blue_score FROM checkpoints WHERE at < ? ORDER BY at DESC LIMIT ?")?;
        let rows = st.query_map(params![before, limit], |r| {
            Ok(Checkpoint {
                at: r.get(0)?,
                sink: r.get(1)?,
                virtual_daa: r.get(2)?,
                sink_blue_score: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn oldest_checkpoint_sink(&self) -> Result<Option<String>> {
        Ok(self
            .0
            .query_row(
                "SELECT sink FROM checkpoints ORDER BY at LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn enqueue(&self, payload: &Value, now: i64) -> Result<()> {
        self.0
            .prepare_cached("INSERT INTO outbox(payload,created_at) VALUES(?,?)")?
            .execute(params![enc(payload), now])?;
        Ok(())
    }
    pub fn pending(&self, limit: i64) -> Result<Vec<(i64, Value)>> {
        let mut st = self.0.prepare_cached(
            "SELECT id,payload FROM outbox WHERE delivered_at IS NULL ORDER BY id LIMIT ?",
        )?;
        let rows = st.query_map([limit], |r| Ok((r.get::<_, i64>(0)?, dec(r.get(1)?))))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn pending_count(&self, limit: i64) -> Result<i64> {
        Ok(self.0.query_row(
            "SELECT COUNT(*) FROM (SELECT id FROM outbox WHERE delivered_at IS NULL LIMIT ?)",
            [limit],
            |r| r.get(0),
        )?)
    }
    pub fn delivered(&self, id: i64, at: i64) -> Result<()> {
        self.0
            .prepare_cached("UPDATE outbox SET delivered_at=? WHERE id=?")?
            .execute(params![at, id])?;
        Ok(())
    }

    /// Operator-managed extra pool nodes (probed like any candidate, never trusted). [{url, note, addedAt}].
    pub fn pool_nodes(&self) -> Result<Vec<Value>> {
        Ok(self
            .get_meta("poolNodes")?
            .as_array()
            .cloned()
            .unwrap_or_default())
    }
    pub fn add_pool_node(&self, url: &str, note: Option<&str>, now: i64) -> Result<Vec<Value>> {
        let mut list: Vec<Value> = self
            .pool_nodes()?
            .into_iter()
            .filter(|n| n.get("url").and_then(|u| u.as_str()) != Some(url))
            .collect();
        if list.len() >= 32 {
            bail!("POOL_NODE_LIMIT");
        }
        list.push(json!({"url": url, "note": note, "addedAt": now}));
        self.set_meta("poolNodes", &Value::Array(list.clone()))?;
        Ok(list)
    }
    pub fn remove_pool_node(&self, url: &str) -> Result<Vec<Value>> {
        let list: Vec<Value> = self
            .pool_nodes()?
            .into_iter()
            .filter(|n| n.get("url").and_then(|u| u.as_str()) != Some(url))
            .collect();
        self.set_meta("poolNodes", &Value::Array(list.clone()))?;
        Ok(list)
    }

    pub fn log(&self, kind: &str, data: &Value, at: i64) -> Result<()> {
        let encoded = enc(data);
        let last: Option<(i64, String, String)> = self
            .0
            .prepare_cached("SELECT id, kind, data FROM journal ORDER BY id DESC LIMIT 1")?
            .query_row([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        if let Some((last_id, last_kind, last_data)) = last {
            if last_kind == kind && last_data == encoded {
                self.0
                    .prepare_cached("UPDATE journal SET at=? WHERE id=?")?
                    .execute(params![at, last_id])?;
                return Ok(());
            }
        }
        self.0
            .prepare_cached("INSERT INTO journal(at,kind,data) VALUES(?,?,?)")?
            .execute(params![at, kind, encoded])?;
        self.0
            .prepare_cached("DELETE FROM journal WHERE id <= (SELECT MAX(id) FROM journal) - 5000")?
            .execute([])?;
        Ok(())
    }
    pub fn journal(&self, limit: i64) -> Result<Vec<Value>> {
        let mut st = self
            .0
            .prepare_cached("SELECT at,kind,data FROM journal ORDER BY id DESC LIMIT ?")?;
        let rows = st.query_map([limit], |r| Ok(json!({"at": r.get::<_, i64>(0)?, "kind": r.get::<_, String>(1)?, "data": dec(r.get(2)?)})))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn prune_final_transitions(&self) -> Result<usize> {
        let mut count = 0usize;
        loop {
            let mut st = self.0.prepare_cached(
                "SELECT round_id, seq, contract, next FROM transitions
                 WHERE status IN ('FINAL', 'ROLLED_BACK') AND (
                   previous IS NOT NULL
                   OR LENGTH(next) > 512
                 ) LIMIT 200",
            )?;
            let rows: Vec<(String, i64, Option<String>, String)> = st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(st);
            if rows.is_empty() {
                break;
            }
            count += rows.len();
            let mut upd = self.0.prepare_cached(
                "UPDATE transitions SET previous=NULL, next=? WHERE round_id=? AND seq=?",
            )?;
            for (round_id, seq, contract, next_raw) in rows {
                let c = contract.unwrap_or_default();
                let next_compact = serde_json::from_str::<Value>(&next_raw)
                    .map(|v| compact_final_next(&v, &c))
                    .unwrap_or(next_raw);
                upd.execute(params![next_compact, round_id, seq])?;
            }
            drop(upd);
        }
        loop {
            let mut st = self.0.prepare_cached(
                "SELECT id, payload FROM outbox
                 WHERE payload LIKE '%\"receipts\":%' OR payload LIKE '%\"history\":%'
                 LIMIT 200",
            )?;
            let rows: Vec<(i64, String)> = st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(st);
            if rows.is_empty() {
                break;
            }
            count += rows.len();
            let mut upd = self
                .0
                .prepare_cached("UPDATE outbox SET payload=? WHERE id=?")?;
            for (id, raw) in rows {
                let compact = serde_json::from_str::<Value>(&raw)
                    .map(|mut v| {
                        if let Some(round) = v.get_mut("round").and_then(|r| r.as_object_mut()) {
                            for k in [
                                "latestTransaction",
                                "genesisTransaction",
                                "receipts",
                                "history",
                                "preimages",
                                "redeemHex",
                            ] {
                                round.shift_remove(k);
                            }
                        }
                        enc(&v)
                    })
                    .unwrap_or(raw);
                upd.execute(params![compact, id])?;
            }
            drop(upd);
        }
        Ok(count)
    }

    pub fn vacuum_and_truncate_wal(&self) -> Result<()> {
        let _ = self.0.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);");
        Ok(())
    }

    pub fn incremental_vacuum(&self, pages: usize) -> Result<()> {
        let _ = run_incremental_vacuum(&self.0, pages);
        let _ = self.0.execute_batch("PRAGMA shrink_memory;");
        Ok(())
    }

    pub fn add_round_receipt(&self, round_id: &str, seq: i64, receipt: &Value) -> Result<()> {
        self.0
            .prepare_cached(
                "INSERT OR REPLACE INTO round_receipts(round_id, seq, receipt) VALUES(?, ?, ?)",
            )?
            .execute(params![round_id, seq, enc(receipt)])?;
        Ok(())
    }

    pub fn add_round_preimages(
        &self,
        round_id: &str,
        seq: i64,
        preimages: &[(String, String)],
    ) -> Result<()> {
        if preimages.is_empty() {
            return Ok(());
        }
        let mut st = self.0.prepare_cached(
            "INSERT OR REPLACE INTO round_preimages(round_id, seq, hash, redeem_hex) VALUES(?, ?, ?, ?)",
        )?;
        for (h, hex) in preimages {
            st.execute(params![round_id, seq, h, hex])?;
        }
        Ok(())
    }

    pub fn rollback_round_extras(&self, round_id: &str, from_seq: i64) -> Result<()> {
        self.0
            .prepare_cached("DELETE FROM round_receipts WHERE round_id=? AND seq>=?")?
            .execute(params![round_id, from_seq])?;
        self.0
            .prepare_cached("DELETE FROM round_preimages WHERE round_id=? AND seq>=?")?
            .execute(params![round_id, from_seq])?;
        Ok(())
    }

    pub fn round_receipts(&self, round_id: &str) -> Result<Vec<Value>> {
        let mut st = self
            .0
            .prepare_cached("SELECT receipt FROM round_receipts WHERE round_id=? ORDER BY seq ASC")?;
        Ok(st
            .query_map([round_id], |r| Ok(dec(r.get(0)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn round_preimages(&self, round_id: &str) -> Result<serde_json::Map<String, Value>> {
        let mut st = self
            .0
            .prepare_cached("SELECT hash, redeem_hex FROM round_preimages WHERE round_id=?")?;
        let mut out = serde_json::Map::new();
        let rows = st.query_map([round_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (h, hex) = row?;
            out.insert(h, Value::String(hex));
        }
        Ok(out)
    }

    pub fn round_receipt_txid_at(&self, round_id: &str, seq: i64) -> Result<Option<String>> {
        let row: Option<Option<String>> = self
            .0
            .prepare_cached(
                "SELECT json_extract(receipt, '$.txid') FROM round_receipts WHERE round_id=? AND seq=?",
            )?
            .query_row(params![round_id, seq], |r| r.get(0))
            .optional()?;
        Ok(row.flatten())
    }

    pub fn round_receipt_count_up_to(&self, round_id: &str, seq: i64) -> Result<i64> {
        Ok(self
            .0
            .prepare_cached("SELECT COUNT(*) FROM round_receipts WHERE round_id=? AND seq<=?")?
            .query_row(params![round_id, seq], |r| r.get(0))?)
    }

    pub fn round_receipts_after(&self, round_id: &str, seq: i64) -> Result<Vec<Value>> {
        let mut st = self.0.prepare_cached(
            "SELECT receipt FROM round_receipts WHERE round_id=? AND seq>? ORDER BY seq ASC",
        )?;
        Ok(st
            .query_map(params![round_id, seq], |r| Ok(dec(r.get(0)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn round_preimages_after(
        &self,
        round_id: &str,
        seq: i64,
    ) -> Result<serde_json::Map<String, Value>> {
        let mut st = self.0.prepare_cached(
            "SELECT hash, redeem_hex FROM round_preimages WHERE round_id=? AND seq>?",
        )?;
        let mut out = serde_json::Map::new();
        let rows = st.query_map(params![round_id, seq], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (h, hex) = row?;
            out.insert(h, Value::String(hex));
        }
        Ok(out)
    }

    /// One-time migration: move embedded `receipts` and `preimages` out of `rounds.data` into incremental tables.
    pub fn migrate_round_extras(&self) -> Result<usize> {
        let mut st = self.0.prepare(
            "SELECT id, data FROM rounds WHERE contract LIKE 'rolling-v1%' AND (json_extract(data, '$.receipts') IS NOT NULL OR json_extract(data, '$.preimages') IS NOT NULL)",
        )?;
        let rows: Vec<(String, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(st);
        if rows.is_empty() {
            return Ok(0);
        }
        let mut migrated = 0usize;
        for (id, raw) in rows {
            let mut data: Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let Some(obj) = data.as_object_mut() else {
                continue;
            };
            let receipts = obj
                .shift_remove("receipts")
                .and_then(|v| match v {
                    Value::Array(a) => Some(a),
                    _ => None,
                })
                .unwrap_or_default();
            let preimages = obj
                .shift_remove("preimages")
                .and_then(|v| match v {
                    Value::Object(m) => Some(m),
                    _ => None,
                })
                .unwrap_or_default();
            if !receipts.is_empty() {
                let mut ins = self.0.prepare_cached(
                    "INSERT OR IGNORE INTO round_receipts(round_id, seq, receipt) VALUES(?, ?, ?)",
                )?;
                for (idx, rec) in receipts.iter().enumerate() {
                    ins.execute(params![id, idx as i64, enc(rec)])?;
                }
            }
            if !preimages.is_empty() {
                let mut ins = self.0.prepare_cached(
                    "INSERT OR IGNORE INTO round_preimages(round_id, seq, hash, redeem_hex) VALUES(?, 0, ?, ?)",
                )?;
                for (h, v) in preimages {
                    if let Some(hex) = v.as_str() {
                        ins.execute(params![id, h, hex])?;
                    }
                }
            }
            self.0
                .prepare_cached("UPDATE rounds SET data=? WHERE id=?")?
                .execute(params![enc(&data), id])?;
            migrated += 1;
        }
        Ok(migrated)
    }

    pub fn shrink_memory(&self) {
        let _ = self.0.execute_batch("PRAGMA shrink_memory;");
    }
}

fn run_incremental_vacuum(conn: &Connection, pages: usize) -> Result<()> {
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    let sql = if pages == 0 {
        "PRAGMA incremental_vacuum(1000000)".to_string()
    } else {
        format!("PRAGMA incremental_vacuum({pages})")
    };
    {
        let mut st = conn.prepare(&sql)?;
        let mut rows = st.query([])?;
        while rows.next()?.is_some() {}
    }
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    Ok(())
}

fn compact_active_next(next: &Value) -> String {
    let Some(obj) = next.as_object() else {
        return enc(next);
    };
    let mut slim = serde_json::Map::new();
    for k in [
        "id",
        "cid",
        "version",
        "terminal",
        "step",
        "module",
        "latestTxid",
        "accepting",
        "latestTransaction",
    ] {
        if let Some(v) = obj.get(k) {
            slim.insert(k.to_string(), v.clone());
        }
    }
    enc(&Value::Object(slim))
}

fn compact_final_next(next: &Value, _contract: &str) -> String {
    let Some(obj) = next.as_object() else {
        return enc(next);
    };
    let mut slim = serde_json::Map::new();
    for k in [
        "id",
        "cid",
        "version",
        "terminal",
        "step",
        "module",
        "latestTxid",
        "accepting",
    ] {
        if let Some(v) = obj.get(k) {
            slim.insert(k.to_string(), v.clone());
        }
    }
    enc(&Value::Object(slim))
}

impl Store {
    /// Journal write that never fails the caller (the JS build would throw; a lost journal row is not state).
    pub fn log(&self, kind: &str, data: Value) {
        let _ = self.db().log(kind, &data, now_ms());
    }
    pub fn log_at(&self, kind: &str, data: Value, at: i64) {
        let _ = self.db().log(kind, &data, at);
    }
}
