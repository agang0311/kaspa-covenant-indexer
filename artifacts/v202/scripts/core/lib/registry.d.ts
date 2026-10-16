import type { Spk } from './covenant-id.js';
/** Optional ordinary registration payment: client/indexer policy, never covenant authorization.
 * V2.0.2 retains the 0.2 KAS/TKAS policy; archived V2 used 0.05 independently. */
export declare const DEFAULT_REGISTRY_ADDRESS = "kaspatest:qztrjpfpuf6g9jw76e6ay909z43hndpv5maqkker6ur4enz7llarsh82ezggl";
export declare const REGISTRATION_SOMPI = 20000000n;
export declare const DEFAULT_REGISTRY_SPK: Spk;
export declare function checkRegistrySpk(spk: Spk): Spk;
