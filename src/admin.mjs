// Admin Router & Handlers for Kaspa Covenant Indexer (KCI)
import fs from 'node:fs';
import path from 'node:path';
import {ADMIN_HTML} from './admin-html.mjs';
import {categoryOf} from './api.mjs';
import {loadContracts} from './contracts.mjs';
import {compileSilverScript} from './compiler.mjs';

export function verifyAdminToken(req, expectedToken) {
  if (!expectedToken) return false;
  const authHeader = req.headers['authorization'];
  if (authHeader && /^Bearer\s+(\S+)$/i.test(authHeader)) {
    const token = authHeader.replace(/^Bearer\s+/i, '').trim();
    if (token === expectedToken) return true;
  }
  const cookies = req.headers['cookie'];
  if (cookies) {
    const match = cookies.match(/(?:^|;\s*)admin_token=([^;]+)/);
    if (match && decodeURIComponent(match[1]) === expectedToken) return true;
  }
  try {
    const url = new URL(req.url, 'http://localhost');
    if (url.searchParams.get('token') === expectedToken) return true;
  } catch {}
  return false;
}

export function readBody(req, limit = 1024 * 1024) {
  return new Promise((resolve, reject) => {
    let buf = '', len = 0;
    req.on('data', chunk => {
      len += chunk.length;
      if (len > limit) {
        req.destroy();
        reject(new Error('PAYLOAD_TOO_LARGE'));
        return;
      }
      buf += chunk;
    });
    req.on('end', () => {
      if (!buf.trim()) return resolve({});
      try { resolve(JSON.parse(buf)); }
      catch (e) { reject(new Error('INVALID_JSON')); }
    });
    req.on('error', reject);
  });
}

export async function handleAdmin(req, res, ctx) {
  const {store, engine, contracts, node, sdk, network = 'testnet-10', adminToken, contractsPath, openMember, log = () => {}} = ctx;
  const url = new URL(req.url, 'http://localhost');
  const pathname = url.pathname;

  const send = (status, body) => {
    res.writeHead(status, {
      'Content-Type': 'application/json; charset=utf-8',
      'Cache-Control': 'no-store',
      'X-Content-Type-Options': 'nosniff'
    });
    res.end(JSON.stringify(body, (_, v) => typeof v === 'bigint' ? v.toString() : v));
  };

  // Serve Dashboard HTML
  if (pathname === '/admin' || pathname === '/admin/' || pathname === '/admin/index.html') {
    res.writeHead(200, {
      'Content-Type': 'text/html; charset=utf-8',
      'Cache-Control': 'no-store'
    });
    res.end(ADMIN_HTML);
    return true;
  }

  // Admin API routes
  if (pathname.startsWith('/admin/api/')) {
    // Check Auth
    if (!verifyAdminToken(req, adminToken)) {
      send(401, {ok: false, error: 'UNAUTHORIZED', detail: 'Invalid or missing Admin Token'});
      return true;
    }

    const sub = pathname.replace('/admin/api', '');

    // 1. Status Overview
    if (sub === '/status' && req.method === 'GET') {
      const mem = process.memoryUsage();
      const allRounds = store.rounds();
      const liveRounds = store.rounds({liveOnly: true});
      const contractsSummary = {};
      for (const r of allRounds) {
        contractsSummary[r.contract] = (contractsSummary[r.contract] || 0) + 1;
      }
      const cp = store.checkpointsBefore(Date.now() + 1, 1)[0] ?? null;
      const nodeStatus = typeof node?.status === 'function' ? node.status() : [{url: node?.url ?? node?.identity?.url, up: true, active: true}];
      const activeNode = node?.identity ?? null;

      send(200, {
        ok: true,
        name: 'kaspa-covenant-indexer',
        version: '1.0.0',
        uptimeSec: Math.floor(process.uptime()),
        memory: {
          rssMb: +(mem.rss / 1024 / 1024).toFixed(1),
          heapUsedMb: +(mem.heapUsed / 1024 / 1024).toFixed(1),
          heapTotalMb: +(mem.heapTotal / 1024 / 1024).toFixed(1),
        },
        network,
        activeNode: {
          url: activeNode?.url ?? node?.url,
          p2pId: activeNode?.p2pId,
          serverVersion: activeNode?.serverVersion,
          isSynced: activeNode?.isSynced,
          hasUtxoIndex: activeNode?.hasUtxoIndex,
          virtualDaaScore: activeNode?.virtualDaaScore?.toString()
        },
        nodePool: nodeStatus,
        rounds: {
          total: allRounds.length,
          live: liveRounds.length,
          byContract: contractsSummary,
          byStatus: {
            open: allRounds.filter(r => categoryOf(r) === 'open').length,
            sealed: allRounds.filter(r => categoryOf(r) === 'sealed').length,
            close: allRounds.filter(r => categoryOf(r) === 'close').length,
            unknown: allRounds.filter(r => categoryOf(r) === 'unknown').length,
          }
        },
        checkpoint: {
          at: cp?.at ?? null,
          sink: cp?.sink ?? null,
          virtualDaa: cp?.virtualDaa?.toString() ?? null
        },
        contracts: contracts ? contracts.describe() : [],
        journal: store.journal(25)
      });
      return true;
    }

    // 2. Test Node Connectivity
    if (sub === '/test-node' && req.method === 'POST') {
      try {
        const body = await readBody(req);
        const testUrl = body?.url?.trim();
        if (!testUrl || !/^(wss?:\/\/)/i.test(testUrl)) {
          send(400, {ok: false, error: 'INVALID_URL', detail: 'URL must begin with ws:// or wss://'});
          return true;
        }

        const rpc = new sdk.RpcClient({url: testUrl, networkId: network, encoding: sdk.Encoding.Borsh});
        const startT = performance.now();
        await rpc.connect({blockAsyncConnect: true, strategy: sdk.ConnectStrategy.Fallback, timeoutDuration: 6000});
        const rttMs = Math.round(performance.now() - startT);
        const info = await rpc.getServerInfo();
        const dag = await rpc.getBlockDagInfo();
        await rpc.disconnect();

        send(200, {
          ok: true,
          rttMs,
          info: {
            serverVersion: info.serverVersion,
            networkId: info.networkId,
            isSynced: info.isSynced,
            hasUtxoIndex: info.hasUtxoIndex,
            virtualDaaScore: info.virtualDaaScore?.toString()
          },
          dag: {
            sink: dag.sink,
            virtualDaaScore: dag.virtualDaaScore?.toString(),
            blockCount: dag.blockCount,
            headerCount: dag.headerCount
          }
        });
      } catch (err) {
        send(200, {ok: false, error: err.message});
      }
      return true;
    }

    // 3. Get Plugins
    if (sub === '/plugins' && req.method === 'GET') {
      let rawConfig = [];
      if (contractsPath && fs.existsSync(contractsPath)) {
        try { rawConfig = JSON.parse(fs.readFileSync(contractsPath, 'utf8')).contracts || []; } catch {}
      }
      send(200, {
        ok: true,
        contractsPath,
        plugins: contracts ? contracts.describe() : [],
        config: rawConfig
      });
      return true;
    }

    // 4. Toggle Plugin Retired State
    if (sub === '/plugins/toggle' && req.method === 'POST') {
      try {
        const body = await readBody(req);
        const {index, retired} = body;
        if (!contractsPath || !fs.existsSync(contractsPath)) {
          send(500, {ok: false, error: 'CONTRACTS_FILE_NOT_FOUND'});
          return true;
        }

        const raw = JSON.parse(fs.readFileSync(contractsPath, 'utf8'));
        if (!Array.isArray(raw.contracts) || !raw.contracts[index]) {
          send(400, {ok: false, error: 'INVALID_PLUGIN_INDEX'});
          return true;
        }

        raw.contracts[index].retired = Boolean(retired);
        const tmp = contractsPath + '.tmp-' + Date.now();
        fs.writeFileSync(tmp, JSON.stringify(raw, null, 2) + '\n');
        fs.renameSync(tmp, contractsPath);

        // Hot reload contracts if possible
        if (engine && sdk) {
          const newContracts = await loadContracts(contractsPath, {sdk, network});
          engine.contracts = newContracts;
          log('PLUGINS_RELOADED', newContracts.describe());
        }

        send(200, {
          ok: true,
          message: 'Plugin status updated',
          plugins: engine?.contracts ? engine.contracts.describe() : []
        });
      } catch (err) {
        send(500, {ok: false, error: err.message});
      }
      return true;
    }

    // 5. Compile SilverScript Contract (.sil)
    if (sub === '/compile' && req.method === 'POST') {
      try {
        const body = await readBody(req);
        const {source, filename, constructorArgs, astOnly} = body;
        const resCompile = await compileSilverScript({
          source,
          filename,
          constructorArgs,
          astOnly: Boolean(astOnly)
        });
        send(resCompile.ok ? 200 : 400, resCompile);
      } catch (err) {
        send(500, {ok: false, error: 'COMPILER_INTERNAL_ERROR', detail: err.message});
      }
      return true;
    }

    send(404, {ok: false, error: 'NOT_FOUND'});
    return true;
  }

  return false;
}
