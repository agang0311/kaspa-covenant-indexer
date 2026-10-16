// Pure check (no SDK/network): Genesis output 1 must be exactly the V2.0.2 registration payment.
// 0.2 KAS/TKAS to the selected network's registry, P2PK; output 2 optional creator change.
// This retains the V2.0.1 payment rule; it does not imply an external Indexer has installed the new Profile.
// registrySpk defaults to the TN10 registry (scripts/core DEFAULT_REGISTRY_SPK); mainnet passes its own.
import {stable} from '../core/lib/bytes.js';
import {DEFAULT_REGISTRY_SPK, REGISTRATION_SOMPI} from '../core/lib/registry.js';
const fail = code => { const e = new Error(code); e.safeCode = code; throw e; };
export function checkRegistration(tx, ownerKey, registrySpk = DEFAULT_REGISTRY_SPK) {
  const o = tx.outputs[1];
  if (!(o && o.value === BigInt(REGISTRATION_SOMPI) && !o.covenant && stable(o.scriptPublicKey) === stable(registrySpk))) fail('REGISTRY_OUTPUT');
  if (!(tx.outputs.length === 2 || (tx.outputs.length === 3 && !tx.outputs[2].covenant && tx.outputs[2].scriptPublicKey.script === `20${ownerKey}ac`))) fail('REGISTRY_LAYOUT');
}
