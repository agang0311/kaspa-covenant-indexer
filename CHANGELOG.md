# Changelog

本文件记录本项目的版本变更。格式参考 [Keep a Changelog](https://keepachangelog.com/)，
版本号遵循 [Semantic Versioning](https://semver.org/)。

README 只描述**当前**能力与用法；历史变更一律记录在此。

---

## [Unreleased]

### Added
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

### Fixed
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
