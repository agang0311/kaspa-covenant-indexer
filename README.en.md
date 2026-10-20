# Kaspa Covenant Indexer (`kaspa-covenant-indexer` / KCI)

[English](./README.en.md) | [简体中文](./README.md)

Standalone stateful covenant event indexer, CID REST service, and Admin Studio.

A single process monitors both **Kaspa Mainnet** and **Testnet 10 (TN10)** — two hot-standby nodes per network —
providing generic lifecycle indexing for state covenants written in **SilverScript** / **Argent**.

Only **Kaswin V2.0.2** (Profile `ed0387c9`) ships as a built-in contract plugin; the plugin architecture accepts any
custom covenant DApp. This service provides:
- 🔍 **Open & Public REST Service**: Low-latency, CID-keyed REST endpoints completely open and unauthenticated for front-end DApps, explorers, and bots.
- 🖥️ **Web Admin Studio (`/admin`)**: Embedded dark-themed management dashboard featuring real-time node latency probing, node health diagnosis, Universal TXID Tracer, SilverScript compiler workbench, and contract plugin hot-toggling.
- 🛡️ **Role-Based Security**: Query and diagnostic APIs require zero authentication; only state-mutating management operations require the admin password (`ADMIN_TOKEN`).
- 🎯 **Universal TXID Tracer**: Trace from transaction hashes at **ANY stage** (Genesis, BUY, CLOSE, REFUND, DRAW) back to the root Genesis, extract the immutable Covenant ID (CID), deduplicate against existing database records, and automatically catch up on-chain states.
- ⚡ **No Continuous Chain Scanning & 4-Tier Spender Funnel**: Event-driven tracking through a four-tier locator funnel (Mempool -> Fast VC Page -> Official REST Zero-Trust Hint Oracle -> Bounded Fallback Scan), enabling sub-second spender resolution with minimal memory footprint (<100MB RSS).
- 📦 **100% Self-Contained Zero-SDK Pure-JS Architecture**: Pure native JavaScript implementation completely eliminating the 12MB C++/WASM binary SDK; native WebSocket Kaspa wRPC JSON protocol and Pure-JS CashAddr/Bech32 codecs, slashing resident memory to ~28MB RSS; embeds all contract rule artifacts (`artifacts/`) and local compilation tools (`bin/silverc`).

---

## Deployment Option 1: Docker & Docker Compose (Recommended)

Fastest and most consistent deployment on any machine with Docker installed.

### 1. Launch with Docker Compose

```bash
git clone https://github.com/agang0311/kaspa-covenant-indexer.git
cd kaspa-covenant-indexer

# (Optional) Customize admin token, node, or registry addresses:
cp .env.example .env

# Start in background
docker compose up -d
```

### 2. Run with Plain Docker

```bash
docker build -t kaspa-covenant-indexer .

docker run -d --name kaspa-covenant-indexer \
  -p 8788:8788 \
  -e ADMIN_TOKEN="your-admin-token" \
  -v kaspa-covenant-indexer-data:/data \
  --restart unless-stopped \
  kaspa-covenant-indexer
```

After ~10–15 seconds (initial pool probing and subscription handshake), verify the service:
- REST API: `curl http://localhost:8788/v1/rounds`
- Web Admin Dashboard: open `http://localhost:8788/admin` in your browser.


---

## Deployment Option 2: Linux Host One-Click Install (Non-Docker)

Ideal for standalone Linux servers or VPS instances. The installer automatically handles runtime dependencies, creates a dedicated system user, sets up data directories, and installs a hardened systemd service.

```bash
git clone https://github.com/agang0311/kaspa-covenant-indexer.git
cd kaspa-covenant-indexer

# Run installer (requires root or sudo privileges)
sudo bash install.sh
```

What the script does:
1. Detects or downloads an isolated Node.js 22 LTS binary (`x86_64` / `arm64`);
2. Creates a dedicated low-privilege `kaswin-index` system user;
3. Deploys files to `/opt/kaswin-indexer` and initializes the database at `/var/lib/kaswin-indexer`;
4. Installs and starts `kaswin-indexer.service` with automatic restart and health verification.

### Service Management Commands:
```bash
systemctl status kaswin-indexer.service   # Check service status
journalctl -u kaswin-indexer.service -f   # View live logs
systemctl restart kaswin-indexer.service  # Restart service
```

---

## Configuration

Settings can be customized via `.env` environment variables or CLI flags:

| Environment Variable | CLI Option | Description | Default |
|---|---|---|---|
| — | `--networks` | Comma-separated networks to monitor | `tn10` |
| `KASWIN_NODE_URL` | `--url` | Single fixed wRPC node URL | Empty (uses pool) |
| `KASWIN_NODE_URLS` | `--urls` | Comma-separated dual-node URLs (single network only) | Empty |
| — | `--pool` | Enable pool speed-testing and primary/standby election | Off |
| `KASWIN_REST_URL` / `KASPA_REST_URL` | `--rest-url` | Official REST accelerator probe URL (`none` / `off` to disable) | Each network's own REST |
| `ADMIN_TOKEN` | `--admin-token` | Admin Studio token (auto-generated in SQLite if omitted) | Auto-generated |
| `KASWIN_REGISTRY_ADDRESSES` | `--registry-addresses` | Override watched Registry address list | Each network's default |
| `KASWIN_CORS_ORIGIN` | `--cors-origin` | Allowed CORS origin (`*`, URL, or `none`) | `*` |
| `KASWIN_API_PORT` | `--api-port` | REST API listening port | `8788` |
| `KASWIN_API_HOST` | `--api-host` | REST API listening host | `0.0.0.0` |

### Multi-network example

```bash
# One process for mainnet + TN10, dual hot-standby nodes per network
node indexer.mjs run \
  --networks tn10,mainnet \
  --contracts contracts/v202.json \
  --db /var/lib/kaswin-event-indexer/events.sqlite \
  --pool \
  --api-host 127.0.0.1 --api-port 8788
```

Each network gets its own SQLite database (`<db>.<key>.sqlite`) bound via `meta.network`. Reusing one file across
networks throws `NETWORK_MISMATCH_DB`, because the same x-only key yields the **same** scriptPublicKey on both networks.

---

## Architecture: Four-Tier Spender Locator Funnel

Under Kaspa GHOSTDAG, the indexer avoids expensive unbounded block scans. When a covenant tip UTXO is spent, the spender resolution pipeline cascades through four progressive stages:

1. **Tier 1 · Node Mempool (`fromMempool`, <5ms)**:
   Immediately extracts candidate transactions from the connected node's memory pool.
2. **Tier 2 · Virtual Selected Parent Chain Fast Path (`tryFast`, ~50ms)**:
   Matches candidate transactions within recent virtual selected chain pages.
3. **Tier 3 · Official REST Zero-Trust Hint Oracle (`fromRest`, ~1s)**:
   Accelerates historical recovery by querying the official REST endpoint (`api-tn10.kaspa.org`) for the contract address history to obtain the spender `txid` and `accepting_block_hash`.
   > **Zero-Trust Principle**: The REST API provides location hints, never consensus proofs. The indexer strictly verifies selected-chain membership, acceptance status, and contract transition rules through its trusted wRPC node.
4. **Tier 4 · Bounded Window Fallback (`windowFind`, Fail-Safe)**:
   Walks historical accepted chain blocks in 96MB memory-isolated worker processes, ensuring full autonomy even if REST is offline or disabled.

---

## REST API Reference

The service serves read-only JSON endpoints on port `8788` (**100% open and unauthenticated for all query operations**,
CORS-friendly with `Access-Control-Allow-Origin: *`).

### 0. Multi-network addressing (three equivalent forms)

One instance serves both networks. Pick any form:

| Form | Example |
|---|---|
| **Network-first prefix (recommended)** | `/tn10/rounds`, `/mainnet/rounds`, `/tn10/v1/rounds` |
| Legacy path prefix | `/v1/tn10/rounds`, `/v1/mainnet/rounds` |
| Query parameter | `/v1/rounds?network=tn10` |

Network keys are `tn10` / `mainnet`; the network ID (`testnet-10`) is also accepted.
An unknown prefix returns `400 {"detail":"UNKNOWN_NETWORK","networks":[...]}`.
Omitting the network uses the primary view (first entry of `--networks`).

### 1. Covenant Rounds Query
- `GET /{net}/rounds`: List indexed rounds with `?limit=` (max 200) and `?cursor=<cid>` keyset pagination.
- **Server-side filters**:
  - `?status=open|sealed|close|unknown` — by category.
  - `?phase=1|2|5` — by state-machine phase (1 open / 2 sealed / 5 refunding).
  - `?terminal=true|false` — completed only / in-progress only.
- `GET /{net}/rounds/{cid}`: Fetch a round by its 64-char hex CID **or by its Genesis TXID**.
  List rows carry lightweight ledger fields (`sold` / `purchaseCount` / `ticketCap` / `minTickets` / `ticketPrice` / `state`);
  the full `purchases` directory is returned only in the detail view.

### 2. Network Catalog
- `GET /v1/networks`: Full catalog (key / networkId / genesis / registry / rounds / live / activeNode).
- `GET /{net}/networks`: Only that network's catalog entry.

### 3. Service & Node Status (Unauthenticated)
- `GET /status` or `GET /v1/status`: Retrieve indexer version, active consensus node, DAA score, round counts, and active contract plugins.
- `GET /admin/api/status`: Admin overview metrics and journal mirrors.
- `GET /admin/api/plugins`: List of loaded contract plugins and profile digests.
- `GET /admin/api/node-pool`: Node pool health matrix and dual-node topology.
- `POST /admin/api/probe-rest`: Official REST zero-trust accelerator handshake diagnosis.

### 4. Universal TXID Tracer (Unauthenticated)
- `POST /admin/api/track`: Submit a transaction ID from any stage to trace backwards, extract CID, deduplicate, and automatically catch up:
  ```bash
  curl -s -X POST -H "Content-Type: application/json" \
    -d '{"txid": "85d293040cb5f342f26f378a9918b4449439bc025840af27195e47052c067dad"}' \
    http://localhost:8788/admin/api/track
  ```

### 5. Admin Management Operations (Requires Password)
- `POST /admin/api/recheck-discoveries`: Re-evaluate previously unmatched genesis tasks and trigger auto-catchup.
- `POST /admin/api/plugins/toggle`: Enable or retire contract plugins.
- `POST /admin/api/plugins/update-config`: Update plugin invariant guards and watched addresses.
- `POST /admin/api/plugins/remove`: Uninstall a plugin.
- `POST /admin/api/compile`: Compile SilverScript (`.sil`) code into JSON artifacts.
- `POST /admin/api/generate-plugin` / `POST /admin/api/install-plugin`: Auto-generate and hot-load plugins.

---

## Developer & CLI Commands

When developing or debugging locally on Node.js 22+:

```bash
# Inspect loaded contract plugins and pinned profiles (offline)
node indexer.mjs contracts

# Inspect local database round statuses and journal
node indexer.mjs status --db events.sqlite

# Run in foreground for debugging
node indexer.mjs run --db events.sqlite --pool --api-port 8788
```

---

## Adapting for Custom Covenant DApps

This project is built around a pluggable architecture. The underlying wRPC JSON connection, dual-node failover pool, selected-chain acceptance verification, reorg rollback engine, and SQLite storage are **completely generic and reusable**.

If you wish to adapt this indexer for your own Kaspa Covenant DApp (e.g., AMM, auctions, order books, bridges):
👉 **See the complete walkthrough: [Custom DApp Adaptation Guide (CUSTOM_DAPP_GUIDE.en.md)](./CUSTOM_DAPP_GUIDE.en.md)**

---

---

## Documentation Index

| Document | Contents |
|---|---|
| [README.md](./README.md) / [README.en.md](./README.en.md) | Current capabilities, deployment, API |
| [CHANGELOG.md](./CHANGELOG.md) | Version history |
| [CUSTOM_DAPP_GUIDE.md](./CUSTOM_DAPP_GUIDE.md) | Plugin authoring guide for custom covenant DApps |
| [docs/MULTI-NETWORK-PREFLIGHT.md](./docs/MULTI-NETWORK-PREFLIGHT.md) | Multi-network design constraints and acceptance boundaries |
| [docs/INDEXER-CODE-REVIEW-20261017.md](./docs/INDEXER-CODE-REVIEW-20261017.md) | Code review record (2026-10-17) |

---

## Contract Version

**This project supports Kaswin V2.0.2 (Profile `ed0387c9`) only.**

The V2 / V2.0.1 / F3 / F3.2 plugin modules and `artifacts/` were removed on 2026-10-16.
Rounds created by those versions remain in the database as history but classify as `unknown` in the API
(fail-closed), since their plugins no longer exist. See [CHANGELOG.md](./CHANGELOG.md#历史版本清理说明).
