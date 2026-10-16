// Network catalog for the read-only indexer: one entry per monitored Kaspa network.
// Facts are PINNED constants (genesis/prefix/registry) taken from fixed sources; endpoints are operator-configurable
// hints and never a consensus claim. A network context is frozen at creation and NEVER falls back to another network.

/**
 * Fixed network facts.
 *  - genesis: rusty-kaspa genesis hash (same value the pinned contract plugins are guarded against).
 *  - addressPrefix: Bech32 human-readable prefix. NOTE (measured 2026-10-16): the prefix is presentation only; the
 *    same x-only key yields the SAME scriptPublicKey on both networks, so every address decode must pass the prefix
 *    explicitly. Never decode an address without the network's own prefix.
 *  - registry: the discovery marker address (an ordinary P2PK output, never a covenant authorization).
 */
export const NETWORKS = Object.freeze({
  tn10: Object.freeze({
    key: 'tn10',
    label: 'Testnet 10',
    networkId: 'testnet-10',
    genesis: 'f896a3034873be1739fc4359236899fd3d65d2bc94f9780df0d0da3eb1cc4370',
    addressPrefix: 'kaspatest',
    rest: 'https://api-tn10.kaspa.org',
    registry: 'kaspatest:qztrjpfpuf6g9jw76e6ay909z43hndpv5maqkker6ur4enz7llarsh82ezggl',
    seed: Object.freeze([
      'wss://muon-10.kaspa.blue/kaspa/testnet-10/wrpc/json',
      'wss://vector-10.kaspa.green/kaspa/testnet-10/wrpc/json',
      'wss://quark-10.kaspa.red/kaspa/testnet-10/wrpc/json',
      'wss://electron-10.kaspa.stream/kaspa/testnet-10/wrpc/json',
      'wss://neutrino-10.kaspa.stream/kaspa/testnet-10/wrpc/json',
      'wss://electron-10.kaspa.blue/kaspa/testnet-10/wrpc/json',
      'wss://proton-10.kaspa.stream/kaspa/testnet-10/wrpc/json',
    ]),
    resolverPath: 'testnet-10',
  }),
  mainnet: Object.freeze({
    key: 'mainnet',
    label: 'Kaspa mainnet',
    networkId: 'mainnet',
    genesis: '58c2d4199e21f910d1571d114969cecef48f09f934d42ccb6a281a15868f2999',
    addressPrefix: 'kaspa',
    rest: 'https://api.kaspa.org',
    registry: 'kaspa:qzccgw790r0mnfe9uvy7nwd7a8990029ycg8e0manehhgruqm2r9v528nz0qr',
    seed: Object.freeze([
      'wss://kate.kaspa.red/kaspa/mainnet/wrpc/json',
      'wss://john.kaspa.red/kaspa/mainnet/wrpc/json',
      'wss://eva.kaspa.green/kaspa/mainnet/wrpc/json',
      'wss://ivy.kaspa.green/kaspa/mainnet/wrpc/json',
      'wss://dina.kaspa.green/kaspa/mainnet/wrpc/json',
    ]),
    resolverPath: 'mainnet',
  }),
});

export const NETWORK_KEYS = Object.freeze(Object.keys(NETWORKS));
export const DEFAULT_NETWORK = 'tn10';

/** Resolve a network key ('tn10' | 'mainnet') or a full networkId ('testnet-10' | 'mainnet'). Fails closed. */
export function networkConfig(key) {
  if (typeof key !== 'string' || !key) throw Error('NETWORK_REQUIRED');
  if (Object.hasOwn(NETWORKS, key)) return NETWORKS[key];
  const byId = NETWORK_KEYS.find(k => NETWORKS[k].networkId === key);
  if (byId) return NETWORKS[byId];
  throw Error('UNKNOWN_NETWORK ' + key);
}

export function networkOfGenesis(genesis) {
  return NETWORK_KEYS.find(k => NETWORKS[k].genesis === genesis) ?? null;
}

/** Parse a comma-separated --networks value. Rejects unknown, empty and duplicate entries. Fails closed. */
export function parseNetworks(raw, fallback = DEFAULT_NETWORK) {
  const list = (raw === undefined || raw === null || raw === '' ? fallback : raw)
    .split(',').map(s => s.trim()).filter(Boolean);
  if (!list.length) throw Error('NETWORKS_EMPTY');
  const nets = list.map(networkConfig);
  if (new Set(nets.map(n => n.key)).size !== nets.length) throw Error('NETWORKS_DUPLICATE');
  return nets;
}
