# KCI Rust 实现

本目录将索引器迁移为原生 Rust 可执行程序 `kci`。运行时不需要 Node.js、Kaspa SDK 或 WASM；管理页面和固定 V2.0.2 产物嵌入二进制。SilverScript 在线编译是可选功能，仍调用主机已有的 `silverc`。

仅支持 **Kaswin V2.0.2 / Profile `ed0387c9e77bb4e0…`**。已有数据库中的其他版本历史记录可以显示为 `unknown`，但不会被原生规则解释；若旧版本还有 LIVE 轮次，启动直接拒绝。历史回放统计仅针对 V2.0.2，不包含旧插件记录。

## 构建与运行

需要 Rust 2024 工具链；当前开发使用 Rust 1.98。保留仓库内 `artifacts/v202/`，编译时会嵌入并在加载规则时核验 SHA-256 和 Profile ID。

```bash
cd rust
cargo build --release --locked

# 一个显式节点，仅监控主网；默认 HTTP 端口 8789，绑定回环地址。
ADMIN_TOKEN='your-admin-token' ./target/release/kci run \
  --network mainnet --url ws://127.0.0.1:18110 \
  --db ./data/mainnet.sqlite --api-port 8789

# 两网，各自选取主备节点；数据库拆分为 events.tn10.sqlite/events.mainnet.sqlite。
ADMIN_TOKEN='your-admin-token' ./target/release/kci run \
  --networks tn10,mainnet --pool --db ./data/events.sqlite \
  --contracts ../contracts/v202.json --api-port 8789
```

`--url` / `--urls` 只允许单网络；未指定节点时自动使用节点池。`--rest-url off` 可禁用 REST 提示。节点池以实际握手身份核对网络，同一个 `p2pId` 的不同 URL 不算两个后端。初始节点选举、备用连接可能需要数十秒。

管理员令牌通过 `ADMIN_TOKEN` 或 `KASPA_INDEXER_ADMIN_TOKEN` 设置，也可复用首个网络数据库中的 `adminToken`。无现成令牌时生成并保存，但不写入日志。数据库及环境文件应仅对运行用户可读。

## 接口与管理

- `/tn10/rounds`、`/mainnet/rounds`、`/<network>/rounds/<CID或Genesis TXID>`。
- 同时支持 `/<network>/v1/rounds`、`/v1/<network>/rounds`、`/v1/rounds?network=...`。
- 筛选：`status=open|sealed|close|unknown`、`phase=1|2|5`、`terminal=true|false`；分页 `limit=1..200`、`cursor=<CID>`。
- `/v1/networks`、`/<network>/status`、`/admin` 以及状态、插件列表、节点探测均开放；CORS 为 `*`。
- `/track`、`/recheck-discoveries`、`/pool/*` 的修改操作、`/plugins/*` 的修改操作与编译均需管理员令牌。推荐 `Authorization: Bearer ...`。
- `/admin/api/track?network=mainnet` 支持任意阶段 TXID 回溯；REST 只提供位置提示，真正推进由节点接受实例和固定合约规则决定。禁用 REST 后，不会退回另一网络的 REST。
- SilverScript Studio 支持 AST 和完整编译。Rust 版不加载 JavaScript 插件，生成/安装 `.mjs` 返回 `NATIVE_PLUGINS_ONLY`；页面已移除相应安装入口及 V2.0.1 预设。
- 配置修改先验证所有网络，再按引擎队列加载。不能移除仍有 LIVE 轮次依赖的原生插件。
- 可选 `--relay URL` / `--relay-file PATH` 发送已提交的 outbox，失败保留重试。服务端接收方须按 `type:roundId:seq` 去重。

`status --db ...` 和 `replay DB` 只读打开 SQLite。`contracts` 只加载并打印固定配置，不连接节点。

## 与 JS 实现的明确差异

| 范围 | Rust 行为 |
|---|---|
| 合约校验 | 端口、地址、KW21 账本、哈希、Genesis、BUY、CLOSE、DRAW_AND_PAY、TIMEOUT_REFUND、REFUND 使用原生实现 |
| 接受实例补全失败 | 中止本次处理；不回退至未经绑定的 REST/mempool 见证 |
| 区块头缺失/裁剪 | 保留待核验状态；记录年龄不能单独证明 FINAL |
| 接受实例读取 | 检查行头、DAA、blue score 与当前链状态，变化时失败重试 |
| RPC | 传输边界只允许查询与订阅方法，不存在签名/广播入口 |
| 窗口扫描 | Rust 进程内按页扫描，保存持久化游标；不启动 JS 子进程。分配释放不等于 OS 必然即时回收 RSS |
| 管理鉴权 | 所有会修改跟踪/配置状态的管理请求需令牌 |
| JS 自定义插件 | 不支持运行或安装；原生规则固定为 V2.0.2 |
| 内存指标 | Linux `/proc/self/status` 提供 RSS；JS 堆字段返回 null |

本实现仍是缓存观察视图，不是共识或付款凭证。`FINAL` 是索引器的 blue-score 策略标记。服务继续采用通知/手动跟踪发现，不在后台全链扫描或自动回填全部注册历史。启动时会对已有 LIVE/STALE 轮次执行 UTXO 对账。

## 本机并行部署

当前本机部署独立使用：

- systemd：`kci-rust-lab.service`
- 可执行文件：`/opt/kci-rust-lab/bin/kci`
- 数据和配置：`/var/lib/kci-rust-lab/`
- API / 控制台：`http://127.0.0.1:8789`、`http://127.0.0.1:8789/admin`

生产 JS 服务仍使用 8788。Rust 的初始数据来自一致性快照，后续各自更新；不要让两个实现同时写同一个 SQLite 文件。`.kci-lock` 防止两个 Rust 进程同时打开同库，但 JS 实现不会遵守该锁。

```bash
systemctl status kci-rust-lab.service
journalctl -u kci-rust-lab.service -n 30 --no-pager
curl http://127.0.0.1:8789/mainnet/status
```

示例单元见 [deploy/kci-rust-lab.service](deploy/kci-rust-lab.service)。本机部署未替换公开域名、Cloudflare 网关或其他主机上的 JS 服务，也未执行 git commit / push。

## 验证命令及证据边界

仅在明确要求验证时执行：

```bash
cargo test --locked
./target/release/kci replay /path/to/events.tn10.sqlite
./target/release/kci replay /path/to/events.mainnet.sqlite

# 以 JS 固定规则为参考，对各动作代表样本及单字段变异进行离线差分。
# 需要支持 node:sqlite 的 Node.js，仅此开发工具依赖 JS。
cd ..
node rust/tools/differential.mjs /path/to/events.tn10.sqlite /path/to/output-directory
```

`replay` 比较历史存储快照和重算结果，包括 JSON 字节序；这是离线规则重算，不会重新证明历史链上的 acceptance。`check-cases FILE` 是差分工具使用的纯离线原生规则入口。故障单测使用本机模拟 wRPC，覆盖错误网络、同后端排除、断线切换、取消清理、广播方法拒绝、补全失败和缺头最终性，不能替代真实链上业务测试。

在线 HTTP 验证检查的是读取、节点连接、UTXO 对账、检查点、鉴权和切换；无钱包、签名或交易广播。完整自然重组、新轮次六种动作的实时闭环、高负载长期内存与跨日稳定性仍需独立观察。原始本机执行输出位于 `/root/kci-rs-lab/`，没有向只读历史存档库写入记录。
