# Kaswin 合约 V2.0.2（已编译候选，VM NOT_RUN）

`open.sil`、`sealed.sil`、`refunding.sil` 已使用具名布局，单一来源为 `ledger-layout.json`。KW21、80B Header＋36B记录、8/6参数ABI及经济/随机/退出规则保持设计不变。2026-10-14授权编译：两次构建得到Profile `ed0387c9e77bb4e0a8f1f4654a326c316d83663f455283ec6a046a095c94856d`，三模板tail、样例构造bytecode、ABI与旧版一致；[结果与VM准备](../docs/V202-COMPILE.md)。候选独立保存在references，尚未批准或执行VM。

**artifacts/pins.json/profile.json 仍是 V2.0.1 历史产物，不匹配新源码，不得直接发布或将78c当成新Profile。** 源码哈希参与Profile计算；V2.0.2批准ID留空，核心src/lib暂不一致。新产物必须逆拓扑编译、审查后单独晋级，不手改旧hash。见[25问/14项/8项与验收边界](../docs/V202-LAYOUT-PREFLIGHT.md)。

源码投影与候选编译入口（本次build已调用只读投影检查，未使用--write）：

```bash
node scripts/contract/sync-layout.mjs --check
# 只有修改布局来源后才需要 --write；不会生成lib或合约字节码。
node scripts/contract/sync-layout.mjs --write
# 编译至全新目录，仅输出UNAPPROVED_CANDIDATE，不自动批准Profile。
node scripts/contract/build.mjs /absolute/silverc /absolute/new-candidate-dir
```

## 以下为 V2.0.1 历史产物记录

下列旧命令/路径按 `pre-layout` tag 解释；历史通过数不适用于V2.0.2源码，也不代表当前项目状态。当前历史证据范围见[VALIDATION](../docs/VALIDATION.md)。

# Kaswin 合约 V2.0.1（历史）

状态：**已编译、SCRIPT_VM 定向验证通过（33/33，严格结果校验，交易由 V2.0.1 核心库构建并经核心解释器交叉核对）；链外核心库已完成（[`scripts/core`](../scripts/core/README.md)）；尚未完成预算校准、网页/Indexer 集成和 TN10 验收，不可部署。** 依据[当前架构规范](../docs/ARCHITECTURE.md)。旧 Profile `7aaf76fe…` 已归档于 `archive/v2-7aaf`。

| 项目 | 值 |
|---|---|
| 合约版本 | V2.0.1 |
| Profile | `78c11314f95531345fb6f8614d4d628e2dbfac3429b7c9e37f4161a3a40907f7` |
| 编译器 | SilverScript v1.0.0 `3ed973335b59269293564805cc2c58a14595ec03`，binary SHA256 `81de9aa4…`（与当前 Profile 相同，未升级工具链） |
| 模板哈希 | OPEN `9f6792fc…`；SEALED `41029ed9…`；REFUNDING `0f3649d9…`（完整值见 `pins.json`） |
| 脚本大小（tail） | OPEN 3,701 B（原 5,764）；SEALED 3,830 B（原 6,479）；REFUNDING 9,888 B（原 9,880） |
| `budgetProfileId` | `null`：VM 预算矩阵尚未针对本版运行和评审 |

## 相对 V2（Profile `7aaf76fe…`）的变更

1. **输出上限收紧**：OPEN 2、SEALED 3、REFUNDING 34（手续费输出循环与入口检查同步）；输入上限仍为 9。当前链上允许、但钱包从不生成的额外辅助输出，本版不再接受。
2. **80B Header，magic `KW21`**：删除开奖中间字段（原 `[48,196)` 共 148B），ownerKey 移到 `[48,80)`，目录从 80 开始；长度 `80 + 36N`，最大 9,296B。4 条记录时 224B（PUSHDATA1），5 条起 PUSHDATA2。
3. **开奖在交易内计算**：SEALED 不再构造 phase 3/4 账本，seed、抽样、中奖票号都是局部值。seed 公式不变（`KASWIN_V2_DRAW`）；抽样前像保留原 8 字节零 counter（`KASWIN_V2_SAMPLE || seed || 0^8`），不重抽。超时转退款只把 phase 改为 5，不再写入锚点 DAA；成熟条件仍取被消费 SEALED 输入的 DAA。
4. **经济参数**：押金 4 TKAS（`400000000`）；每条购买记录退款扣 0.02 TKAS（`2000000`）。**最后一批退款**押金返还 3.98，扣下的 0.02 并入该批执行费池（池 = `(k+1) × 0.02`）；EMPTY、PAID 全额返还 4。登记付款 0.2 TKAS 是钱包/Indexer 发现政策，不在合约内（见下方“未完成”）。
5. **保留**：完整账本（退款只推进游标）、统一 spend 入口和参数顺序（OPEN 8 参数，SEALED/REFUNDING 6 参数）、actor 语义、开奖赏金 1 TKAS、费用上限 0.5、DRAW_DELAY 100、TIMEOUT_DELAY 432000 DAA、目录上限 256、每批退款最多 32。

## 文件

| 路径 | 内容 |
|---|---|
| `src/open.sil` `src/sealed.sil` `src/refunding.sil` | 合约模板（前向模板哈希为全零占位，只能经 `tools/build.mjs` 链接后编译） |
| `artifacts/*-linked.sil` `*-linked.json` `*-linked.args.json` `*.log` | 链接源码、编译产物、构造参数、编译日志 |
| `artifacts/build-report.json` `pins.json` `profile.json` | 构建报告与全部哈希 pin |
| `artifacts/vm/results.jsonl` `summary.json` | SCRIPT_VM 定向验证结果 |
| `tools/bundle.mjs` | 逆拓扑链接、模板标记检查、bundle 全量 pin 校验加载 |
| `tools/build.mjs` | REFUNDING → SEALED → OPEN 编译，写入新目录 |
| `tools/vm-check.mjs` | SCRIPT_VM 定向正负例（交易由 `scripts/V2.0.1` 的 `buildAction` 生成，负例在其基础上篡改；每例同时跑 `interpretAccepted`） |

## 复现

```bash
# 编译（输出到新目录；与仓库内 artifacts/pins 逐字节相同即可复现）
node contracts/V2.0.1/tools/build.mjs /absolute/silverc /absolute/new-dir
# VM（需要固定 harness，见 contracts/f3.2/artifacts/budget/vm-binary-review.json；在 unshare -n 断网命名空间内执行）
node contracts/V2.0.1/tools/vm-check.mjs /absolute/vm-binary-review.json
```

2026-10-10 两次独立编译结果逐字节相同。

## 验证（V 层：SCRIPT_VM）

固定 harness（`a9e20394…`，rusty-kaspa `cfafeb4`），公开测试密钥（标量 1），断网执行。**33/33 符合预期**（2026-10-11 重跑，`KASWIN_V201_VM_CHECK_3`）：每个结果都经 `tests/audit/vm-result.mjs` 严格校验（schema/名称/Profile/scope、预算数组与交易一致、共识单位上限）；负例必须在**输入 0（合约状态输入）**以 `VerifyError` 被拒，基础设施错误、资金输入签名失败或预算耗尽都不算通过；核心解释器的拒绝代码也逐例固定。

| 范围 | 正例（ACCEPT） | 负例（REJECT） |
|---|---|---|
| OPEN | BUY（0/4/255 条记录，覆盖 PUSHDATA1→2 与最大目录）；CLOSE 空轮、→SEALED（3/256）、→REFUNDING | BUY 3 个输出；origin 与 CID 不符；空轮押金少 1 sompi |
| SEALED | DRAW（3/256 条）；TIMEOUT（3/256 条） | 中奖记录 hint 错误；DRAW 4 个输出；派奖押金只给 3.98；超时后继改动游标 |
| REFUNDING | 只有 1 条（带/不带执行者输入）；33 条中间批与最后一批；256 条首批、最后一批（带/不带执行者输入）；多票记录 | 最后一批押金给 4.00 或 3.97；买家少 1 sompi；执行者多拿 1 sompi（只证明 fee 守恒绑定，核心 `FEE_MISMATCH`）；**金额与 fee 不变、只换执行者收款公钥**；**只换买家退款公钥**（两者核心 `PAYOUT_MISMATCH`）；中间批扣押金 |

另核对：同一输入下 V2.0.1 核心库开奖（`drawWinner`）与 V2 核心库原开奖中奖票号相同；`neg-DRAW-wrong-winner-hint` 被拒、正确 hint 被接受，说明 VM 中的开奖结果与核心库一致。每个用例的核心解释器结论与 VM 一致（正例接受、负例拒绝），预算分两项分别报告：`withinCommittedBudget`（单位 ≤ 9999 + 10000 × 交易所带预算，即 VM 实际执行的上限）与 `withinFormula`（单位 ≤ `P.actionUnits` 暂定公式），全部正例两项都满足；两者都不是校准矩阵。

各正例的交易大小与 storage mass 记在 `artifacts/vm/results.jsonl`（按 `apps/kaswin-v2/scripts/shared/mass.mjs` 计算，S 层）。例：BUY 首张票交易约 4,280 B（原 6,491 B），只剩 1 条的最后一批退款带执行者输入时约 10,482 B、storage mass 约 7,500；不带执行者输入时 storage mass 约 405,000（执行者小额输出，接近 500,000 上限），钱包应带执行者输入。

## 未完成（部署前必须完成）

- **网页 / Indexer**：核心库已在 `scripts/V2.0.1`；网页仍只加载 V2 核心（`scripts/f3.2`）。Indexer V2.0.1 插件 `kaswin-v2.0.1@78c11314f9553134`（Genesis 80B、登记 0.2，固定本 commit 的 bundle 与核心）已于 2026-10-10 在 .201 启用，与 V2 插件并存；VPS 未部署；TN10 上还没有 V2.0.1 轮次，插件尚未处理过真实交易。旧 Profile 保持原样。
- **VM 预算校准**：2026-10-13 已跑全矩阵（`tests/audit/23-v201-vm-budget-calibration.mjs`，2,549/2,549，0 超预算、0 超公式，最小余量 14.62%）并回放 861 笔 TN10 真实交易（`24-v201-tn10-vm-replay.mjs`，全部在预算内），公式无需改动，见[记录](../docs/VM-BUDGET.md)与 `tests/contracts/evidence/budget/`。候选证据审查与 `budgetProfileId` 写入随网页构建门一起完成，当前仍为 `null`。
- **完整交易验证与 TN10**：mass/relay/节点接受、真实 SeqCommit、真实钱包签名、新轮登记与新旧 Indexer 并存，均未执行；链上实验需单独授权。
- **独立审查**：本版源码和测试是同一助手编写与自查，不是独立审计。

## 架构关卡

本版不改变对象、身份、拓扑或授权模型，25 问 / 14 项材料 / 8 项 anti-pattern 见[架构规范](../docs/ARCHITECTURE.md)。实现后核对结果：

- 身份（1–3）：OPEN 的 `genesisLedger()` 按 80B 重建零票初态（magic、phase 1、配置、12 字节零、ownerKey），CID 仍按 KIP-20 由 origin 与 genesis 输出推导；核心库 `S.rootId(origin, S.rootScript(...))` 与链上重建一致（所有 OPEN 正例 ACCEPT；`neg-BUY-wrong-origin-cid` 被拒）。
- 后继（10–13）：三个模板的 `successor()` 均保留完整 P2SH 脚本比对、模板哈希、`OpAuth*`/`OpCov*` 计数与 CID 绑定；TIMEOUT 后继必须是“只改 phase 的同一账本”（负例覆盖）。
- 守恒（14–15）：`checkFee` 仍要求 `totalIn − totalOut == fee`；退款后继金额 = 输入 − 本批原始票款；押金只在最后一批扣固定 0.02（负例覆盖 4.00 / 3.97 / 中间批）。
- 随机性：seed 输入与域分隔不变，抽样前像保留零 counter，拒绝采样不变；JS 对照新旧开奖票号一致。
- 退出（19–20）：EMPTY / PAID / REFUNDED 三终局和超时路径保持；最后一批执行费池最少 0.04。
- anti-pattern：无新增全局状态；未删除任何 successor 校验；CID 不代替转换校验；Indexer 不参与裁决。
