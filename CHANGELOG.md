# Changelog

本文件记录本项目的版本变更。格式参考 [Keep a Changelog](https://keepachangelog.com/)，
版本号遵循 [Semantic Versioning](https://semver.org/)。

README 只描述**当前**能力与用法；历史变更一律记录在此。

---

## [Unreleased]

### Added
- 原生 Rust 实现 [`rust/`](rust/README.md)：V2.0.2 规则、wRPC JSON、双节点、独立网络数据库、事件引擎、REST/Admin、SilverScript 编译和历史回放/差分工具。运行时不依赖 Node.js 或 Kaspa SDK。
- 本机（`kci-rust-lab.service`）与 jp / la 两台 VPS（`kci.service`，musl 静态二进制）已切换到 Rust，均监听 8788，
  原 JS 服务停用但保留；其余 JS 部署未改动。仅内置原生契约插件可执行，不安装 JavaScript 插件。
- 原生 `rolling-v1` 插件（仅 `tn10`，UNVALIDATED，固定 5 套合约工件 [`artifacts/rolling-v1/`](artifacts/rolling-v1/)）与简化只读接口
  `/tn10/info`、`/tn10/rounds`（列表、详情、`/history`、`/export`）及 `/tn10/txid/<txid>`；支持多 UTXO 分片、顺序批量追块与回滚。
- Admin 控制台新增「🎲 Rolling 轮次」只读标签页（固定 TN10，调用 `/tn10/{info,rounds,txid}`），支持按 CID / TXID 查询、
  查看账本摘要 / Manager outpoint / 冻结分片 / 分页转换历史与导出原始 JSON；金额均使用 `BigInt` 与十进制字符串处理。
- Rust：剪枝点以下的花费恢复（[`rust/src/pruned.rs`](rust/src/pruned.rs)）。tip 的花费交易接受于节点剪枝点之下时，
  从 REST 取交易内容，仅在以下条件全部满足时提交：
  - 本地按 rusty-kaspa 规则重算 txid 一致（REST 不提供 lockTime / sequence / gas，只尝试少量候选值并要求精确匹配）；
  - 资金输入金额来自已存储、或同样通过 txid 校验的前序交易；
  - 节点仍能通过它的某个输出证明已接受并给出确切接受 DAA：该输出仍在 UTXO 集中，或被节点保留的子交易花费
    （V2 `Full` 行输入的 `utxoEntry.blockDaaScore`）；
  - 该 DAA 低于剪枝点 DAA，共识上不可逆，因此直接记为 `FINAL`。

  签名脚本（v1 txid 不承诺）、接受块哈希与 blue score 仍来自 REST，由 `evidence.prunedRecovery` 与
  `witnessSource=REST_COPY_TXID_MATCH` 标明。规则拒绝 REST 见证时延后为 `EVIDENCE_PRUNED`，不标 `STALE`。
  只追溯一层后代：交易与其子交易都已剪枝、且没有存活输出时保持 `EVIDENCE_PRUNED`（最多每 5 分钟重试一次）。
- Rust：live-tip 锚点。`reconcile()` 看到 tip 未花费时记录至少 30 秒前的 checkpoint；tip 的创建区块头被剪枝后，
  花费搜索从该锚点开始。
- Rust 的证据边界收紧：接受见证补全失败直接返回错误；缺失区块头不按记录年龄升级 FINAL；管理跟踪/重试请求需管理员令牌。这些差异只作用于 Rust，下述历史条目仍描述 JS 版本。
- 网络优先路径前缀 `/tn10` 与 `/mainnet`：一个实例同时服务两条网络，无需 query 参数。
  - `/tn10/rounds`、`/mainnet/rounds`、`/tn10/v1/rounds`、`/tn10/networks`、`/tn10/status` 等。
  - 旧路径 `/v1/tn10/rounds`、`/v1/rounds?network=` 全部保留，向后兼容。
  - 未知网络前缀返回 `400 UNKNOWN_NETWORK` 并附可用网络列表，而非裸 404。
- `GET /v1/rounds` 服务端过滤：`?phase=1|2|5`、`?terminal=true|false`。
- 列表视图暴露轻量账本元数据 `sold` / `purchaseCount` / `ticketCap` / `minTickets` / `ticketPrice`
  与 `state` 快照；`purchases` 大数组仍只在详情视图返回。
- Admin 控制台新增「已入池节点」表格，支持运行时「设为主连接」与「重新选举最优主备」。

### Changed
- `checkFinality()` 不再因单个已剪枝区块头而中断整轮巡检；超过 1 小时的剪枝记录直接归档为 `FINAL`。
- `reconcile()` 巡检范围扩大到 `LIVE` + `STALE` 的活动轮次，`STALE` 轮次可在链上 Tip 已花费时自愈。
- Admin UI 所有 TXID / 区块哈希完整显示，并支持点击复制。
- 概览状态分类 `category()` 支持 `rolling-v1@*` 轮次（`sales`→`open`、`sealed`/`win`/`frozen`→`sealed`、`refund` 或已终局→`close`、`STALE`/`ROLLED_BACK`→`unknown`），概览卡片同步显示 Rolling 轮次计数。
- 插件配置元数据按插件类型返回 `configurable`（`rolling-v1` 仅开放 `registryAddresses`），Admin 配置弹窗隐藏并跳过提交未开放选项。

### Fixed
- **Admin 插件列表错位**：改为按 `module` 匹配运行时插件状态，而非数组下标；未在当前网络加载的插件（如主网视图下的 `rolling-v1`）
  显示「本网络未加载」，不再误标为 `ACTIVE`。
- **Rust 主网轮次卡在已剪枝区块头**：`resolve_spend` 先读取 tip 创建区块的区块头。标准 10 BPS 主网节点只保留约
  32 小时，超过该时长未花费的 tip 返回 `cannot find header`，每轮巡检报错。3 个主网轮次（CID `7e48d62a…`、
  `183de5b5…`、`5e1f79a3…`）因此停在 2026-10-30 的花费之前，现已通过上述恢复追平。
- **主网 Tracer 404**：`/admin/api/track` 忽略 `?network=`，始终使用主视图（TN10）的 REST，
  导致主网 TXID 查询恒返回 `HTTP 404`。现按网络选择 store / engine / restUrl。
- **`SUCCESSOR_UTXO_MISMATCH`**：GHOSTDAG 下包含块 DAA ≤ 接受块 DAA，原代码用严格等值判定，
  将合法的并行块合并误判为不匹配。
- **`ACCEPTED_ROW_NOT_FOUND`**：`minConfirmationCount` 边界余量不足，偶尔把目标接受块裁掉。
  现留 2 块余量，并在未命中时以 `minConfirmationCount: 0` 重试一次。
- **零买退款轮次卡 STALE**：V2 `High` 详细度返回 `version` / `lockTime` 为 `null`，
  `isFullTransaction()` 只检查 `undefined`，把不完整交易当作完整交易，最终 `BigInt(null)` 抛错。
- **`PAYOUT_MISMATCH`**：`normalizeConsensusFields()` 把 wire 字符串形式的 `scriptPublicKey`
  当作对象读取 `.script`，脚本被抹成空串。现统一走 `wireSpk()`。
- **`SUCCESSOR_MISMATCH` / `CLOSE_LOCKTIME`**：`fromRest()` 未复制 `covenant_id`，且 REST 的
  `lock_time` 为 `null`。现补全 covenant 元数据，并对 REST 线索强制从区块体 hydrate 真实共识字段。
- **`WORKER_NODE_NOT_USABLE`**：窗口扫描子进程用 `tn10` 比对节点的 `testnet-10`，恒不相等。
- **创世被瞬时 REST 404 永久丢弃**：新交易广播后 REST 尚未入库，原逻辑直接标记 `IGNORED` 且不再重试。
- **1-DAA 差异死循环**：`acceptingDaa` 比 `notificationDaa` 大 1 属正常合并，原逻辑判为重组并丢弃结果。
- **checkpoint 断档**：checkpoint 走串行队列，被长任务阻塞后 `fastStart()` 全部失效，花费一律退化为慢路径。

---

## [0.1.2] - 2026-10-16

### Changed
- **Zero-SDK 纯 JavaScript 架构**：移除 12MB 的 `kaspa_bg.wasm`，常驻内存由 ~80MB 降至 ~28–38MB RSS。
- 新增纯 JS CashAddr / Bech32 编解码（`src/address.mjs`），P2PK (v0) 与 P2SH (v8) 与官方 SDK 双向一致。
- 新增原生 WebSocket wRPC JSON 客户端（`src/rpc.mjs`），直连节点 `/wrpc/json`，支持请求追踪与
  `VirtualChainChanged` / `UtxosChanged` 订阅。
- 节点池支持并发 RTT 测速、滞后剔除，并按 `p2pId` 强制后端隔离，形成主备自愈换血。

### Removed
- `sdk/` 目录（12MB WASM 二进制）与全部 SDK 依赖。

---

## [0.1.1] - 2026-10-15

### Added
- 官方 REST 零信任线索探针，形成四级花费者 / 创世定位漏斗。
- 通用任意阶段交易回溯器 `POST /admin/api/track` 与 Web Admin 诊断看板。
- 自包含封装，消除对外部仓库的路径依赖。

### Changed
- 主契约升级为 **Kaswin V2.0.2**（`ed0387c9`，80B KW21 规范命名布局）。

---

## [0.1.0] - 2026-10-13

### Added
- 首个可用版本：事件驱动索引、CID REST 服务、Admin 控制台。

---

## 历史版本清理说明

**2026-10-16 起，本项目只支持 Kaswin V2.0.2 单一合约版本。**

此前支持的 V2（`7aaf76fe` / `206d4ec7`）、V2.0.1（`78c11314`）、F3（`1dc41809`）、
F3.2（`7ca61d81`）等插件模块、`artifacts/` 产物与适配器均已删除。

数据库中由旧版本创建的轮次记录**保留为历史数据**，但因对应插件已不存在，
它们在 API 中统一归类为 `unknown`（fail-closed，不猜测其状态）。
