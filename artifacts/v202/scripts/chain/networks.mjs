// Public network profiles and identity checks, extracted unchanged from the V2.0.1 CLI.
// Fixed rusty-kaspa cfafeb4 genesis/params/address sources; endpoint observations dated 2026-10-12.
// Configuration availability does NOT enable a Web network or grant permission to sign/broadcast.
// CLI filesystem locations, wallet names and approval schemas are deliberately excluded.
const freeze = o => Object.freeze(Object.fromEntries(Object.entries(o).map(([k, v]) => [k, v && typeof v === 'object' ? Object.freeze(v) : v])));
const fail = code => { const e = new Error(code); e.safeCode = code; throw e; };

export const NETWORKS = Object.freeze({
  tn10: freeze({
    key: 'tn10', label: 'Testnet 10', unit: 'TKAS',
    networkId: 'testnet-10',
    genesis: 'f896a3034873be1739fc4359236899fd3d65d2bc94f9780df0d0da3eb1cc4370',
    addressPrefix: 'kaspatest',
    nodes: ['wss://tn10.kaspay.top/wrpc', 'wss://muon-10.kaspa.blue/kaspa/testnet-10/wrpc/json', 'wss://quark-10.kaspa.red/kaspa/testnet-10/wrpc/json',
      'wss://vector-10.kaspa.green/kaspa/testnet-10/wrpc/json', 'wss://electron-10.kaspa.stream/kaspa/testnet-10/wrpc/json'],
    indexer: 'https://cd311.cn:888/indexer',
    registry: {address: 'kaspatest:qztrjpfpuf6g9jw76e6ay909z43hndpv5maqkker6ur4enz7llarsh82ezggl',
      spk: {version: 0, script: '2096390521e27482c9ded675d215e5156379b42ca6fa0b5b23d7075ccc5efffa38ac'}},
    maxFeePerTx: 50_000_000n,    // 0.5 TKAS per transaction (project cap)
  }),
  mainnet: freeze({
    key: 'mainnet', label: 'Kaspa mainnet', unit: 'KAS',
    networkId: 'mainnet',
    genesis: '58c2d4199e21f910d1571d114969cecef48f09f934d42ccb6a281a15868f2999',
    addressPrefix: 'kaspa',
    // Public mainnet wRPC JSON endpoints observed reachable (2.1.0, synced, UTXO index) on 2026-10-12.
    nodes: ['wss://kate.kaspa.red/kaspa/mainnet/wrpc/json', 'wss://john.kaspa.red/kaspa/mainnet/wrpc/json'],
    indexer: null,               // no mainnet indexer deployed: pass --indexer=URL explicitly, or --no-indexer
    registry: {address: 'kaspa:qzccgw790r0mnfe9uvy7nwd7a8990029ycg8e0manehhgruqm2r9v528nz0qr',
      spk: {version: 0, script: '20b1843bc578dfb9a725e309e9b9bee9ca57bd4526107cbf7d9e6f740f80da8656ac'}},
    // 0.5 KAS per transaction, same as TN10 (largest V2.0.1 TN10 fee observed: 0.05426 TKAS, R256 REFUND). A lower cap could
    // stop a suite between steps and leave live rounds; total spend is bounded by the approval's maxTotalFeeSompi.
    maxFeePerTx: 50_000_000n,
  }),
});
export const NETWORK_KEYS = Object.freeze(Object.keys(NETWORKS));
export const DEFAULT_NETWORK = 'tn10';
export function networkConfig(key = DEFAULT_NETWORK) {
  if (!Object.hasOwn(NETWORKS, key)) fail('UNKNOWN_NETWORK');
  return NETWORKS[key];
}
/** Which network does a stored record belong to (by its genesis)? null if neither. */
export const networkOfGenesis = g => NETWORK_KEYS.find(k => NETWORKS[k].genesis === g) ?? null;
/** Read-only node identity check from two independent RPC answers: getServerInfo.networkId (synced, UTXO index) and
 * getBlockDagInfo.network must both name this network. Throws NODE_NETWORK_MISMATCH. */
export {assertNodeNetwork} from './node-identity.mjs';
