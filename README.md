# Kaspa Covenant Indexer (`kaspa-covenant-indexer` / KCI)

[English](./README.en.md) | [简体中文](./README.md)

Kaspa 状态契约（Stateful Covenant）通用事件索引器、CID REST 服务与 Admin 管理控制台的独立自包含发布版本。

单一进程同时监控 **Kaspa Mainnet** 与 **Testnet 10 (TN10)** 两条网络（每网双节点热备），
面向 **SilverScript** / **Argent** 编写的状态契约提供通用生命周期索引能力。

当前仅内置 **Kaswin V2.0.2**（Profile `ed0387c9`）一个契约插件；插件化架构允许接入任意自定义 Covenant DApp。
本项目提供：
- 🔍 **全开放免密 REST API**：所有查询接口（`/v1/rounds`、`/status`、`/admin/api/status`、`/admin/api/track` 等）**100% 免密开放**，供各类前端 DApp / 监控高并发调用；
- 🖥️ **Web 控制台 (`/admin`)**：内置现代化暗黑管理面板，支持实时节点连通性测速、节点健康体检、通用交易全链路追溯、SilverScript 在线编译工坊与契约插件热载；
- 🛡️ **管理操作安全隔离**：链上无私钥、不签名、不广播；仅修改配置、启停插件、安装卸载等**核心管理动作**才需要管理员密码；
- 🎯 **通用交易追溯器 (Universal Tracer)**：支持输入合约生命周期中**任意阶段的 TXID**（Genesis / BUY / CLOSE / REFUND / DRAW），自动沿链反向回溯提取 CID、精准去重并由索引器全自动追补状态；
- ⚡ **无持续扫链与四级定位漏斗**：基于 UTXO 通知与多级定位漏斗（节点内存池 -> 链尖快道 -> 官方 REST 零信任探针加速 -> 本地窗口切片兜底），实现亚秒级花费者与创世定位，内存占用极低（<100MB RSS）；
- 📦 **100% 零外部依赖与 Zero-SDK 纯 JS 架构**：纯原生 JavaScript 架构，彻底移除 12MB C++/WASM SDK 二进制依赖；原生 Node.js WebSocket 实现 Kaspa wRPC JSON 协议与 Pure-JS CashAddr/Bech32 编解码，常驻内存仅 ~28MB RSS；内嵌全版本契约规则产物（`artifacts/`）与本地编译工具（`bin/silverc`）。

---

## 快速部署方案一：Docker / Docker Compose（推荐首选）

无论在什么系统上，只要已安装 Docker，即可一键拉起。

### 1. 使用 Docker Compose 一键启动

```bash
git clone https://github.com/agang0311/kaspa-covenant-indexer.git
cd kaspa-covenant-indexer

# （可选）配置管理员 Token 或自定义节点：
cp .env.example .env

# 一键启动
docker compose up -d
```

### 2. 使用纯 Docker 命令运行

```bash
docker build -t kaspa-covenant-indexer .

docker run -d --name kaspa-covenant-indexer \
  -p 8788:8788 \
  -e ADMIN_TOKEN="your-admin-token" \
  -v kaspa-covenant-indexer-data:/data \
  --restart unless-stopped \
  kaspa-covenant-indexer
```

启动约 10~15 秒（完成双节点探测与订阅握手）后：
- 访问只读 API：`curl http://localhost:8788/v1/rounds`
- 访问管理面板：浏览器打开 `http://localhost:8788/admin`


---

## 快速部署方案二：Linux 独立主机一键安装（无 Docker）

适合部署在独立 VPS 或物理机上，脚本会自动创建系统用户、设置数据目录、注册 systemd 服务并开机自启。

```bash
git clone https://github.com/agang0311/kaspa-covenant-indexer.git
cd kaspa-covenant-indexer

# 执行一键安装（需要 root / sudo 权限）
sudo bash install.sh
```

脚本将自动完成：
1. 检查或下载独立的 Node.js 22 LTS 二进制；
2. 创建 `kaswin-index` 专属低权限运行用户；
3. 将程序部署至 `/opt/kaswin-indexer`，数据目录设为 `/var/lib/kaswin-indexer`；
4. 注册并启动 `kaswin-indexer.service` 并通过健康检查。

### 服务管理命令：
```bash
systemctl status kaswin-indexer.service   # 查看状态
journalctl -u kaswin-indexer.service -f   # 查看实时日志
systemctl restart kaswin-indexer.service  # 重启服务
```

---

## 配置说明

支持通过 `.env` 环境变量或启动参数覆盖配置：

| 环境变量 | CLI 参数 | 说明 | 默认值 |
|---|---|---|---|
| — | `--networks` | 监控的网络列表（逗号分隔） | `tn10` |
| `KASWIN_NODE_URL` | `--url` | 固定单 wRPC 节点地址 | 留空（使用节点池） |
| `KASWIN_NODE_URLS` | `--urls` | 逗号分隔的双节点地址（仅单网络） | 留空 |
| — | `--pool` | 启用节点池自动测速与主备选举 | 关闭 |
| `KASWIN_REST_URL` / `KASPA_REST_URL` | `--rest-url` | 官方 REST 定位加速探针 URL（`none` / `off` 禁用） | 各网络自己的 REST |
| `ADMIN_TOKEN` | `--admin-token` | Admin 控制台令牌（不设置则自动生成并持久化在 SQLite） | 自动生成 |
| `KASWIN_REGISTRY_ADDRESSES` | `--registry-addresses` | 覆盖监听的 Registry 地址列表 | 各网络默认 Registry |
| `KASWIN_CORS_ORIGIN` | `--cors-origin` | 跨域来源（`*`、具体域名或 `none` 禁用） | `*` |
| `KASWIN_API_PORT` | `--api-port` | REST API 端口 | `8788` |
| `KASWIN_API_HOST` | `--api-host` | REST API 监听地址 | `0.0.0.0` |

### 多网络示例

```bash
# 单进程同时监控主网与 TN10，每网双节点热备
node indexer.mjs run \
  --networks tn10,mainnet \
  --contracts contracts/v202.json \
  --db /var/lib/kaswin-event-indexer/events.sqlite \
  --pool \
  --api-host 127.0.0.1 --api-port 8788
```

每个网络使用独立的 SQLite 数据库（`<db>.<key>.sqlite`），通过 `meta.network` 绑定，
跨网络复用同一文件会抛 `NETWORK_MISMATCH_DB`（因为同一 x-only 公钥在两条网络生成**相同**的 SPK）。

---

## 核心架构：四级定位漏斗与 REST 零信任线索探针

在 Kaspa GHOSTDAG 体系下，索引器绝不依赖对全链数万个区块的漫无目的轮询。当检测到轮次 Tip 离开 UTXO 集合需要定位后续花费者（Spender）时，流水线按四级漏斗极速收敛：

1. **第一级 · 节点内存池 (`fromMempool`, <5ms)**：
   若交易刚广播或待入块，直接在连接节点的 Mempool 中秒级提取。
2. **第二级 · 选定链链尖快道 (`tryFast`, ~50ms)**：
   若交易在最近的虚拟选定链（VSPC）页面中入块，直接命中 Full Row 完成状态提取。
3. **第三级 · 官方 REST 零信任探针 (`fromRest`, ~1s)**：
   若索引器断线重连或追赶跨越数千个区块的历史交易，直接向官方 REST 端点（如 `api-tn10.kaspa.org`）请求合约地址历史，秒级获取花费者 `txid` 与其入块的 `accepting_block_hash`。
   > **零信任安全规范**：REST 仅提供线索（Hint），绝不作为证据。索引器获取线索后，仍由本地受信节点（wRPC）逐字节校验选定链共识、接受状态与插件状态机转移规则，无法被任何第三方虚假响应投毒。
4. **第四级 · 内存安全窗口扫描 (`windowFind`, 兜底防线)**：
   若前三级均未命中，系统启动 96MB 内存隔离的子进程，按蓝色评分窗口逐页向前回溯，确保系统即使在 REST 宕机或无外网环境下仍 100% 具备链上自洽追踪能力。

---

## REST API 接口说明

服务默认运行在 `8788` 端口，**所有查询类接口 100% 开放免密（CORS 友好，`Access-Control-Allow-Origin: *`）**。

### 0. 多网络寻址（三种写法等价）

同一实例同时服务两条网络，任选一种：

| 写法 | 示例 |
|---|---|
| **网络优先前缀（推荐）** | `/tn10/rounds`、`/mainnet/rounds`、`/tn10/v1/rounds` |
| 旧版路径前缀 | `/v1/tn10/rounds`、`/v1/mainnet/rounds` |
| Query 参数 | `/v1/rounds?network=tn10` |

网络标识既可用 `tn10` / `mainnet`，也可用 `testnet-10`。
未知网络前缀返回 `400 {"detail":"UNKNOWN_NETWORK","networks":[...]}`。
不指定网络时使用主视图（`--networks` 的第一个）。

### 1. 契约轮次查询
- `GET /{net}/rounds`：已索引轮次列表，支持 `?limit=`（上限 200）、`?cursor=<cid>` 游标分页。
- **服务端过滤**：
  - `?status=open|sealed|close|unknown`：按分类筛选。
  - `?phase=1|2|5`：按状态机阶段筛选（1 售票中 / 2 已封盘 / 5 退款中）。
  - `?terminal=true|false`：只看已终局 / 只看进行中。
- `GET /{net}/rounds/{cid}`：按 64 位十六进制 CID **或创世 TXID** 查询轮次详情。
  列表视图返回轻量账本字段（`sold` / `purchaseCount` / `ticketCap` / `minTickets` / `ticketPrice` / `state`）；
  `purchases` 完整购买目录仅详情视图返回。

### 2. 网络目录
- `GET /v1/networks`：全部网络目录（key / networkId / genesis / registry / rounds / live / activeNode）。
- `GET /{net}/networks`：仅该网络的目录条目。

### 3. 索引器与节点状态查询（免密）
- `GET /status` 或 `GET /v1/status`：获取当前索引器版本、主连接节点、选定链 DAA、收录轮次统计及当前活动契约插件。
- `GET /admin/api/status`：管理看板运行指标与审计流水镜像。
- `GET /admin/api/plugins`：已加载的全部契约插件列表与规则指纹。
- `GET /admin/api/node-pool`：候选节点池与双节点热备当前健康拓扑。
- `POST /admin/api/probe-rest`：官方 REST 零信任加速探针在线握手诊断。

### 4. 通用交易全链路追溯器 (Universal Tracer, 免密)
- `POST /admin/api/track`：输入任意阶段的交易哈希，自动反向回溯至创世、提取 CID、与本地数据库比对去重；若为未收录轮次则自动提交索引器进行全生命周期追块：
  ```bash
  # 请求体
  curl -s -X POST -H "Content-Type: application/json" \
    -d '{"txid": "85d293040cb5f342f26f378a9918b4449439bc025840af27195e47052c067dad"}' \
    http://localhost:8788/admin/api/track
  ```

### 5. 核心管理变更操作 (需管理员密码验证)
- `POST /admin/api/recheck-discoveries`：重审所有被忽略的历史创世任务并触发索引器全自动追块。
- `POST /admin/api/plugins/toggle`：启用或停用指定契约插件。
- `POST /admin/api/plugins/update-config`：更新插件创世守门断言、监听地址与不变量。
- `POST /admin/api/plugins/remove`：卸载插件。
- `POST /admin/api/compile`：在线编译 SilverScript (`.sil`) 契约。
- `POST /admin/api/generate-plugin` / `POST /admin/api/install-plugin`：自动生成并热载插件。

---

## 开发者与调试命令

本地已有 Node.js 22 环境时，可直接使用以下命令：

```bash
# 查看加载的契约规则与配置（不连网）
node indexer.mjs contracts

# 查看数据库当前索引状态与轮次
node indexer.mjs status --db events.sqlite

# 前台启动运行
node indexer.mjs run --db events.sqlite --pool --api-port 8788
```

---

## 适配其他合约 DApp (二次开发)

本项目采用插件化架构，底层 Borsh wRPC 连接、双节点故障转移、Selected-chain acceptance 共识核验、重组回滚和 SQLite 存储**完全通用**。

如果你想将此 Indexer 改造并接入自己的 Kaspa Covenant DApp（如 AMM、拍卖、借贷、订单薄等）：
👉 **详细步骤与插件编写示例见：[二次开发与适配指南 (CUSTOM_DAPP_GUIDE.md)](./CUSTOM_DAPP_GUIDE.md)**

---

---

## 文档索引

| 文档 | 内容 |
|---|---|
| [README.md](./README.md) / [README.en.md](./README.en.md) | 当前能力、部署与 API |
| [CHANGELOG.md](./CHANGELOG.md) | 版本变更历史 |
| [CUSTOM_DAPP_GUIDE.md](./CUSTOM_DAPP_GUIDE.md) | 接入自定义 Covenant DApp 的插件编写指南 |
| [docs/MULTI-NETWORK-PREFLIGHT.md](./docs/MULTI-NETWORK-PREFLIGHT.md) | 多网络改造的设计约束与验收边界 |
| [docs/INDEXER-CODE-REVIEW-20261017.md](./docs/INDEXER-CODE-REVIEW-20261017.md) | 2026-10-17 代码审查记录 |

---

## 合约版本说明

**本项目仅支持 Kaswin V2.0.2（Profile `ed0387c9`）。**

V2 / V2.0.1 / F3 / F3.2 的插件模块与 `artifacts/` 产物已于 2026-10-16 全部删除。
数据库中由旧版本创建的轮次保留为历史记录，但因插件不存在，API 中统一归类为 `unknown`（fail-closed）。
详见 [CHANGELOG.md](./CHANGELOG.md#历史版本清理说明)。
