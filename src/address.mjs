// Pure JavaScript Kaspa Bech32 (CashAddr-style) address encoder & decoder.
// Zero WASM SDK dependencies. Supports P2PK (version 0) and P2SH (version 8) for Testnet 10 & Mainnet.

const CHARSET = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';
const REV = new Map([...CHARSET].map((c, i) => [c, i]));

function polymod(values) {
  let c = 1n;
  for (const d of values) {
    const c0 = c >> 35n;
    c = ((c & 0x07ffffffffn) << 5n) ^ BigInt(d);
    if (c0 & 0x01n) c ^= 0x98f2bc8e61n;
    if (c0 & 0x02n) c ^= 0x79b76d99e2n;
    if (c0 & 0x04n) c ^= 0xf33e5fb3c4n;
    if (c0 & 0x08n) c ^= 0xae2eabe2a8n;
    if (c0 & 0x10n) c ^= 0x1e4f43e470n;
  }
  return c ^ 1n;
}

const checksum = (payload5, prefix) =>
  polymod([...[...prefix].map(c => c.charCodeAt(0) & 0x1f), 0, ...payload5, 0, 0, 0, 0, 0, 0, 0, 0]);

function conv8to5(bytes) {
  const out = []; let buff = 0, bits = 0;
  for (const b of bytes) {
    buff = (buff << 8) | b; bits += 8;
    while (bits >= 5) { bits -= 5; out.push((buff >> bits) & 31); buff &= (1 << bits) - 1; }
  }
  if (bits > 0) out.push((buff << (5 - bits)) & 31);
  return out;
}

function conv5to8(five) {
  const out = []; let buff = 0, bits = 0;
  for (const c of five) {
    buff = (buff << 5) | c; bits += 5;
    while (bits >= 8) { bits -= 8; out.push((buff >> bits) & 255); buff &= (1 << bits) - 1; }
  }
  return out;
}

export function encodeAddress(version, payload, prefix = 'kaspatest') {
  const five = conv8to5([version, ...payload]), sum = checksum(five, prefix);
  const sumBytes = [];
  for (let i = 4; i >= 0; i--) sumBytes.push(Number((sum >> BigInt(i * 8)) & 0xffn));
  return prefix + ':' + [...five, ...conv8to5(sumBytes)].map(i => CHARSET[i]).join('');
}

export function decodeAddress(address, expectedPrefix = null) {
  if (typeof address !== 'string' || address.length > 120 || !address.includes(':')) {
    throw Error('INVALID_KASPA_ADDRESS');
  }
  const lower = address.toLowerCase();
  const [prefix, body] = lower.split(':');
  if (expectedPrefix && prefix !== expectedPrefix) throw Error(`PREFIX_MISMATCH: expected ${expectedPrefix}`);
  if (!['kaspa', 'kaspatest', 'kaspadev', 'kaspasim'].includes(prefix)) throw Error(`UNKNOWN_PREFIX: ${prefix}`);

  const five = [];
  for (const c of body) {
    const v = REV.get(c);
    if (v === undefined) throw Error(`INVALID_CHAR_IN_ADDRESS: ${c}`);
    five.push(v);
  }
  if (five.length < 8) throw Error('ADDRESS_TOO_SHORT');
  const payload5 = five.slice(0, -8), sum5 = five.slice(-8);
  let given = 0n;
  for (const b of conv5to8(sum5)) given = (given << 8n) | BigInt(b);
  if (checksum(payload5, prefix) !== given) throw Error('ADDRESS_CHECKSUM_ERROR');
  const bytes = conv5to8(payload5);
  const version = bytes[0];
  const payload = Uint8Array.from(bytes.slice(1));
  if ((version !== 0 && version !== 8) || payload.length !== 32) throw Error('UNSUPPORTED_ADDRESS_VERSION_OR_LENGTH');
  return {prefix, version, payload};
}

const hex = b => Array.from(b, x => x.toString(16).padStart(2, '0')).join('');
const unhex = h => Uint8Array.from(h.match(/../g) ?? [], x => parseInt(x, 16));

/** Address to standard scriptPublicKey ({version: 0, script: '...'}). */
export function addressToSpk(address, expectedPrefix = null) {
  const {version, payload} = decodeAddress(address, expectedPrefix);
  return version === 0
    ? {version: 0, script: '20' + hex(payload) + 'ac'}
    : {version: 0, script: 'aa20' + hex(payload) + '87'};
}

/** ScriptPublicKey to Kaspa address string (P2PK or P2SH). Returns null for non-standard scripts. */
export function spkToAddress(spk, prefix = 'kaspatest') {
  if (spk?.version !== 0 || typeof spk.script !== 'string') return null;
  const s = spk.script.toLowerCase();
  if (/^20[0-9a-f]{64}ac$/.test(s)) return encodeAddress(0, unhex(s.slice(2, 66)), prefix);
  if (/^aa20[0-9a-f]{64}87$/.test(s)) return encodeAddress(8, unhex(s.slice(4, 68)), prefix);
  return null;
}

/** Standard addressOf function matching the engine contract */
export function addressOf(spk, network = 'testnet-10') {
  const prefix = network === 'mainnet' ? 'kaspa' : 'kaspatest';
  const addr = spkToAddress(spk, prefix);
  if (!addr) throw Error('SPK_HAS_NO_ADDRESS');
  return addr;
}

export function addressOfFactory(_sdkIgnored, network = 'testnet-10') {
  return spk => addressOf(spk, network);
}
