// Admin Router & Handlers for Kaspa Covenant Indexer (KCI)
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {ADMIN_HTML} from './admin-html.mjs';
import {categoryOf} from './api.mjs';
import {loadContracts} from './contracts.mjs';
import {compileSilverScript} from './compiler.mjs';
import {generatePluginCode} from './generator.mjs';
import {TN10_SEED} from './node-pool.mjs';

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
          isSynced: activeNode?.isSynced ?? true,
          hasUtxoIndex: activeNode?.hasUtxoIndex ?? true,
          virtualDaaScore: activeNode?.virtualDaaScore?.toString()
        },
        rest: {
          url: engine?.config?.restUrl ?? null,
          enabled: Boolean(engine?.config?.restUrl)
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
        contracts: (engine?.contracts || contracts)?.describe() || [],
        journal: store.journal(25)
      });
      return true;
    }

    // Node Pool & Candidates API
    if (sub === '/node-pool' && req.method === 'GET') {
      const members = typeof node?.status === 'function' ? node.status() : [{url: node?.url ?? node?.identity?.url, up: true, active: true}];
      send(200, {
        ok: true,
        seeds: TN10_SEED,
        members,
        active: node?.identity ?? null,
        rest: {
          url: engine?.config?.restUrl ?? null,
          enabled: Boolean(engine?.config?.restUrl)
        }
      });
      return true;
    }

    // Probe REST API endpoint
    if (sub === '/probe-rest' && req.method === 'POST') {
      const restUrl = engine?.config?.restUrl;
      if (!restUrl) {
        send(200, {ok: false, enabled: false, message: 'REST acceleration is disabled'});
        return true;
      }
      const t0 = performance.now();
      try {
        const ac = new AbortController(), t = setTimeout(() => ac.abort(), 4000); t.unref?.();
        const res = await fetch(`${restUrl.replace(/\/+$/, '')}/info/kaspad`, {signal: ac.signal});
        clearTimeout(t);
        const latencyMs = Math.round(performance.now() - t0);
        if (!res.ok) {
          send(200, {ok: false, enabled: true, restUrl, error: `HTTP ${res.status}`, latencyMs});
          return true;
        }
        const info = await res.json();
        send(200, {
          ok: true,
          enabled: true,
          restUrl,
          latencyMs,
          serverVersion: info.serverVersion,
          isSynced: info.isSynced,
          isUtxoIndexed: info.isUtxoIndexed,
          mempoolSize: info.mempoolSize,
          p2pIdHashed: info.p2pIdHashed
        });
      } catch (e) {
        send(200, {ok: false, enabled: true, restUrl, error: String(e?.message ?? e).slice(0, 100), latencyMs: Math.round(performance.now() - t0)});
      }
      return true;
    }

    if (sub === '/probe-pool' && req.method === 'POST') {
      try {
        const body = await readBody(req).catch(() => ({}));
        const customCandidates = Array.isArray(body?.urls) ? body.urls : [];
        const targets = [...new Set([
          ...customCandidates,
          ...TN10_SEED,
          ...(typeof node?.status === 'function' ? node.status().map(m => m.url) : [node?.url]).filter(Boolean)
        ])].filter(u => typeof u === 'string' && /^wss?:\/\//i.test(u));

        const probeSingleTarget = async (url) => {
          let rpc = null;
          try {
            rpc = new sdk.RpcClient({url, networkId: network, encoding: sdk.Encoding.Borsh});
            const t0 = performance.now();
            await rpc.connect({blockAsyncConnect: true, strategy: sdk.ConnectStrategy.Fallback, timeoutDuration: 3500});
            const rttMs = Math.round(performance.now() - t0);
            const info = await rpc.getServerInfo();
            let p2pId = null; try { p2pId = (await rpc.getInfo()).p2pId; } catch {}
            const dag = await rpc.getBlockDagInfo().catch(() => null);
            await rpc.disconnect();
            return {
              url,
              up: true,
              rttMs,
              p2pId,
              serverVersion: info.serverVersion || null,
              isSynced: Boolean(info.isSynced),
              hasUtxoIndex: Boolean(info.hasUtxoIndex),
              virtualDaaScore: info.virtualDaaScore ? info.virtualDaaScore.toString() : null,
              sink: dag?.sink || null
            };
          } catch (err) {
            if (rpc) { try { await rpc.disconnect(); } catch {} }
            return {
              url,
              up: false,
              rttMs: null,
              error: (err?.message || 'CONNECT_FAILED').slice(0, 100)
            };
          }
        };

        const probeWithDeadline = (url) => {
          let timer;
          const deadline = new Promise(resolve => {
            timer = setTimeout(() => resolve({url, up: false, rttMs: null, error: 'PROBE_TIMEOUT'}), 4500);
            timer.unref?.();
          });
          return Promise.race([probeSingleTarget(url), deadline]).finally(() => clearTimeout(timer));
        };

        const results = await Promise.all(targets.map(probeWithDeadline));

        let maxDaa = 0n;
        for (const r of results) {
          if (r.up && r.virtualDaaScore) {
            try {
              const v = BigInt(r.virtualDaaScore);
              if (v > maxDaa) maxDaa = v;
            } catch {}
          }
        }

        const p2pCount = {};
        for (const r of results) {
          if (r.up && r.p2pId) p2pCount[r.p2pId] = (p2pCount[r.p2pId] || 0) + 1;
        }

        for (const r of results) {
          if (r.up) {
            r.sharedBackend = r.p2pId ? (p2pCount[r.p2pId] > 1) : false;
            if (r.virtualDaaScore) {
              try {
                const diff = maxDaa - BigInt(r.virtualDaaScore);
                r.lag = diff > 0n ? diff.toString() : '0';
              } catch {
                r.lag = '0';
              }
            } else {
              r.lag = '0';
            }
          }
        }

        // Sort: online first, then lowest RTT
        results.sort((a, b) => {
          if (a.up !== b.up) return a.up ? -1 : 1;
          if (a.up && b.up) return (a.rttMs || 9999) - (b.rttMs || 9999);
          return a.url.localeCompare(b.url);
        });

        send(200, {
          ok: true,
          candidates: results,
          maxDaa: maxDaa.toString()
        });
      } catch (err) {
        send(500, {ok: false, error: err.message});
      }
      return true;
    }

    // 2. Test Node Connectivity
    if (sub === '/test-node' && req.method === 'POST') {
      let rpc = null, timer = null;
      try {
        const body = await readBody(req);
        const testUrl = body?.url?.trim();
        if (!testUrl || !/^(wss?:\/\/)/i.test(testUrl)) {
          send(400, {ok: false, error: 'INVALID_URL', detail: 'URL must begin with ws:// or wss://'});
          return true;
        }

        rpc = new sdk.RpcClient({url: testUrl, networkId: network, encoding: sdk.Encoding.Borsh});
        const timeoutPromise = new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error('CONNECT_TIMEOUT (5s)')), 5000);
          timer.unref?.();
        });

        const connectTask = async () => {
          const startT = performance.now();
          await rpc.connect({blockAsyncConnect: true, strategy: sdk.ConnectStrategy.Fallback, timeoutDuration: 4000});
          const rttMs = Math.round(performance.now() - startT);
          const info = await rpc.getServerInfo();
          let p2pId = null; try { p2pId = (await rpc.getInfo()).p2pId; } catch {}
          const dag = await rpc.getBlockDagInfo().catch(() => null);
          return {rttMs, info, p2pId, dag};
        };

        const {rttMs, info, p2pId, dag} = await Promise.race([connectTask(), timeoutPromise]);
        clearTimeout(timer);
        await rpc.disconnect();

        send(200, {
          ok: true,
          rttMs,
          info: {
            p2pId,
            serverVersion: info.serverVersion,
            networkId: info.networkId,
            isSynced: info.isSynced,
            hasUtxoIndex: info.hasUtxoIndex,
            virtualDaaScore: info.virtualDaaScore?.toString()
          },
          dag: {
            sink: dag?.sink,
            virtualDaaScore: dag?.virtualDaaScore?.toString(),
            blockCount: dag?.blockCount,
            headerCount: dag?.headerCount
          }
        });
      } catch (err) {
        if (timer) clearTimeout(timer);
        if (rpc) { try { await rpc.disconnect(); } catch {} }
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
        plugins: (engine?.contracts || contracts)?.describe() || [],
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

    // 4b. Update Plugin Configuration Options (Genesis Guards, Registry, etc.)
    if (sub === '/plugins/update-config' && req.method === 'POST') {
      try {
        const body = await readBody(req);
        const {index, options, retired} = body;
        if (!contractsPath || !fs.existsSync(contractsPath)) {
          send(500, {ok: false, error: 'CONTRACTS_FILE_NOT_FOUND'});
          return true;
        }

        const raw = JSON.parse(fs.readFileSync(contractsPath, 'utf8'));
        if (!Array.isArray(raw.contracts) || !raw.contracts[index]) {
          send(400, {ok: false, error: 'INVALID_PLUGIN_INDEX'});
          return true;
        }

        if (retired !== undefined) raw.contracts[index].retired = Boolean(retired);
        if (options && typeof options === 'object') {
          raw.contracts[index].options = {
            ...(raw.contracts[index].options || {}),
            ...options
          };
        }

        const tmp = contractsPath + '.tmp-' + Date.now();
        fs.writeFileSync(tmp, JSON.stringify(raw, null, 2) + '\n');
        fs.renameSync(tmp, contractsPath);

        // Hot reload contracts if possible
        if (engine && sdk) {
          const newContracts = await loadContracts(contractsPath, {sdk, network});
          engine.contracts = newContracts;
          for (const a of newContracts.watchAddresses()) {
            try { await engine.subscribe(a); } catch {}
          }
          log('PLUGIN_CONFIG_UPDATED', {
            index,
            plugin: raw.contracts[index].module,
            watchAddresses: newContracts.watchAddresses()
          });
        }

        send(200, {
          ok: true,
          message: 'Plugin configuration updated and reloaded',
          plugins: engine?.contracts ? engine.contracts.describe() : [],
          config: raw.contracts
        });
      } catch (err) {
        send(500, {ok: false, error: err.message});
      }
      return true;
    }

    // 4c. Remove / Uninstall Plugin
    if (sub === '/plugins/remove' && req.method === 'POST') {
      try {
        const body = await readBody(req);
        const {index} = body;
        if (!contractsPath || !fs.existsSync(contractsPath)) {
          send(500, {ok: false, error: 'CONTRACTS_FILE_NOT_FOUND'});
          return true;
        }

        const raw = JSON.parse(fs.readFileSync(contractsPath, 'utf8'));
        if (!Array.isArray(raw.contracts) || !raw.contracts[index]) {
          send(400, {ok: false, error: 'INVALID_PLUGIN_INDEX'});
          return true;
        }

        const removedEntry = raw.contracts.splice(index, 1)[0];
        const tmp = contractsPath + '.tmp-' + Date.now();
        fs.writeFileSync(tmp, JSON.stringify(raw, null, 2) + '\n');
        fs.renameSync(tmp, contractsPath);

        // Delete plugin file if it was a custom file and not builtin
        const contractsDir = path.dirname(path.resolve(contractsPath));
        const modFile = path.join(contractsDir, removedEntry.module);
        try {
          if (fs.existsSync(modFile) && removedEntry.module.endsWith('.mjs')) {
            fs.unlinkSync(modFile);
          }
        } catch {}

        // Hot reload contracts into running engine
        if (engine && sdk) {
          const newContracts = await loadContracts(contractsPath, {sdk, network});
          engine.contracts = newContracts;
          log('PLUGIN_REMOVED_AND_RELOADED', {
            removed: removedEntry.module,
            remaining: newContracts.describe()
          });
        }

        send(200, {
          ok: true,
          message: 'Plugin successfully removed and unloaded',
          plugins: engine?.contracts ? engine.contracts.describe() : [],
          config: raw.contracts
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

    // 6. Generate Plugin Code from Artifact
    if (sub === '/generate-plugin' && req.method === 'POST') {
      try {
        const body = await readBody(req);
        const {
          artifact,
          contractName,
          pluginId,
          version,
          registryAddresses,
          registrationSompi,
          requiredDeposit,
          initialStateGuards,
          allowedCreators
        } = body;
        const resGen = generatePluginCode({
          artifact,
          contractName,
          pluginId,
          version,
          registryAddresses,
          registrationSompi,
          requiredDeposit,
          initialStateGuards,
          allowedCreators
        });
        send(200, resGen);
      } catch (err) {
        send(400, {ok: false, error: 'PLUGIN_GENERATION_FAILED', detail: err.message});
      }
      return true;
    }

    // 7. Install / Hot-Load Generated Plugin
    if (sub === '/install-plugin' && req.method === 'POST') {
      try {
        const body = await readBody(req);
        const {
          filename,
          code,
          pluginId,
          version,
          templateHash,
          registryAddresses = [],
          registrationSompi = '5000000',
          requiredDeposit = null,
          initialStateGuards = null,
          allowedCreators = [],
          activate = true
        } = body;

        if (!filename || !/^[a-z0-9][a-z0-9._-]{0,63}\.mjs$/i.test(filename)) {
          send(400, {ok: false, error: 'INVALID_FILENAME', detail: 'Filename must match /^[a-z0-9._-]+\\.mjs$/'});
          return true;
        }
        if (!code || typeof code !== 'string' || code.length < 50) {
          send(400, {ok: false, error: 'INVALID_CODE', detail: 'Code payload cannot be empty'});
          return true;
        }
        if (!contractsPath || !fs.existsSync(contractsPath)) {
          send(500, {ok: false, error: 'CONTRACTS_CONFIG_NOT_FOUND'});
          return true;
        }

        const contractsDir = path.dirname(path.resolve(contractsPath));
        const targetFilePath = path.join(contractsDir, filename);

        // Write plugin file safely
        const tmpFile = targetFilePath + '.tmp-' + Date.now();
        fs.writeFileSync(tmpFile, code, 'utf8');
        fs.renameSync(tmpFile, targetFilePath);

        const sha256 = crypto.createHash('sha256').update(code).digest('hex');

        // Update contracts config JSON
        const rawConfig = JSON.parse(fs.readFileSync(contractsPath, 'utf8'));
        if (!Array.isArray(rawConfig.contracts)) rawConfig.contracts = [];

        // Check if module already in list
        const existingIdx = rawConfig.contracts.findIndex(c => c.module === filename || c.module === `./${filename}`);
        const newEntry = {
          module: filename,
          sha256,
          retired: !activate,
          options: {
            id: pluginId || filename.replace(/\.mjs$/, ''),
            version: version || '1.0.0',
            templateHash: templateHash || null,
            registryAddresses: Array.isArray(registryAddresses) ? registryAddresses : [],
            registrationSompi: registrationSompi || '0',
            requiredDeposit: requiredDeposit || null,
            initialStateGuards: initialStateGuards || null,
            allowedCreators: Array.isArray(allowedCreators) ? allowedCreators : []
          }
        };

        if (existingIdx >= 0) {
          rawConfig.contracts[existingIdx] = newEntry;
        } else {
          if (activate) rawConfig.contracts.unshift(newEntry);
          else rawConfig.contracts.push(newEntry);
        }

        const tmpConfig = contractsPath + '.tmp-' + Date.now();
        fs.writeFileSync(tmpConfig, JSON.stringify(rawConfig, null, 2) + '\n', 'utf8');
        fs.renameSync(tmpConfig, contractsPath);

        // Hot reload contracts into running engine and subscribe to new registry addresses
        if (engine && sdk) {
          const newContracts = await loadContracts(contractsPath, {sdk, network});
          engine.contracts = newContracts;
          for (const a of newContracts.watchAddresses()) {
            try { await engine.subscribe(a); } catch {}
          }
          log('PLUGIN_INSTALLED_AND_RELOADED', {
            filename,
            sha256,
            active: activate,
            watchAddresses: newContracts.watchAddresses(),
            plugins: newContracts.describe()
          });
        }

        send(200, {
          ok: true,
          message: 'Plugin successfully installed and loaded into indexer',
          filename,
          sha256,
          plugins: engine?.contracts ? engine.contracts.describe() : []
        });
      } catch (err) {
        send(500, {ok: false, error: 'PLUGIN_INSTALL_FAILED', detail: err.message});
      }
      return true;
    }

    // 8. Get Preset Contract Source (.sil)
    if (sub === '/preset-contract' && req.method === 'GET') {
      const name = url.searchParams.get('name');
      const presetsDir = path.join(path.dirname(path.resolve(contractsPath || '')), 'presets');
      const map = {
        'v201-open': {sil: 'open-linked.sil', args: 'open-linked.args.json', title: 'KaswinOpen (V2.0.1)'},
        'v201-sealed': {sil: 'sealed-linked.sil', args: 'sealed-linked.args.json', title: 'KaswinSealed (V2.0.1)'},
        'v201-refunding': {sil: 'refunding-linked.sil', args: 'refunding-linked.args.json', title: 'KaswinRefunding (V2.0.1)'}
      };

      const found = map[name];
      if (!found) {
        send(404, {ok: false, error: 'PRESET_NOT_FOUND'});
        return true;
      }

      const silPath = path.join(presetsDir, found.sil);
      const argsPath = path.join(presetsDir, found.args);

      if (!fs.existsSync(silPath)) {
        send(404, {ok: false, error: 'PRESET_FILE_NOT_FOUND', path: silPath});
        return true;
      }

      const source = fs.readFileSync(silPath, 'utf8');
      let constructorArgs = [];
      try {
        constructorArgs = JSON.parse(fs.readFileSync(argsPath, 'utf8'));
      } catch {}

      send(200, {
        ok: true,
        name: found.title,
        filename: found.sil,
        source,
        constructorArgs
      });
      return true;
    }

    send(404, {ok: false, error: 'NOT_FOUND'});
    return true;
  }

  return false;
}
