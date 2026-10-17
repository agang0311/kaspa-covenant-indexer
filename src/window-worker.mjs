// Child-process chunk of the bounded window walk (see window-scan.mjs and Engine.windowChunk for the memory rationale).
// Receives one job over IPC, opens its own read-only native JSON wRPC connection to the given node URL, scans at most maxPages pages,
// replies once and exits; process exit returns all native memory to the OS. No keys, no submit.
import {JsonRpcClient} from './rpc.mjs';
import {scanWindow, spendsOutpoint} from './window-scan.mjs';
import {networkConfig} from './networks.mjs';

// Never outlive the parent: if the IPC channel closes (parent exited/killed), stop at once.
process.on('disconnect', () => process.exit(0));
process.once('message', async job => {
  let rpc, reply;
  try {
    const {url, network, tip, from, maxPages, budgetMs} = job;
    const fromBlue = BigInt(job.fromBlue), sinkBlue = BigInt(job.sinkBlue), windowBlue = BigInt(job.windowBlue), w = job.w ? BigInt(job.w) : null;
    let expectedId = network;
    try { const cfg = networkConfig(network); if (cfg?.networkId) expectedId = cfg.networkId; } catch {}
    rpc = new JsonRpcClient({url, networkId: expectedId, timeoutDuration: 8000});
    await rpc.connect({timeoutDuration: 8000});
    const info = await rpc.getServerInfo();
    if (info.networkId !== expectedId || info.isSynced !== true) {
      throw Error(`WORKER_NODE_NOT_USABLE: networkId=${info?.networkId} (expected ${expectedId}), isSynced=${info?.isSynced}`);
    }
    const r = await scanWindow(rpc, {match: t => spendsOutpoint(t, tip), from, fromBlue, sinkBlue, windowBlue, maxPages, w, until: Date.now() + budgetMs});
    reply = {ok: true, r: JSON.parse(JSON.stringify(r, (k, v) => typeof v === 'bigint' ? String(v) : v))};
  } catch (e) {
    reply = {ok: false, message: String(e?.message ?? e).slice(0, 200)};
  }
  try { await rpc?.disconnect(); } catch {}
  process.send(reply, () => process.exit(0));
});
