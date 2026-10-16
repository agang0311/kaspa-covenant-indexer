// F3.1 registration is an optional ordinary Genesis output, never a covenant authorization.
// The registry address and its address prefix are NETWORK facts: they are injected, never assumed.
import {addressToSpk, spkToAddress} from '../address.mjs';
import {networkConfig} from '../networks.mjs';

/** Legacy default (TN10). Kept for existing callers/tests; new callers should pass an explicit network. */
export const DEFAULT_REGISTRY_ADDRESS = networkConfig('tn10').registry;
export const REGISTRATION_SOMPI = '5000000';

/**
 * Build the registry scope for ONE network. `network` may be a key ('tn10'|'mainnet') or a networkId.
 * Addresses MUST carry that network's prefix: a prefix mismatch is rejected, because the prefix is the only
 * presentation-level signal separating otherwise identical scriptPublicKeys (measured 2026-10-16).
 */
export function registryScope(_sdk, addresses, network = 'tn10') {
  const net = networkConfig(network);
  if (!Array.isArray(addresses) || addresses.length < 1 || addresses.length > 16 || new Set(addresses).size !== addresses.length)
    throw Error('REGISTRY_ADDRESSES');
  return addresses.map(address => {
    if (typeof address !== 'string' || !address.startsWith(net.addressPrefix + ':') || address.length > 120)
      throw Error('REGISTRY_ADDRESS_PREFIX ' + net.addressPrefix);
    const spk = addressToSpk(address, net.addressPrefix);
    if (spkToAddress(spk, net.addressPrefix) !== address) throw Error('REGISTRY_ADDRESS_ROUNDTRIP');
    return {address, spk, network: net.key};
  });
}

const wireSpk = p => typeof p === 'string' ? p : /^[0-9a-f]*$/.test(p?.script ?? '') && Number.isInteger(p?.version) && p.version >= 0 && p.version <= 65535 ? p.version.toString(16).padStart(4, '0').match(/../g).reverse().join('') + p.script : null;

export function registrationOf(tx, scope, ownerKey, amountSompi = REGISTRATION_SOMPI) {
  if (!/^[1-9][0-9]*$/.test(amountSompi)) throw Error('REGISTRATION_AMOUNT');
  const outputs = tx.outputs ?? [];
  const o = outputs[1], spk = o?.scriptPublicKey;
  if (outputs.length !== 2 && outputs.length !== 3) return null;
  if (outputs.length === 3) {
    const change = outputs[2];
    if (!ownerKey || !/^[1-9][0-9]*$/.test(String(change.value)) || change.covenant || wireSpk(change.scriptPublicKey) !== '000020' + ownerKey + 'ac') return null;
  }
  const match = scope.filter(r => String(o?.value) === amountSompi && wireSpk(spk) === wireSpk(r.spk) && !o?.covenant);
  if (match.length !== 1) return null;
  return {address: match[0].address, outpoint: {transactionId: tx.verboseData.transactionId, index: 1}, amountSompi, spk: match[0].spk};
}
