// Pure JavaScript SDK compatibility shim (Zero WASM dependencies).
import {addressToSpk, spkToAddress, addressOf, addressOfFactory} from './address.mjs';
import {JsonRpcClient} from './rpc.mjs';

export {addressToSpk, spkToAddress, addressOf, addressOfFactory, JsonRpcClient};

export function loadSdk() {
  return {
    version: () => '2.1.0',
    RpcClient: JsonRpcClient,
    Encoding: { Borsh: 0, SerdeJson: 1 },
    ConnectStrategy: { Fallback: 1, Retry: 2 },
    Address: class {
      constructor(addr) { this.addr = String(addr || ''); }
      toString() { return this.addr; }
    },
    ScriptPublicKey: class {
      constructor(version, script) { this.version = Number(version); this.script = String(script); }
    },
    Resolver: class {
      constructor(opts = {}) { this.opts = opts; }
    },
    payToAddressScript: (a) => addressToSpk(a.toString ? a.toString() : String(a)),
    addressFromScriptPublicKey: (s, net) => addressOf(s, net)
  };
}
