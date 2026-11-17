//! Bounded accepted-chain window walk (`src/window-scan.mjs`): walks the selected chain from `from` in windows of
//! <= windowBlue blue score with getVirtualChainFromBlockV2 High (minConfirmationCount trims each page, recomputed from
//! a fresh sink blue score before every page), keeping one page in memory. Density varies by >100x, so the window
//! adapts to a per-page transaction target. Runs in-process (Rust returns freed memory; no child process needed).
use crate::dual_node::DualNode;
use crate::util::{canonical, js_bigint, js_string, now_ms};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Acc {
    pub hash: String,
    pub daa: i128,
    pub blue_score: i128,
    pub is_chain: bool,
    pub selected_parent: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Found {
    pub acc: Acc,
    pub tx: Value,
    pub containing: String,
}

#[derive(Clone, Debug)]
pub struct Cursor {
    pub from: String,
    pub from_blue: i128,
    pub w: Option<i128>,
}

pub enum WindowResult {
    Found(Found),
    Done,
    Cursor(Cursor),
}

pub fn page_tx_target() -> usize {
    std::env::var("WINDOW_TX_TARGET")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(4000)
}

pub fn spends_outpoint(tx: &Value, tip: &Value) -> bool {
    crate::normalize::spends_loose(tx, tip)
}

pub fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|a| a.iter().map(|h| js_string(Some(h))).collect())
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
pub async fn scan_window(
    node: &Arc<DualNode>,
    tip: &Value,
    mut from: String,
    mut from_blue: i128,
    sink_blue: i128,
    window_blue: i128,
    max_pages: usize,
    until: i64,
    w: Option<i128>,
) -> Result<WindowResult> {
    let max_window = window_blue;
    let mut w = match w {
        Some(x) if x > 0 => x.min(max_window),
        _ => max_window.min(8),
    };
    let target = page_tx_target();
    for _page in 0..max_pages {
        if now_ms() > until {
            return Ok(WindowResult::Cursor(Cursor {
                from,
                from_blue,
                w: Some(w),
            }));
        }
        let current = js_bigint(
            node.query("getSinkBlueScore", json!({}))
                .await?
                .get("blueScore"),
        )?;
        let want = from_blue + w;
        let min = if current > want { current - want } else { 0 };
        let r = node.query("getVirtualChainFromBlockV2", json!({"startHash": from, "dataVerbosityLevel": "High", "minConfirmationCount": min as u64})).await?;
        if !strings(r.get("removedChainBlockHashes")).is_empty() {
            bail!("WINDOW_START_NOT_CHAIN");
        }
        let added = strings(r.get("addedChainBlockHashes"));
        let empty = vec![];
        let rows = r
            .get("chainBlockAcceptedTransactions")
            .and_then(|x| x.as_array())
            .unwrap_or(&empty);
        if rows.len() != added.len() {
            bail!("VC_ROWS_NOT_ALIGNED");
        }
        for (i, row) in rows.iter().enumerate() {
            let h = row.get("chainBlockHeader");
            if js_string(h.and_then(|h| h.get("hash"))) != added[i] {
                bail!("VC_HEADER_MISMATCH");
            }
            for t in row
                .get("acceptedTransactions")
                .and_then(|x| x.as_array())
                .unwrap_or(&empty)
            {
                if !spends_outpoint(t, tip) {
                    continue;
                }
                let h = h.unwrap();
                if crate::util::is_nullish(h.get("daaScore"))
                    || crate::util::is_nullish(h.get("blueScore"))
                {
                    bail!("VC_HEADER_FIELDS_MISSING");
                }
                let tx = canonical(t);
                let containing = tx
                    .get("verboseData")
                    .and_then(|v| v.get("blockHash"))
                    .filter(|b| crate::util::truthy(Some(b)))
                    .map(|b| js_string(Some(b)));
                if crate::normalize::txid_of(&tx).is_none() || containing.is_none() {
                    bail!("VC_HIGH_ROW_INCOMPLETE");
                }
                return Ok(WindowResult::Found(Found {
                    acc: Acc {
                        hash: added[i].clone(),
                        daa: js_bigint(h.get("daaScore"))?,
                        blue_score: js_bigint(h.get("blueScore"))?,
                        is_chain: true,
                        selected_parent: Some(if i > 0 {
                            added[i - 1].clone()
                        } else {
                            from.clone()
                        }),
                    },
                    tx,
                    containing: containing.unwrap(),
                }));
            }
        }
        if added.is_empty() {
            // Page fully trimmed (next chain block is more than w blue away, or near the sink): widen and retry.
            if from_blue >= sink_blue || current - from_blue <= w {
                return Ok(WindowResult::Done);
            }
            w *= 2;
            continue;
        }
        let txs: usize = rows
            .iter()
            .map(|r| {
                r.get("acceptedTransactions")
                    .and_then(|x| x.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0)
            })
            .sum();
        if w > max_window {
            w = max_window; // a widened (sparse) window never carries into dense pages
        }
        if txs > target && w > 1 {
            w = (w / 2).max(1);
        } else if txs * 4 < target && w < max_window {
            w = (w * 2).min(max_window);
        }
        let last = rows
            .last()
            .and_then(|r| r.get("chainBlockHeader"))
            .and_then(|h| h.get("blueScore"));
        if crate::util::is_nullish(last) {
            bail!("VC_HEADER_FIELDS_MISSING");
        }
        let top = js_bigint(last)?;
        if top >= sink_blue {
            return Ok(WindowResult::Done);
        }
        from = added.last().unwrap().clone();
        from_blue = top;
    }
    Ok(WindowResult::Cursor(Cursor {
        from,
        from_blue,
        w: Some(w),
    }))
}
