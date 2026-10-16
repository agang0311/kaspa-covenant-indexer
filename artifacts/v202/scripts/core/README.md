> 目录已重排；本轮未测试/构建。以下历史命令/路径以 `pre-layout` tag 为准，当前命令见[根README](../../README.md)与[迁移记录](../../docs/LAYOUT-MIGRATION.md)。

# 合约 V2.0.2 链外核心库（源码候选）

当前 `src` 的布局集中在 `ledger-layout.ts`（投影自 `contracts/ledger-layout.json`）；状态访问和seed冻结切片采用具名偏移，`HEADER` 改为 `HEADER_BYTES`。CONTRACT_VERSION已设V2.0.2，APPROVED_PROFILE_ID留空并以PROFILE_NOT_APPROVED拒绝运行时使用。`lib`仍保留V2.0.1历史产物，**未重建、不对应当前src**；包version只表示源码目标，不表示可执行产物已升级。新Profile必须编译与评审，不能将历史78c改标签复用。见[实施Preflight](../../docs/V202-LAYOUT-PREFLIGHT.md)。

## 以下为 V2.0.1 历史说明（不作为V2.0.2验证）

这是 [合约 V2.0.1](../../contracts/README.md) 的链外核心：账本编解码、交易构建、accepted 解释、开奖、Genesis 发现与浏览器持久化。不是链上合约、通用 SDK 或钱包。`src/*.ts` 是源码，`lib/` 由固定 TypeScript 5.8.3 生成，**不要直接编辑 lib**。

与 V2（`scripts/f3.2`，Profile `7aaf76fe…`）完全分开：两套 codec 不能混用，一个 Profile 只对应一套核心。`assertCompatibleProfile`仅接受固定 `78c11314f95531345fb6f8614d4d628e2dbfac3429b7c9e37f4161a3a40907f7`，重新核验实际tail、完整Profile ID及编译器，不相信调用者提供的version/id标签。新增Profile必须显式审查并更新核心；此兼容批准不代表生产/预算批准。

## 相对 V2 核心的变化

| 模块 | 变化 |
|---|---|
| `state` | 80B Header（magic `KW21`），`Ledger` 只有 phase/config/sold/purchaseCount/cursor/ownerKey/directory；`Phase` 只有 OPEN=1、SEALED=2、REFUNDING=5；押金 4 TKAS、退款扣费 0.02 TKAS；`OUTPUT_LIMIT` 2/3/34；新增 `paidTickets` |
| `protocol` | `authenticateDraw` 返回独立 `Draw`（seed、边界、锚点、目标块），不再伪装成账本；`sample(seed, sold)` 用 8 字节零 counter；新增 `drawWinner`、`recordOfTicket`、`refundBatch`；`timeoutDaa` 只取被消费 SEALED 输入的 DAA；超时后继只改 phase；最后一批退款押金 3.98、执行费池 `(k+1)×0.02`；`transition` 检查输出上限 |
| `accepted` | 按模板检查输出上限；开奖用 `drawWinner` 并要求 hint 等于中奖记录；TIMEOUT 后继只改 phase，锚点 DAA 作为派生值 `timeoutAnchorDaa` 返回；最后一批退款按 3.98 / 池 `(k+1)×0.02` 核对；`draw` 附带 `anchorDaa` 与 `seed` |
| `registry` | 登记付款 0.2 TKAS（`20000000`） |
| `builders` / `genesis-discovery` | 创建/发现/消费前强制Profile兼容检查；Genesis 公告 = `KASWIN_GENESIS_V2` ‖ profileId ‖ 80B 账本 |
| `artifacts` / `state` | `loadTrustedProfile`、`makeProfile`及公开脚本生成/snapshot入口拒绝旧或篡改Profile；不按可变对象身份缓存校验结果。`parseCompiledFrame`、`profileId`、`scriptFor`只是底层解析/计算工具，不能单独用作交易批准 |
| `pass-a` | P/T选中链检查与普通mergeset payload成员读取分离；后者不依赖其first-parent/selected-parent次序。保留SeqCommit/payload/context核验，强化P/T多次读取的header字段一致性 |
| 其它 | `bytes`、`hashes`、`blake3`、`covenant-id`、`transaction`、`persistence` 与 V2 相同 |

## 状态

- 预算：`actionUnits` 暂时沿用 V2 已评审公式作为上界（V2.0.1 脚本更小，现有 V2.0.1 定向 VM 实测都低于公式）；完整 V2.0.1 校准矩阵后需要重新拟合，届时才设置 `budgetProfileId`。
- 尚未接入网页与 Indexer。

```bash
npm run check:core          # lib 与 src 的固定编译输出逐字节一致
node web/build/check-core.mjs --write    # 修改 src 后重新生成 scripts/core/lib
npm test                    # 核心库与单元测试
```

## 修复验证（2026-10-11，S/L）

[当前架构记录](../../docs/ARCHITECTURE.md)与 [VALIDATION](../../docs/VALIDATION.md)：核心22/22，其中7项新增测试覆盖真实旧artifact混用、伪造id与校验后mutation、合成完整PASS-A及具体错误拒绝；原15项继续通过。14→42生成文件逐字节一致。V2.0.1既有runner离线回归4/4；未重新运行VM、连接节点、签名广播或重建网页。旧V2继承的PASS-A问题不在此增量修复范围内。

## 历史验证（2026-10-10，不代表本轮重跑VM）

- 核心库测试 15/15（S/L）：80B 偏移与往返、拒绝 V2 账本与 phase 3/4、PUSHDATA1/2 分界、CID 重建、开奖 seed 与票号与 V2 核心相同、单次抽样、超时成熟与只改 phase、各动作的输出上限、DRAW 3 输出与 hint、退款 1/31/32/33 条与游标 32/224 的批次/押金/执行费池、最小池 0.04 覆盖约 0.021 网络费、Genesis 80B 公告与 0.2 登记、版本隔离。
- 与合约一起的 VM 验证（V）：[`tests/contracts/vm-check.mjs`](../../tests/contracts/vm-check.mjs) 用本库 `buildAction` 构建全部交易，33/33 VM 结论符合预期，且每例 `interpretAccepted` 与 VM 一致。
