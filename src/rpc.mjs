// Native WebSocket JSON wRPC client for Kaspa nodes (Zero WASM SDK dependencies).
// Communicates with any Kaspa node exposing wRPC JSON protocol (standard on all TN10 and mainnet kaspad nodes).

export class JsonRpcClient {
  constructor({url, networkId = 'testnet-10', timeoutDuration = 15000} = {}) {
    let cleanUrl = String(url || '').trim();
    if (cleanUrl) {
      if (/^wss?:\/\/[^/]+\/wrpc$/i.test(cleanUrl)) cleanUrl += '/json';
      cleanUrl = cleanUrl.replace(/\/wrpc\/borsh\/?$/i, '/wrpc/json');
    }
    this.url = cleanUrl || null;
    this.networkId = networkId;
    this.timeoutDuration = timeoutDuration;
    this.ws = null;
    this.idCounter = 1;
    this.pending = new Map();
    this.listeners = new Map();
    this.connected = false;
    this.connecting = null;
  }

  addEventListener(event, fn) {
    if (!this.listeners.has(event)) this.listeners.set(event, new Set());
    this.listeners.get(event).add(fn);
  }

  removeEventListener(event, fn) {
    this.listeners.get(event)?.delete(fn);
  }

  emit(event, data) {
    const set = this.listeners.get(event);
    if (set) for (const fn of set) { try { fn(data); } catch {} }
  }

  async connect({timeoutDuration = this.timeoutDuration} = {}) {
    if (this.connected && this.ws?.readyState === WebSocket.OPEN) return;
    if (this.connecting) return this.connecting;

    this.connecting = new Promise((resolve, reject) => {
      let timer = setTimeout(() => {
        this.connecting = null;
        try { this.ws?.close(); } catch {}
        reject(new Error('CONNECT_TIMEOUT'));
      }, timeoutDuration);
      timer.unref?.();

      try {
        const ws = new WebSocket(this.url);
        this.ws = ws;

        ws.onopen = () => {
          clearTimeout(timer);
          this.connected = true;
          this.connecting = null;
          this.emit('connect', {});
          resolve();
        };

        ws.onclose = () => {
          clearTimeout(timer);
          const wasConnected = this.connected;
          this.connected = false;
          this.connecting = null;
          if (wasConnected) this.emit('disconnect', {});
          for (const [id, p] of this.pending) {
            clearTimeout(p.timer);
            p.reject(new Error('NODE_DISCONNECTED'));
          }
          this.pending.clear();
        };

        ws.onerror = (e) => {
          clearTimeout(timer);
          if (this.connecting) {
            this.connecting = null;
            reject(new Error('CONNECT_FAILED: ' + (e?.message || 'WebSocket Error')));
          }
        };

        ws.onmessage = (event) => {
          this._handleMessage(event.data);
        };
      } catch (err) {
        clearTimeout(timer);
        this.connecting = null;
        reject(err);
      }
    });

    return this.connecting;
  }

  async disconnect() {
    this.connected = false;
    this.connecting = null;
    if (this.ws) {
      try { this.ws.close(); } catch {}
      this.ws = null;
    }
  }

  _handleMessage(raw) {
    let msg;
    try { msg = JSON.parse(raw); } catch { return; }

    // 1. Response to a pending RPC request
    if (msg.id !== undefined && this.pending.has(msg.id)) {
      const p = this.pending.get(msg.id);
      clearTimeout(p.timer);
      this.pending.delete(msg.id);
      if (msg.error) {
        p.reject(new Error(msg.error.message || JSON.stringify(msg.error)));
      } else {
        p.resolve(msg.params ?? {});
      }
      return;
    }

    // 2. Notifications
    const method = msg.method || '';
    const params = msg.params || {};

    if (method === 'virtualChainChangedNotification' || params.VirtualChainChanged) {
      const data = params.VirtualChainChanged || params;
      this.emit('virtual-chain-changed', { data });
    } else if (method === 'utxosChangedNotification' || params.UtxosChanged) {
      const data = params.UtxosChanged || params;
      this.emit('utxos-changed', { data });
    }
  }

  async call(method, params = {}, timeoutMs = 15000) {
    if (!this.connected || this.ws?.readyState !== WebSocket.OPEN) {
      await this.connect({timeoutDuration: timeoutMs});
    }
    const id = this.idCounter++;
    const payload = JSON.stringify({ id, method, params });

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`RPC_TIMEOUT: ${method}`));
      }, timeoutMs);
      timer.unref?.();

      this.pending.set(id, { resolve, reject, timer, method });
      try {
        this.ws.send(payload);
      } catch (e) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(e);
      }
    });
  }

  // Subscription methods
  async subscribeUtxosChanged(addresses) {
    return this.call('subscribe', { UtxosChanged: { addresses } });
  }

  async unsubscribeUtxosChanged(addresses) {
    return this.call('unsubscribe', { UtxosChanged: { addresses } });
  }

  async subscribeVirtualChainChanged(includeAccepted = false) {
    return this.call('subscribe', { VirtualChainChanged: { include_accepted_transaction_ids: Boolean(includeAccepted) } });
  }

  // Queries
  async getBlockDagInfo() { return this.call('getBlockDagInfo', {}); }
  async getServerInfo() { return this.call('getServerInfo', {}); }
  async getInfo() { return this.call('getInfo', {}); }
  async getSinkBlueScore() { return this.call('getSinkBlueScore', {}); }
  async getBlock(params) { return this.call('getBlock', params); }
  async getBlocks(params) { return this.call('getBlocks', params); }
  async getUtxosByAddresses(params) { return this.call('getUtxosByAddresses', params); }
  async getVirtualChainFromBlock(params) { return this.call('getVirtualChainFromBlock', params); }
  async getVirtualChainFromBlockV2(params) { return this.call('getVirtualChainFromBlockV2', params); }
  async getMempoolEntriesByAddresses(params) { return this.call('getMempoolEntriesByAddresses', params); }
}
