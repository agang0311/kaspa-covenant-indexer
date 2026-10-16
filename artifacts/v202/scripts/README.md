# 共享操作与核心脚本

当前源码目标 **V2.0.2**；目录按职责，不按版本。运行时通过 `contract/runtime.mjs` 核对core版本、pins版本、批准Profile与实际内容，不升级工具pin或放行发布门。2026-10-15按[项目约定](../项目约定.md)落实链下流程优化，接口、L层回归及未验收边界见[实施记录](../docs/OPERATION-FLOW-IMPLEMENTATION.md)。历史阶段和当时的NOT_RUN分别见[整改记录](../docs/V202-SCRIPTS-REMEDIATION.md)、[阶段3–4](../docs/STAGE3-4.md)与[阶段5](../docs/STAGE5.md)，不能用旧阶段说明覆盖当前实施结果。

- `core/src` / `core/lib`：协议编码、builder和accepted规则；本轮不改源码或生成库。
- `contract/`：构建/bundle/布局同步为Node专用；`runtime.mjs`是浏览器安全的版本/Profile门，不加载Node模块。
- `chain/nodes.mjs`：唯一JSON RPC与节点连接/取消/fee机制；NetworkRpc/NetworkLink只配置预算和网络身份策略；不保留network-link转发文件。
- `chain/chain.mjs`：UTXO读取/存活/完整输入与接受字段检查；`network-context`只注入网络地址/DAA策略。
- `round/operation-engine.mjs`：公共builder/quote与批准核验→签名/验签→会话记录/显式持久化策略；不强制额外签前/后live RPC。
- `round/authorized-operation.mjs`：CLI同步批准检查、普通签名/验签、精确内容与强制Journal；接收已知查询起点，不加载钱包文件。
- `round/engine.mjs`：网页计划、批准、KasWare、CAS、RBF、对账/恢复编排；不保留engine2/EngineV2旧接口。
- `round/record-verifier.mjs` / `passa.mjs`：共享接受解释/回执与PASS-A采集/中奖计算。
- `store/intent-store.mjs`：显式required/optional策略，CAS与同步Journal分别适配；`session-store.mjs`提供可选IndexedDB及故障时的会话记录。实验文件权限/fsync仍在Node专用`journal.mjs`，失败停发。
- `tracking/diagnosis.mjs` / `signed-record.mjs`：公共诊断与完整签名字节门；tracker为Web策略，reorg-guard为CLI谱系/settlement策略，不互相扩大授权。
- `wallet/`：KasWare与本地签名/密码学；文件钱包加载仍只在CLI入口。
- `release-safety.mjs`：交易发布门，不因共享机制放开只读候选。

运行时代码不得反向依赖tests/web。tests/flows保留case、预选资金与交错调度，通过公共操作内核/适配器工作；模拟签名/接受不是真实链证据。网页仍TN10，公共主网配置不等于网页支持主网。
