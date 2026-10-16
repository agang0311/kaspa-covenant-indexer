// Universal TXID Tracer & Covenant Lifecycle Resolver for KCI.
// Accepts a txid from ANY stage (Genesis, BUY, CLOSE, DRAW, REFUND, etc.),
// traces backward along input 0 to resolve Genesis, extracts the immutable Covenant ID (CID),
// deduplicates against existing rounds/CIDs in SQLite, and triggers automatic catch-up if new.

export async function traceAndTrackTxid(txid, {store, engine, restUrl, now = () => Date.now()}) {
  const cleanTxid = String(txid || '').trim().toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(cleanTxid)) {
    throw new Error('INVALID_TXID: 请输入 64 位十六进制交易哈希');
  }

  // 1. Fast Path: Check if this exact txid is already indexed in transitions table
  const existingTx = store.db.prepare(
    "SELECT * FROM transitions WHERE txid=? AND status!='ROLLED_BACK' LIMIT 1"
  ).get(cleanTxid);

  if (existingTx) {
    const round = store.round(existingTx.round_id);
    const trs = store.transitions(existingTx.round_id);
    return {
      status: 'ALREADY_INDEXED',
      action: 'DEDUPLICATED_BY_TXID',
      message: `该交易已被索引器收录（所属轮次: ${existingTx.round_id.slice(0, 16)}..., 阶段: ${existingTx.kind}, Seq #${existingTx.seq}）`,
      queriedTxid: cleanTxid,
      roundId: existingTx.round_id,
      cid: round?.data?.cid ?? existingTx.round_id,
      contract: existingTx.contract,
      phase: round?.data?.state?.phase,
      terminal: round?.data?.terminal,
      transitionsCount: trs.length,
      traceHops: 0
    };
  }

  // 2. Fast Path: Check if this txid is a known Genesis round in rounds table
  const existingRound = store.round(cleanTxid);
  if (existingRound && existingRound.status !== 'ROLLED_BACK') {
    const trs = store.transitions(cleanTxid);
    return {
      status: 'ALREADY_INDEXED',
      action: 'DEDUPLICATED_BY_ROUND',
      message: `该交易为已收录轮次的创世交易（CID: ${existingRound.data?.cid?.slice(0, 16)}...）`,
      queriedTxid: cleanTxid,
      roundId: existingRound.id,
      cid: existingRound.data?.cid ?? existingRound.id,
      contract: existingRound.contract,
      phase: existingRound.data?.state?.phase,
      terminal: existingRound.data?.terminal,
      transitionsCount: trs.length,
      traceHops: 0
    };
  }

  // 3. Trace backward along input 0 to discover the Genesis transaction and extract CID
  let curTxid = cleanTxid;
  const chainHops = [];
  let genesisTxid = null;
  let extractedCid = null;
  const baseRest = (restUrl || 'https://api-tn10.kaspa.org').replace(/\/+$/, '');

  for (let hop = 0; hop < 64; hop++) {
    // Check if curTxid is already in the database
    const dbRound = store.round(curTxid);
    if (dbRound && dbRound.status !== 'ROLLED_BACK') {
      genesisTxid = dbRound.id;
      extractedCid = dbRound.data?.cid ?? dbRound.id;
      break;
    }

    const dbTx = store.db.prepare(
      "SELECT round_id FROM transitions WHERE txid=? AND status!='ROLLED_BACK' LIMIT 1"
    ).get(curTxid);
    if (dbTx) {
      const parentRound = store.round(dbTx.round_id);
      genesisTxid = dbTx.round_id;
      extractedCid = parentRound?.data?.cid ?? dbTx.round_id;
      break;
    }

    // Fetch transaction from REST API
    let rawTx = null;
    try {
      const ac = new AbortController(), t = setTimeout(() => ac.abort(), 4000);
      t.unref?.();
      const res = await (engine?.fetchImpl ?? globalThis.fetch)(`${baseRest}/transactions/${curTxid}`, {signal: ac.signal});
      clearTimeout(t);
      if (!res.ok) {
        if (hop === 0) throw new Error(`链上未查到该交易 (HTTP ${res.status})，请核对 TXID 是否正确`);
        break;
      }
      rawTx = await res.json();
    } catch (e) {
      if (hop === 0) throw e;
      break;
    }

    chainHops.push({txid: curTxid, payload: rawTx.payload});

    // Check if this hop carries covenant_id in outputs
    const outCid = (rawTx.outputs || []).find(o => o.covenant_id || o.covenant?.covenantId);
    if (outCid) {
      extractedCid = outCid.covenant_id || outCid.covenant?.covenantId;
    }

    // Check if this hop carries the Kaswin Genesis payload marker
    const payload = rawTx.payload || '';
    const isGenesisPayload = typeof payload === 'string' && (
      payload.startsWith('4b415357494e5f47454e45534953') || // 'KASWIN_GENESIS' in hex
      payload.includes('4b573231') ||                         // 'KW21'
      payload.includes('4b573230')                            // 'KW20'
    );

    if (isGenesisPayload) {
      genesisTxid = curTxid;
      break;
    }

    // Move backward along input 0
    const in0 = rawTx.inputs?.[0];
    const prevTxid = in0?.previous_outpoint_hash || in0?.previousOutpoint?.transactionId;
    if (!prevTxid || prevTxid === '0'.repeat(64)) {
      if (hop === 0 && !extractedCid) {
        throw new Error('NOT_COVENANT_TX: 该交易未包含状态契约信息，且前驱无契约链条');
      }
      // If we have an extracted CID, this earliest hop is our Genesis candidate
      genesisTxid = curTxid;
      break;
    }

    curTxid = prevTxid;
  }

  if (!genesisTxid) {
    throw new Error('GENESIS_NOT_RESOLVED: 沿输入链未能追溯到合法的契约创世交易');
  }

  // 4. Deduplication Check against existing CID in database
  if (extractedCid) {
    const byCid = store.byCid(extractedCid);
    if (byCid && byCid.length > 0) {
      const matched = byCid[0];
      const trs = store.transitions(matched.id);
      // If the round is live, notify engine to advance
      if (matched.status === 'LIVE' && engine && typeof engine.resolveSpend === 'function') {
        engine.run('spend:' + matched.id, () => engine.resolveSpend(matched.id));
      }
      return {
        status: 'ALREADY_INDEXED',
        action: 'DEDUPLICATED_BY_CID',
        message: `经回溯定位到契约 CID: ${extractedCid.slice(0, 16)}...，该轮次已存在于索引数据库中（去重成功）`,
        queriedTxid: cleanTxid,
        genesisTxid,
        roundId: matched.id,
        cid: extractedCid,
        contract: matched.contract,
        phase: matched.data?.state?.phase,
        terminal: matched.data?.terminal,
        transitionsCount: trs.length,
        traceHops: chainHops.length
      };
    }
  }

  // 5. Transaction is a NEW covenant round! Enqueue Genesis task to engine for automatic catch-up
  const taskAt = now();
  store.discoverTask(genesisTxid, {eventAt: taskAt}, taskAt);
  store.db.prepare("UPDATE discoveries SET status='PENDING', error=NULL WHERE txid=?").run(genesisTxid);

  if (engine && typeof engine.discoverGenesis === 'function') {
    engine.run('genesis:' + genesisTxid, () => engine.discoverGenesis(genesisTxid));
  }

  return {
    status: 'TRACKED_NEW',
    action: 'CATCHUP_TRIGGERED',
    message: `成功通过回溯将交易归集至创世 ${genesisTxid.slice(0, 16)}... (跨越 ${chainHops.length} 阶状态)，索引器后台已自动启动全生命周期追踪与追块！`,
    queriedTxid: cleanTxid,
    genesisTxid,
    cid: extractedCid,
    traceHops: chainHops.length
  };
}
