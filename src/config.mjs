// Operator configuration only; does not change pinned contract rules.
import {networkConfig} from './networks.mjs';

/** Default REST hint oracle: the TN10 endpoint. indexer.mjs overrides this per network unless --rest-url is explicit. */
export const DEFAULT_REST_URL = networkConfig('tn10').rest;

export function runtimeOptions(cli, env = process.env) {
  const out = {...cli};
  const nodeKeys = ['url', 'urls', 'resolver', 'pool'];
  // An explicit CLI node mode overrides the entire environment node mode.
  if (!nodeKeys.some(k => cli[k])) {
    out.url = env.KASWIN_NODE_URL || undefined;
    out.urls = env.KASWIN_NODE_URLS || undefined;
    out.resolver = env.KASWIN_RESOLVER_URLS || undefined;
  }
  if (nodeKeys.filter(k => out[k]).length > 1) throw Error('USE_ONE_OF_URL_URLS_RESOLVER_POOL');
  const raw = cli['registry-addresses'] ?? env.KASWIN_REGISTRY_ADDRESSES;
  if (raw !== undefined) {
    const list = raw.split(',').map(x => x.trim());
    if (!list.length || list.length > 16 || list.some(x => !x.startsWith('kaspatest:') || /\s/.test(x)) || new Set(list).size !== list.length)
      throw Error('INVALID_REGISTRY_ADDRESSES');
    out.registryAddresses = list; // Actual address checksum/SPK validated by the pinned SDK at plugin creation.
  }
  const cors = cli['cors-origin'] ?? env.KASWIN_CORS_ORIGIN;
  if (cors !== undefined) out['cors-origin'] = cors;
  const adminToken = cli['admin-token'] ?? env.KASPA_INDEXER_ADMIN_TOKEN ?? env.ADMIN_TOKEN;
  if (adminToken !== undefined) out['admin-token'] = adminToken;
  const rest = cli['rest-url'] ?? env.KASWIN_REST_URL ?? env.KASPA_REST_URL;
  // restUrlExplicit distinguishes "operator chose a value (or disabled it)" from "use the selected network's own REST".
  out.restUrlExplicit = rest !== undefined;
  if (rest !== undefined) {
    if (rest === 'none' || rest === 'false' || rest === 'off') out.restUrl = null;
    else out.restUrl = rest;
  } else {
    out.restUrl = DEFAULT_REST_URL;
  }
  const networks = cli.networks ?? env.KASWIN_NETWORKS;
  if (networks !== undefined) out.networks = networks;
  return out;
}
