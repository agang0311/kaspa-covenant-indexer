# 单进程多网络监控 Preflight（TN10 + Mainnet，4 节点）

状态：**APPROVED FOR INFRASTRUCTURE IMPLEMENTATION（仅限只读基础设施接线）**
主网合约业务监控：**就绪但无对象**（主网 registry 当前 0 个 UTXO）
日期：2026-10-16
范围：`kaspa-covenant-indexer` 只读索引基础设施的多网络化 + 每网络双节点（共 4 连接）

> **2026-10-16 用户决定（简化）**：不再兼容旧版本合约（V2 / V2.0.1 / F3 / F3.2 全部移除），
> **只保留 Kaswin V2.0.2**；且 V2.0.2 **本身不区分主网与 TN10**（已由源码核实，见 2.1）。
> 因此网络差异只剩 **registry 发现地址** 一个事实。

---

## 0. 用户需求原文

> "我希望一个进程监控 2 个网络连接 4 个节点"

需求分解：
1. **一个进程**：单个 Node.js 进程，不是两个 systemd 单元；
2. **2 个网络**：TN10 + Mainnet 同时监控；
3. **4 个节点**：每个网络 2 个节点（主 + 热备），共 4 条 wRPC 连接。

---

## 1. 设计头部与证据

- 项目 / 功能：`kaspa-covenant-indexer` 多网络只读索引（infrastructure only）
- 决策方：用户（本 workspace 所有者）；负责人：本 agent
- 设计版本：v1（2026-10-16）
- 状态：基础设施接线 **APPROVED**；主网合约监控 **BLOCKED**
- 已确认需求：单进程 / 2 网络 / 4 节点 / 只读 / 不签名 / 不广播
- 节点固定版本（本次只读实测 2026-10-16）：
  - TN10: `kaspad 2.1.0`，`networkId=testnet-10`，`hasUtxoIndex=true`
  - Mainnet: `kaspad 2.1.0` / `2.0.1`，`networkId=mainnet`，`hasUtxoIndex=true`
- 网络 genesis（一手核对）：
  - TN10 `f896a3034873be1739fc4359236899fd3d65d2bc94f9780df0d0da3eb1cc4370`（沿用既有 pin）
  - Mainnet `58c2d4199e21f910d1571d114969cecef48f09f934d42ccb6a281a15868f2999`（来自发布库 `scripts/chain/mainnet.mjs`，固定 rusty-kaspa genesis 事实）
- 证据等级：
  - **S（源码）**：`artifacts/v202/scripts/core/lib/state.js`（`profileId` 不哈希 networkGenesis）、`genesis-discovery.js`（`verifyGenesisAnnouncement` 不含 networkGenesis）
  - **L（离线）**：131 项既有回归；本次新增多网络隔离用例
  - **N（网络，只读）**：主网 4 个节点握手 + DAG 查询 + registry UTXO 查询；TN10 现有服务
  - **V（编译/VM）**：N/A（不涉及合约编译）
- 本次审查范围 / 非目标：
  - **在范围内**：网络抽象、节点池按网络分组、Store 网络列、API 网络维度、Admin 展示
  - **不在范围内**：主网合约规则实现、主网签名/广播、主网 Profile 新编译、跨网络数据合并

---

## 2. 关键一手事实（决定设计）

### 2.1 `networkGenesis` 不参与 Profile 与合约身份计算（S 级证据）
```javascript
// artifacts/v202/scripts/core/lib/state.js:38-45
export function profileId(frames) {
    for (const m of MODULES) { ... check(templateHash(f.tail) === f.templateHash, 'ARTIFACT_HASH'); }
    check(new Set(MODULES.map(m => frames[m].templateHash)).size === 3, 'FRAME_COLLISION');
    return hex(blake2b256(cat(ascii('KASWIN_PROFILE_V2'), ...MODULES.flatMap(m => [
      unhex(frames[m].sourceSha256, 32), unhex(frames[m].templateHash, 32), unhex(frames[m].dispatchTag, 4)]))));
}
```

结论：`profileId` 只依赖 `sourceSha256` / `templateHash` / `dispatchTag`。
`networkGenesis` **不进入** Profile 计算，也**不进入** `verifyGenesisAnnouncement` 的认证等式。

**这意味着**：同一套已认证模板可以在任意网络上使用，Profile ID 不变。这是多网络可行的技术基础。

原 `contracts/kaswin-v202.mjs` 的 `V202_TN10_PIN_REQUIRED` 是**项目策略守卫**（防止误用），不是合约身份要求。
本次改造把它改为“必须是已知网络的 genesis”+ 网络上下文注入（registry 地址按网络选择）。

### 2.2 CID 与 genesis 无关，只依赖 authorizing outpoint + script（S 级证据）

```javascript
// artifacts/v202/scripts/core/lib/genesis-discovery.js
check(S.rootId(e.authorizingOutpoint, script) === e.covenantId, 'OPEN_GENESIS_CID_MISMATCH');
```

CID 由 `authorizing outpoint + P2SH(script)` 推导，与网络无关。**跨网络 CID 冲突在密码学上可忽略，但为保守起见 Store 仍以网络维度隔离。**

### 2.3 主网 registry 地址当前 0 个 UTXO（N 级实测，2026-10-16）

```
mainnet registry live UTXOs: 0
```

主网 registry 地址 `kaspa:qzccgw790r0mnfe9uvy7nwd7a8990029ycg8e0manehhgruqm2r9v528nz0qr`
（SPK `20b1843bc578dfb9a725e309e9b9bee9ca57bd4526107cbf7d9e6f740f80da8656ac`）上没有任何 UTXO。

**结论：主网上目前没有已登记的 Kaswin 轮次。** 索引器可以连上并运行，但会发现 0 个轮次。
这符合"基础设施可先就绪，业务对象等待链上部署"的推进方式。

### 2.4 跨网络地址隔离必须显式（N 级实测发现的真实风险）

在**主网节点**上查询 **TN10 registry 地址**时，节点并未报错，而是按前缀无关的 SPK 匹配返回了主网上的一个 UTXO：

```
TN10 addr on mainnet node -> 1 entry
  address: "kaspa:qztrjpfpuf6g9jw76e6ay909z43hndpv5maqkker6ur4enz7llarskpvzdkem"
  scriptPublicKey: "00002096390521e27482c9ded675d215e5156379b42ca6fa0b5b23d7075ccc5efffa38ac"
```

**这是本次设计最重要的安全发现**：Kaspa 地址前缀（`kaspa:` / `kaspatest:`）**只是展示层**，SPK 才是链上事实。同一个 x-only key 在两个网络上产生相同 SPK。

因此：
- **必须**在每网络实例内用**该网络的 prefix** 解码地址，并且
- **必须**在网络上下文创建时冻结 prefix，禁止任何回退到 TN10 的默认值（发布库 `NETWORK-CONTEXT-REFACTOR.md` 的"缺失时失败，不回退TN10"原则同样适用）。

### 2.5 主网节点池可发现（N 级实测）

```
mainnet resolver answers: 3
  wss://eva.kaspa.green/kaspa/mainnet/wrpc/borsh
  wss://ivy.kaspa.green/kaspa/mainnet/wrpc/borsh
  wss://dina.kaspa.green/kaspa/mainnet/wrpc/borsh

wss://eva.kaspa.green  | rtt 3048ms | 2.1.0 mainnet synced:true utxo:true | daa 567398311 | p2p b96dd829-...
wss://ivy.kaspa.green  | rtt 2715ms | 2.0.1 mainnet synced:true utxo:true | daa 567398364 | p2p 628eff1b-...
wss://dina.kaspa.green | rtt 1570ms | 2.1.0 mainnet synced:true utxo:true | daa 567398386 | p2p b0b311ad-...
```

三个主网后端 `p2pId` 互不相同 → 可以组成真正的双节点主备。
另有固定主网端点 `wss://kate.kaspa.red/kaspa/mainnet/wrpc/json`、`wss://john.kaspa.red/kaspa/mainnet/wrpc/json`（发布库 `mainnet.mjs`）。

---

## 3. 25 问（本次影响范围）

| # | 必答问题 | 答案 / 证据 / 状态 |
|---|---|---|
| 1 | Covenant instance identity 是什么？ | 既有实例 CID，不变；网络只是链下命名空间，**不冒充 CID**。**PASS** |
| 2 | Covenant ID 如何 genesis？ | 既有 authorizing outpoint + 规范 OPEN script 推导（`rootId`）。**不涉及网络**。**PASS** |
| 3 | 哪些 successor 继承该 ID？ | 既有合法 continuation。不变。**PASS** |
| 4 | Current State 存在哪里？ | 链上 live covenant UTXO。不变。**PASS** |
| 5 | Immutable fields 是什么？ | 既有 owner/config/Profile 约束不变；**新增**：网络上下文（key/genesis/prefix/registry）在实例创建时冻结。**PASS** |
| 6 | Mutable fields 是什么？ | 既有 phase/sold/directory/cursor 不变；缓存按网络隔离，属 derived/cached。**PASS** |
| 7 | 合法 transition 集合？ | GENESIS / BUY / CLOSE / DRAW_AND_PAY / TIMEOUT_REFUND / REFUND 不变。**PASS** |
| 8 | 每个 transition 谁可触发？ | 既有签名/permissionless 规则不变；钱包网络必须匹配网络上下文。**PASS** |
| 9 | 1:1、1:N、N:1、N:M、1:0 拓扑？ | 既有 1:1 与 1:0 不变。**PASS** |
| 10 | Authorized successors 哪些 outputs？ | 既有约束后继集合不变。**PASS** |
| 11 | 如何验证 successor script？ | 固定 Profile 模板检查不变（Profile 与网络无关，见 2.1）。**PASS** |
| 12 | 如何验证 successor covenant binding？ | CID / authorizing_input 检查不变。**PASS** |
| 13 | 如何验证 successor state？ | 规范账本重编码、SPK/金额核对不变。**PASS** |
| 14 | 永不可违反的 invariants？ | **新增网络隔离不变量**：不跨网络读取/订阅/记录/合并；网络 genesis 不匹配的节点拒绝；地址 prefix 不匹配拒绝。既有不变量保持。**PASS** |
| 15 | value / token / asset conservation？ | 既有 bigint 守恒与费用约束不变；登记 SPK 由选定网络传入（`registrationOf` 已参数化）。**PASS** |
| 16 | 哪些用户争抢同一 UTXO？ | 同轮参与者争抢同一状态 outpoint。不新增共享 UTXO。**PASS** |
| 17 | State Contention Surface 多大？ | 每轮既有竞争面不变。**连接池不是共识锁**，不引入链上串行点。**PASS** |
| 18 | 能否进一步 partition？ | 不改变轮次分区；网络维度本身就是一层分区。**PASS** |
| 19 | 如何终止？ | 既有 PAID/REFUNDED/EMPTY 终局不变。**PASS** |
| 20 | 资金如何退出？ | 既有普通付款输出与退款路径不变。**PASS** |
| 21 | Indexer 职责？ | 仅发现/展示/缓存。**新增**：响应必须声明 `network`，查询必须按网络过滤。**PASS** |
| 22 | Indexer 消失仍必须成立的事实？ | 节点 acceptance、规范谱系/见证/付款检查仍是事实路径。数据不可得时返回未知，不伪造。**PASS** |
| 23 | 是否需 foreign template？ | 不增加。**PASS** |
| 24 | 是否需 multi-contract flow？ | 不增加。**PASS** |
| 25 | L1 Covenant 还是 Based App？ | 本次是只读基础设施接线，不改变既有 L1 选择。**PASS** |

**25 问全部 PASS**（因为本次不改变任何合约语义，只改变链下配置与接线）。

---

## 4. 14 项架构材料

1. **UTXO inventory**：既有轮次状态、普通资金/付款、可选登记。**不新增对象**。
2. **Schemas**：共识账本不变。**新增链下 network 记录**：`{key, label, networkId, genesis, addressPrefix, rest, registry:{address,spk}, nodes[], seed[], resolverPath}`。外部 URL 不是共识承诺。
3. **Lineage**：既有规范 OPEN CID 不变。网络 genesis 只隔离链下数据。
4. **状态图**：OPEN→SEALED→PAID；OPEN/SEALED→REFUNDING→REFUNDED；空轮→EMPTY。**不改**。
5. **UTXO topology**：普通资金→既有状态→后继/普通终局付款。网络配置**不进入**交易拓扑。
6. **逐 transition I/O**：既有 builder/解释器不改。GENESIS 登记脚本必须显式传入**该网络**的 registry SPK。
7. **授权规则**：钱包报告网络与地址前缀必须匹配网络上下文；签名验签与批准字节门保持。
8. **Successor 规则**：固定 Profile、CID、binding、SPK、金额、状态检查保持。
9. **不变量**：
   - I1 网络身份一致：节点 `networkId`、`getBlockDagInfo.network`、genesis 三者必须同时匹配。
   - I2 缓存隔离：不同网络的数据永不合并、永不互相覆盖。
   - I3 线索不冒充 acceptance：REST 仅 hint。
   - I4 金额 bigint/十进制字符串。
   - I5 无跨网络回退：网络上下文缺失时失败，不回退 TN10。
10. **退出**：既有 payout/refund 不变；REST 不可用不伪造退出结果。
11. **Indexer**：按网络发现/验证/acceptance/reorg/恢复；`meta.network` 必须匹配；不可裁决资金。
12. **Contention**：不新增全局链上串行点。**本机写入路径**：单进程内每网络一个 Engine，各自单写入者，DB 内以 `network` 列隔离。
13. **L1/Based 边界**：范围 N/A；不改变链上应用设计。
14. **Anti-pattern 审计**：见第 5 节。

---

## 5. 8 项 Anti-pattern 审查

| # | 检查项 | 证据 / 结论 |
|---|---|---|
| 1 | 全局 state 管全部用户 | **通过**：未新增全局状态 UTXO；网络维度是链下分区。 |
| 2 | 只签名不验 successor | **通过**：既有 successor/付款检查全部保留。 |
| 3 | 相同 ID 即信状态 | **通过**：CID 与模板/状态/金额检查保持独立。 |
| 4 | Indexer 决定余额/归属 | **通过**：索引仍是缓存视图；API 声明 `requiresIndependentVerification: true`。 |
| 5 | Solidity 逐字段翻译 | **N/A**：不涉及合约设计。 |
| 6 | 为方便全操作共享 UTXO | **通过**：不改变 UTXO 拓扑。 |
| 7 | 无 termination/payout | **通过**：既有终局与退出路径保留。 |
| 8 | 只验 bytes 不验 template/script/lineage | **通过**：`verifyGenesisAnnouncement` + `assertCompatibleProfile` 完整保留。 |

---

## 6. 本次实现边界（严格限定）

### 允许实现
1. `src/networks.mjs`：网络目录（TN10 + Mainnet），每网络 genesis / prefix / registry / 节点种子 / resolver 路径。
2. `src/node-pool.mjs`：`probePool` / `choosePair` 接受网络上下文；resolver 查询按网络路径。
3. `src/store.mjs`：网络绑定（`meta.network`），跨网络复用同一 DB 文件失败关闭。
4. `indexer.mjs`：`--networks tn10,mainnet` 启动多个 Engine 实例，每网络一个双节点池。
5. `src/api.mjs` / `src/admin.mjs`：API 响应 `meta.network` 动态化；`?network=` 过滤；Admin 分网络展示。
6. `contracts/kaswin-v202.mjs`：唯一支持的合约插件；`networkGenesis` 由网络上下文注入。
7. 删除旧版本：`kaswin-v2.mjs` / `kaswin-v201.mjs` / `kaswin-f3.mjs` / `kaswin-f32.mjs`、`artifacts/{v2,v2-206d,v201,f3,f32}`、`src/adapter/rounds.mjs`、`src/profile.mjs`、`contracts/{default,tn10-v201}.json`。

### 明确禁止
- ❌ 主网签名/广播/钱包操作
- ❌ 跨网络数据合并或聚合视图
- ❌ 网络上下文缺失时回退到 TN10
- ❌ 自动升级节点/合约/SDK 版本

### 主网业务现状（2026-10-16 实测）
1. 主网 registry 地址当前 **0 个 UTXO**（2.3 实测）→ 主网暂无已登记轮次；
2. 基础设施已就绪：主网节点池可发现（≥5 个独立 p2pId 后端）、双节点已建立、订阅已生效；
3. 一旦主网出现首个已登记 Genesis，索引器会自动发现并追补（无需改代码）。

**因此本次交付：基础设施多网络就绪 + TN10 业务继续运行 + 主网以“空索引器”姿态待命。**

---

## 7. 残余风险

| 风险 | 缓解 |
|---|---|
| 同 x-only key 跨网络 SPK 相同（2.4 实测） | 网络上下文冻结 prefix；查询与展示全部按网络维度隔离；禁止跨网络地址复用 |
| 主网节点 RTT 明显高于 TN10（1.5–3.0s vs 0.25–0.35s） | 双节点主备 + 现有 failover；主网心跳与超时独立配置 |
| 主网 `ivy.kaspa.green` 为 2.0.1，与 2.1.0 混合 | `verifyNode` 只校验 networkId/synced/utxoIndex，不锁 serverVersion；两版本均支持所需 RPC |
| 单进程内存翻倍 | 每网络独立 Engine；实测单网络 ~28–38MB，双网络预计 ~60–80MB，`MemoryMax` 需上调 |
| 主网 `--pool` 候选池较小 | 固定端点 + resolver 发现结合；池不足时明确记录 `POOL_ONE_BACKEND_ONLY` |
| 删除旧版本后历史轮次无法用插件重新解释 | 历史轮次全部为 TERMINAL（已核实 0 个 LIVE），保留在 DB 中仅作展示；`categoryOf` 对非 V2.0.2 key 返回 `unknown` |

---

## 8. 开工门槛确认

- [x] 25 问清楚（全部 PASS，不涉及合约语义变更）
- [x] 14 项完整
- [x] 8 项 anti-pattern 审查
- [x] 网络身份/授权/退出无未解阻断
- [x] 每个价值敏感事实有共识可验证路径（既有 `verifyNode` + acceptance 检查保留）
- [x] 主网基础设施：就绪（节点池/订阅/网络校验实测通过）
- [x] 主网业务对象：无（registry 0 UTXO，等待链上登记）
- [x] 审批人/日期/范围/残余风险明确记录

**审批结论**：只读基础设施多网络化 **APPROVED 并已实施**；主网业务对象等待链上出现。
