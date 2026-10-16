// Node pool selection for dual-node mode: probe candidate wRPC endpoints of ONE network, drop bad ones, and pick two
// members that are NOT the same backend (distinct p2pId). Measured 2026-09-30 (TN10): 7 public hostnames resolved by
// kaspa-resolver, but 5 of them report ONE p2pId, same peer count and same sink -> one node behind several names.
// A "standby" on the same backend fails together with the primary. Measured 2026-10-16 (mainnet): 3 distinct p2pIds.
import {networkConfig} from './networks.mjs';

/** Backwards-compatible TN10 seed export (kept for callers/tests that import it directly). */
export const TN10_SEED = networkConfig('tn10').seed;

/** Seed list for one network key. Fails closed on an unknown network. */
export function seedFor(network) {
  return networkConfig(network).seed;
}

/**
 * Probe all candidates in parallel. probe(url) -> {p2pId, blueScore(bigint), rttMs} or throws.
 * Returns ranked healthy candidates: [{url, p2pId, blueScore, rttMs, lag}], plus rejected ones with reasons.
 */
export async function probePool(urls, probe, {maxLag = 50n, deadlineMs = 12000} = {}) {
  const uniq = [...new Set(urls)];
  const settled = await Promise.all(uniq.map(url => {
    let t;
    return Promise.race([probe(url), new Promise((_, j) => { t = setTimeout(() => j(Error('PROBE_TIMEOUT')), deadlineMs); t.unref?.(); })])
      .then(r => ({url, ...r}), e => ({url, error: String(e?.message ?? e).slice(0, 80)}))
      .finally(() => clearTimeout(t));
  }));
  const ok = settled.filter(r => !r.error && r.p2pId && typeof r.blueScore === 'bigint');
  const top = ok.reduce((m, r) => r.blueScore > m ? r.blueScore : m, 0n);
  const rejected = settled.filter(r => r.error).map(r => ({url: r.url, reason: r.error}));
  const healthy = [];
  for (const r of ok) {
    const lag = top - r.blueScore;
    if (lag > maxLag) rejected.push({url: r.url, reason: 'LAGGING_' + lag});
    else healthy.push({url: r.url, p2pId: r.p2pId, blueScore: r.blueScore, rttMs: r.rttMs, lag});
  }
  healthy.sort((a, b) => a.rttMs - b.rttMs || (a.lag < b.lag ? -1 : a.lag > b.lag ? 1 : 0) || a.url.localeCompare(b.url));
  return {healthy, rejected};
}

/**
 * Choose [primary, standby] from ranked healthy candidates, never two on the same backend (p2pId).
 * `exclude` = p2pIds to avoid (e.g. the member that is still up when replacing the other one).
 */
export function choosePair(healthy, {exclude = []} = {}) {
  const out = [], seen = new Set(exclude);
  for (const c of healthy) {
    if (seen.has(c.p2pId)) continue;
    seen.add(c.p2pId); out.push(c);
    if (out.length === 2) break;
  }
  return out;
}

/** One candidate for replacing a member: best healthy one whose backend differs from every p2pId in `exclude`. */
export function chooseReplacement(healthy, exclude) {
  return choosePair(healthy, {exclude})[0] ?? null;
}

/** Ask kaspa-resolver services for node URLs of ONE network (each answer is one url). Best effort; failures ignored. */
export async function discoverFromResolvers(resolverHosts, {network = 'tn10', rounds = 3, timeoutMs = 4000, fetchImpl = fetch} = {}) {
  const net = networkConfig(network);
  const found = new Set();
  const ask = async host => {
    const ac = new AbortController(), t = setTimeout(() => ac.abort(), timeoutMs); t.unref?.();
    try {
      const r = await fetchImpl(`https://${host}/v2/kaspa/${net.resolverPath}/tls/wrpc/borsh`, {signal: ac.signal});
      if (!r.ok) return;
      const j = await r.json();
      // Accept any wRPC transport (borsh or json) whose path names THIS network; the client normalises to /wrpc/json.
      if (typeof j?.url === 'string' && /^wss:\/\/[a-z0-9.-]+\/kaspa\/[a-z0-9-]+\/wrpc\/(borsh|json)$/.test(j.url)) {
        const urlNet = j.url.match(/\/kaspa\/([a-z0-9-]+)\/wrpc\//)?.[1];
        if (urlNet === net.resolverPath) found.add(j.url);
      }
    } catch {} finally { clearTimeout(t); }
  };
  for (let i = 0; i < rounds; i++) await Promise.all(resolverHosts.map(ask));
  return [...found];
}

/** Resolver hosts from rusty-kaspa rpc/wrpc/client/Resolvers.toml (4 groups x 4, fetched 2026-09-30). */
export const RESOLVER_HOSTS = Object.freeze(['eric', 'maxim', 'sean', 'troy'].map(n => n + '.kaspa.stream')
  .concat(['john', 'mike', 'paul', 'alex'].map(n => n + '.kaspa.red'), ['jake', 'mark', 'adam', 'liam'].map(n => n + '.kaspa.green'),
    ['noah', 'ryan', 'jack', 'luke'].map(n => n + '.kaspa.blue')));
