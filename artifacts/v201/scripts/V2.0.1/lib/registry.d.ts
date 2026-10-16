import type { Spk } from './covenant-id.js';
/** Optional ordinary registration payment: client/indexer policy, never covenant authorization.
 * Contract V2.0.1 fixes 0.2 TKAS; the V2 Profile (scripts/f3.2) keeps 0.05 TKAS independently. */
export declare const DEFAULT_REGISTRY_ADDRESS = "kaspatest:qztrjpfpuf6g9jw76e6ay909z43hndpv5maqkker6ur4enz7llarsh82ezggl";
export declare const REGISTRATION_SOMPI = 20000000n;
export declare const DEFAULT_REGISTRY_SPK: Spk;
export declare function checkRegistrySpk(spk: Spk): Spk;
