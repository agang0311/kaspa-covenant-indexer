// Native JSON wRPC Node Interface (Zero WASM SDK dependencies).
import {JsonRpcClient} from './rpc.mjs';
import {addressOfFactory, addressToSpk, spkToAddress} from './address.mjs';
import {networkConfig} from './networks.mjs';

export {addressOfFactory, addressToSpk, spkToAddress};

export async function openNode(sdkOrOpts, maybeOpts = {}) {
  const opts = (sdkOrOpts && typeof sdkOrOpts === 'object' && !sdkOrOpts.RpcClient) ? sdkOrOpts : maybeOpts;
  const {
    network = 'testnet-10',
    url = null,
    timeoutMs = 15000,
    log = () => {},
    connectDeadlineMs = null
  } = opts;
  const net = networkConfig(network);

  const rpc = new JsonRpcClient({url, networkId: net.networkId, timeoutDuration: timeoutMs});
  const handlers = {connect: [], disconnect: [], utxos: [], chain: []};
  rpc.addEventListener('connect', () => handlers.connect.forEach(f => f()));
  rpc.addEventListener('disconnect', () => handlers.disconnect.forEach(f => f()));
  rpc.addEventListener('utxos-changed', e => handlers.utxos.forEach(f => f(e.data)));
  rpc.addEventListener('virtual-chain-changed', e => {
    const removed = e?.data?.removedChainBlockHashes;
    if (Array.isArray(removed) && removed.length && removed.length <= 10_000) {
      const r = removed.map(String);
      handlers.chain.forEach(f => f(r));
    }
  });

  const connecting = rpc.connect({timeoutDuration: timeoutMs});
  let identity;
  try {
    if (connectDeadlineMs) {
      let timer;
      await Promise.race([
        connecting,
        new Promise((_, reject) => { timer = setTimeout(() => reject(Error('CONNECT_TIMEOUT')), connectDeadlineMs); })
      ]).finally(() => clearTimeout(timer));
    } else {
      await connecting;
    }
    identity = await verifyNode(rpc, net);
  } catch (e) {
    connecting.catch(() => {});
    try { await rpc.disconnect(); } catch {}
    throw e;
  }
  log('CONNECTED', identity);

  const node = {
    rpc,
    identity,
    network: net,
    url: rpc.url ?? null,
    on(kind, fn) { handlers[kind].push(fn); },
    verify: async () => (node.identity = await verifyNode(rpc, net)),
    subscribeUtxosChanged: addrs => rpc.subscribeUtxosChanged(addrs),
    unsubscribeUtxosChanged: addrs => rpc.unsubscribeUtxosChanged(addrs),
    subscribeVirtualChainChanged: () => rpc.subscribeVirtualChainChanged(false),
    getBlockDagInfo: () => rpc.getBlockDagInfo(),
    getSinkBlueScore: () => rpc.getSinkBlueScore(),
    getBlock: r => rpc.getBlock(r),
    getBlocks: r => rpc.getBlocks(r),
    getMempoolEntriesByAddresses: r => rpc.getMempoolEntriesByAddresses(r),
    getUtxosByAddresses: r => rpc.getUtxosByAddresses(r),
    getVirtualChainFromBlock: r => rpc.getVirtualChainFromBlock(r),
    getVirtualChainFromBlockV2: r => rpc.getVirtualChainFromBlockV2(r),
    close: () => rpc.disconnect(),
  };
  return node;
}

/**
 * Fail-closed node identity check. All three facts must name the SAME network:
 *  1. getServerInfo.networkId, 2. getBlockDagInfo.network, 3. the network table's pinned genesis.
 * A node that answers on the wrong network must never be used: the same x-only key yields the same SPK on both
 * networks (measured 2026-10-16), so a mixed pool would otherwise look like valid data.
 */
async function verifyNode(rpc, net) {
  const info = await rpc.getServerInfo();
  if (info.networkId !== net.networkId) throw Error('WRONG_NETWORK_' + info.networkId);
  if (info.isSynced !== true) throw Error('NODE_NOT_SYNCED');
  if (info.hasUtxoIndex !== true) throw Error('NODE_WITHOUT_UTXOINDEX');
  // getBlockDagInfo carries a `network` field on kaspad; when present it must agree as well.
  try {
    const dag = await rpc.getBlockDagInfo();
    if (dag?.network != null && String(dag.network) !== net.networkId) throw Error('WRONG_NETWORK_DAG_' + dag.network);
  } catch (e) {
    if (/^WRONG_NETWORK_DAG_/.test(String(e?.message ?? ''))) throw e;
  }
  let p2pId = null;
  try { p2pId = (await rpc.getInfo()).p2pId; } catch {}
  return {
    network: net.key,
    networkId: net.networkId,
    genesis: net.genesis,
    url: rpc.url ?? null,
    nodeId: rpc.nodeId ?? null,
    p2pId,
    serverVersion: info.serverVersion,
    isSynced: true,
    hasUtxoIndex: true,
    virtualDaaScore: info.virtualDaaScore ? info.virtualDaaScore.toString() : null,
    checkedAt: new Date().toISOString()
  };
}
