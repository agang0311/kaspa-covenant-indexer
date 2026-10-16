// Native JSON wRPC Node Interface (Zero WASM SDK dependencies).
import {JsonRpcClient} from './rpc.mjs';
import {addressOfFactory, addressToSpk, spkToAddress} from './address.mjs';

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

  const rpc = new JsonRpcClient({url, networkId: network, timeoutDuration: timeoutMs});
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
    identity = await verifyNode(rpc, network);
  } catch (e) {
    connecting.catch(() => {});
    try { await rpc.disconnect(); } catch {}
    throw e;
  }
  log('CONNECTED', identity);

  const node = {
    rpc,
    identity,
    url: rpc.url ?? null,
    on(kind, fn) { handlers[kind].push(fn); },
    verify: async () => (node.identity = await verifyNode(rpc, network)),
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

async function verifyNode(rpc, network) {
  const info = await rpc.getServerInfo();
  if (info.networkId !== network) throw Error('WRONG_NETWORK_' + info.networkId);
  if (info.isSynced !== true) throw Error('NODE_NOT_SYNCED');
  if (info.hasUtxoIndex !== true) throw Error('NODE_WITHOUT_UTXOINDEX');
  let p2pId = null;
  try { p2pId = (await rpc.getInfo()).p2pId; } catch {}
  return {
    url: rpc.url ?? null,
    nodeId: rpc.nodeId ?? null,
    p2pId,
    serverVersion: info.serverVersion,
    networkId: info.networkId,
    isSynced: true,
    hasUtxoIndex: true,
    virtualDaaScore: info.virtualDaaScore ? info.virtualDaaScore.toString() : null,
    checkedAt: new Date().toISOString()
  };
}
