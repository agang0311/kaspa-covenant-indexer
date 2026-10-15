# Kaspa Covenant Indexer (`kaspa-covenant-indexer` / KCI)

[English](./README.en.md) | [简体中文](./README.md)

Kaspa 状态契约（Stateful Covenant）通用事件索引器、CID REST 服务与 Admin 管理控制台的独立自包含发布版本。
全面支持 **Kaspa Testnet 10 (TN10)**、**SilverScript** 与 **Argent** 契约生命周期管理。

本项目用于监听链上事件、定位花费者、验证选定链 acceptance、核验多版本契约状态机规则（内置 **Kaswin V2** `7aaf76fe`、**F3.2** `7ca61d81` 与 **F3** `1dc41809`），并提供：
- 🔍 **CID REST 查询 API**：为各类前端 DApp / Web 提供高并发、零延迟的契约最新状态只读查询；
- 🖥️ **Web 控制台 (`/admin`)**：内置现代化暗黑管理面板，支持实时节点连通性测速、节点健康体检、内存/负载监控与合约插件热启停；
- 🛡️ **纯只读与安全隔离**：链上无私钥、不签名、不广播；管理 API 全面受 Admin Token 认证保护，隔绝越权与注入风险；
- ⚡ **无持续扫链与四级定位漏斗**：基于 UTXO 通知与多级定位漏斗（节点内存池 -> 链尖快道 -> 官方 REST 零信任探针加速 -> 本地窗口切片兜底），实现亚秒级花费者与创世定位，内存占用极低（<100MB RSS）；
- 📦 **零外部依赖**：已内置 Node.js 22+ 兼容的官方 Kaspa WASM SDK (2.1.0) 与编译后契约规则包。

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
| `KASWIN_NODE_URL` | `--url` | 固定单 wRPC 节点地址 | 留空（使用节点池） |
| `KASWIN_NODE_URLS` | `--urls` | 逗号分隔的双节点地址 | 留空 |
| `KASWIN_REST_URL` / `KASPA_REST_URL` | `--rest-url` | 官方 REST 定位加速探针 URL（填 `none` 或 `off` 禁用） | `https://api-tn10.kaspa.org` |
| `ADMIN_TOKEN` | `--admin-token` | Web 管理后台认证令牌（不设置则自动生成并持久化在 SQLite 中） | 自动生成 |
| `KASWIN_REGISTRY_ADDRESSES` | `--registry-addresses` | 监听的 Registry 地址列表 | 官方 TN10 Registry |
| `KASWIN_CORS_ORIGIN` | `--cors-origin` | 跨域来源（`*`、具体域名或 `none` 禁用） | `*`（默认允许跨域） |
| `KASWIN_API_PORT` | `--api-port` | REST API 端口 | `8788` |
| `KASWIN_API_HOST` | `--api-host` | REST API 监听地址 | `0.0.0.0` |

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

服务默认运行在 `8788` 端口：

- `GET /v1/rounds`：获取已索引轮次列表（支持 `?limit=50`、`?cursor=<cid>` 分页）。
- `GET /v1/rounds?status=open`：筛选进行中轮次。
- `GET /v1/rounds?status=sealed`：筛选已封存/待结算轮次。
- `GET /v1/rounds?status=close`：筛选已完结/终局轮次。
- `GET /v1/rounds/{cid}`：根据 64 位十六进制 Covenant ID 查询轮次详情。

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
