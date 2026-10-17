// Embedded single-file Web Dashboard for Kaspa Covenant Indexer (KCI)
// Fully responsive across Mobile & Desktop. Zero viewport spillover.
// Clear separation of concerns:
// - Compiler Tab (Studio): Compiles .sil to bytecode, template hash, and generates .mjs plugin code.
// - Plugins Tab (Management): Configures contract operational rules (Registry addresses, Deposit, State Invariants, Active/Retired).

export const ADMIN_HTML = `<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
<title>Kaspa Covenant Indexer · 控制台</title>
<style>
:root {
  --bg: #060a12;
  --bg-panel: #0d1524;
  --bg-panel-elevated: #121d30;
  --bg-panel-hover: #17243c;
  --border: #1a273e;
  --border-light: #253757;
  --border-glow: rgba(112, 199, 186, 0.25);
  --kaspa: #70c7ba;
  --kaspa-dim: rgba(112, 199, 186, 0.12);
  --text: #f1f5f9;
  --muted: #94a3b8;
  --dim: #64748b;
  --emerald: #10b981;
  --amber: #f59e0b;
  --rose: #ef4444;
  --indigo: #6366f1;
  --font-mono: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace;
  --radius-sm: 6px;
  --radius-md: 10px;
  --radius-lg: 14px;
}
* { box-sizing: border-box; margin: 0; padding: 0; }
html, body {
  width: 100%;
  max-width: 100vw;
  overflow-x: hidden;
  background: var(--bg);
  background-image: 
    radial-gradient(ellipse 60% 40% at 50% -10%, rgba(112, 199, 186, 0.08) 0%, transparent 70%),
    linear-gradient(180deg, transparent 0%, var(--bg) 600px);
  color: var(--text);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "PingFang SC", "Microsoft YaHei", sans-serif;
  line-height: 1.5;
  font-size: 13px;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}
.mono { font-family: var(--font-mono); }

/* Top Navigation Bar: Sticky Header + Tabs */
.top-nav-bar {
  position: sticky;
  top: 0;
  z-index: 50;
  width: 100%;
  max-width: 100vw;
  background: rgba(11, 18, 30, 0.96);
  backdrop-filter: blur(12px);
  border-bottom: 1px solid var(--border);
  box-sizing: border-box;
}

header {
  padding: 8px 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: 100%;
  box-sizing: border-box;
}
.header-brand-line {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  gap: 8px;
  min-width: 0;
}
.header-status-line {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  gap: 10px;
}
.brand {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  flex: 1;
  overflow: hidden;
}
.logo-badge {
  background: var(--kaspa-dim);
  border: 1px solid var(--kaspa);
  color: var(--kaspa);
  font-size: 11px;
  font-weight: 800;
  padding: 2px 7px;
  border-radius: var(--radius-sm);
  letter-spacing: 0.6px;
  flex-shrink: 0;
}
.version-badge {
  background: rgba(112, 199, 186, 0.12);
  border: 1px solid rgba(112, 199, 186, 0.35);
  color: var(--kaspa);
  font-size: 11px;
  font-weight: 700;
  font-family: var(--font-mono);
  padding: 1px 6px;
  border-radius: var(--radius-sm);
  letter-spacing: 0.3px;
  flex-shrink: 0;
}
.brand-title {
  font-size: 13.5px;
  font-weight: 700;
  color: var(--text);
  white-space: nowrap;
  min-width: 0;
}
@media (max-width: 480px) {
  .brand { gap: 5px; }
  .brand-title {
    font-size: 12px;
    letter-spacing: -0.2px;
  }
  .logo-badge { padding: 1px 5px; font-size: 10px; }
  .version-badge { padding: 1px 5px; font-size: 10px; }
}
@media (max-width: 380px) {
  .brand { gap: 4px; }
  .brand-title {
    font-size: 11px;
    letter-spacing: -0.4px;
  }
}
@media (max-width: 340px) {
  .logo-badge { display: none; }
  .brand-title {
    font-size: 10.5px;
    letter-spacing: -0.4px;
  }
}
.header-btns {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.header-daa-text {
  font-size: 11.5px;
  color: var(--muted);
}
.status-pill {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  background: rgba(16, 185, 129, 0.12);
  color: var(--emerald);
  border: 1px solid rgba(16, 185, 129, 0.3);
  padding: 3px 9px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
}
.status-pill.degraded {
  background: rgba(245, 158, 11, 0.12);
  color: var(--amber);
  border-color: rgba(245, 158, 11, 0.3);
}
.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 6px currentColor;
}
.btn-header {
  padding: 4px 8px;
  font-size: 12px;
}

@media (min-width: 769px) {
  header {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    padding: 10px 24px;
  }
  .header-brand-line {
    width: auto;
    justify-content: flex-start;
  }
  .header-status-line {
    width: auto;
    justify-content: flex-end;
    gap: 12px;
  }
}

@media (max-width: 480px) {
  .btn-text { display: none; }
  .btn-header { padding: 4px 7px; font-size: 12px; }
}

/* Tabs: Smooth horizontal swipe on mobile */
nav.tabs {
  border-top: 1px solid rgba(255, 255, 255, 0.04);
  padding: 0 12px;
  display: flex;
  gap: 2px;
  overflow-x: auto;
  white-space: nowrap;
  -webkit-overflow-scrolling: touch;
  scrollbar-width: none;
  width: 100%;
  box-sizing: border-box;
}
nav.tabs::-webkit-scrollbar { display: none; }
.tab-btn {
  background: transparent;
  border: none;
  border-bottom: 2px solid transparent;
  color: var(--muted);
  padding: 9px 12px;
  font-size: 12.5px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.15s;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  white-space: nowrap;
  flex-shrink: 0;
}
.tab-btn:hover { color: var(--text); }
.tab-btn.active {
  color: var(--kaspa);
  border-bottom-color: var(--kaspa);
}

/* Layout */
main {
  flex: 1;
  padding: 16px;
  max-width: 1320px;
  width: 100%;
  margin: 0 auto;
  box-sizing: border-box;
}
@media (max-width: 768px) {
  main { padding: 12px; }
}
.view-panel { display: none; }
.view-panel.active { display: block; }

/* Grids */
.grid-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(230px, 1fr));
  gap: 12px;
  margin-bottom: 16px;
}
.grid-2col {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
  margin-bottom: 16px;
}
@media (max-width: 768px) {
  .grid-cards { grid-template-columns: 1fr; gap: 10px; margin-bottom: 12px; }
  .grid-2col { grid-template-columns: 1fr; gap: 12px; margin-bottom: 12px; }
}

.card {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  padding: 14px 16px;
  transition: border-color 0.15s;
}
.card:hover { border-color: var(--border-light); }
.card-label {
  font-size: 11px;
  color: var(--muted);
  text-transform: uppercase;
  letter-spacing: 0.6px;
  font-weight: 600;
  margin-bottom: 6px;
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.card-val {
  font-size: 19px;
  font-weight: 700;
  color: var(--text);
  line-height: 1.2;
}
.card-sub {
  font-size: 11.5px;
  color: var(--muted);
  margin-top: 5px;
}

/* Section Header */
.section-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin: 14px 0 10px;
  flex-wrap: wrap;
  gap: 8px;
}
.section-title {
  font-size: 14.5px;
  font-weight: 700;
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text);
}
.section-desc {
  font-size: 12px;
  color: var(--muted);
  margin-top: 2px;
}
@media (max-width: 768px) {
  .section-head {
    flex-direction: column;
    align-items: stretch;
  }
  .section-head > button {
    width: 100%;
    justify-content: center;
  }
}

/* Tables & Wrappers */
.table-wrap {
  width: 100%;
  max-width: 100%;
  overflow-x: auto;
  -webkit-overflow-scrolling: touch;
}
.table-wrap table {
  min-width: 520px;
}
.table-wrap-compact table {
  min-width: 100%;
}
table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
th {
  background: rgba(18, 29, 48, 0.7);
  color: var(--muted);
  font-weight: 600;
  text-align: left;
  padding: 9px 12px;
  border-bottom: 1px solid var(--border);
  white-space: nowrap;
}
td {
  padding: 9px 12px;
  border-bottom: 1px solid var(--border);
  white-space: nowrap;
}
td.wrap-cell {
  white-space: normal;
  word-break: break-all;
}
tr:hover td { background: rgba(23, 36, 60, 0.5); }

/* Badges & Tags */
.tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 7px;
  border-radius: 4px;
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
}
.tag-active { background: rgba(16, 185, 129, 0.15); color: var(--emerald); border: 1px solid rgba(16, 185, 129, 0.3); }
.tag-standby { background: rgba(99, 102, 241, 0.15); color: var(--indigo); border: 1px solid rgba(99, 102, 241, 0.3); }
.tag-retired { background: rgba(148, 163, 184, 0.15); color: var(--muted); border: 1px solid rgba(148, 163, 184, 0.3); }
.tag-warn { background: rgba(245, 158, 11, 0.15); color: var(--amber); border: 1px solid rgba(245, 158, 11, 0.3); }
.tag-rose { background: rgba(239, 68, 68, 0.15); color: var(--rose); border: 1px solid rgba(239, 68, 68, 0.3); }

/* Buttons */
.btn {
  background: var(--bg-panel-elevated);
  color: var(--text);
  border: 1px solid var(--border);
  padding: 6px 12px;
  border-radius: var(--radius-sm);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.15s;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  white-space: nowrap;
}
.btn:hover { border-color: var(--kaspa); color: var(--kaspa); background: var(--bg-panel-hover); }
.btn-pri {
  background: var(--kaspa);
  color: #04100c;
  border-color: var(--kaspa);
}
.btn-pri:hover { background: #5ebaa9; color: #04100c; }
.btn-sm { padding: 4px 9px; font-size: 11.5px; }
/* Action buttons inside tables must never be squeezed into a vertical stack. */
table td .btn, table td button {
  white-space: nowrap;
  flex: 0 0 auto;
  line-height: 1.35;
}

/* Inputs */
input, select, textarea {
  background: #09101b;
  border: 1px solid var(--border);
  color: var(--text);
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  font-size: 12.5px;
  width: 100%;
}
input:focus, select:focus, textarea:focus {
  outline: none;
  border-color: var(--kaspa);
  box-shadow: 0 0 10px var(--border-glow);
}

/* Modals & Toast */
.modal-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(3, 7, 13, 0.85);
  backdrop-filter: blur(6px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
  padding: 16px;
}
.modal-box {
  background: var(--bg-panel);
  border: 1px solid var(--border-light);
  border-radius: var(--radius-lg);
  max-width: 440px;
  width: 100%;
  padding: 20px;
  box-shadow: 0 20px 45px rgba(0, 0, 0, 0.6);
}
.toast {
  position: fixed;
  bottom: 20px;
  right: 20px;
  background: var(--bg-panel-elevated);
  border: 1px solid var(--kaspa);
  color: var(--text);
  padding: 10px 18px;
  border-radius: var(--radius-md);
  box-shadow: 0 10px 25px rgba(0,0,0,0.5);
  font-size: 12px;
  font-weight: 600;
  z-index: 200;
  display: none;
}

/* Compiler Workbench Toolbar & Controls */
.compiler-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 12px;
  flex-wrap: wrap;
  gap: 10px;
}
.compiler-toolbar-left {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
}
.compiler-toolbar-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.compiler-controls {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.compiler-btn-group {
  display: flex;
  gap: 10px;
}
.compiler-btn-group .btn {
  flex: 1;
  justify-content: center;
  height: 38px;
}

@media (min-width: 769px) {
  .compiler-controls {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: flex-end;
    gap: 16px;
  }
  .compiler-btn-group .btn {
    flex: initial;
    height: auto;
  }
}

@media (max-width: 640px) {
  .compiler-toolbar {
    flex-direction: column;
    align-items: stretch;
  }
  .compiler-toolbar-left {
    width: 100%;
  }
  .compiler-toolbar-left select {
    width: 100% !important;
  }
  .compiler-toolbar-right {
    width: 100%;
    justify-content: space-between;
  }
}

/* Journal Activity Feed Styles */
.journal-feed {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
  max-width: 100%;
  box-sizing: border-box;
}
.journal-card {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 12px 14px;
  transition: border-color 0.15s;
  box-sizing: border-box;
  max-width: 100%;
  overflow: hidden;
  word-break: break-all;
  overflow-wrap: anywhere;
}
.journal-card:hover {
  border-color: var(--border-light);
}
.journal-card-top {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
  flex-wrap: wrap;
  gap: 6px;
}
.journal-time {
  font-size: 11px;
  color: var(--muted);
  flex-shrink: 0;
}
.journal-body {
  font-size: 12px;
  line-height: 1.6;
  max-width: 100%;
  overflow: hidden;
  word-break: break-all;
  overflow-wrap: anywhere;
}
.journal-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 4px 10px;
  max-width: 100%;
  overflow: hidden;
}
.journal-field {
  display: flex;
  gap: 6px;
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
}
.journal-field-label {
  color: var(--muted);
  flex-shrink: 0;
}
.journal-field-val {
  color: var(--text);
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  word-break: break-all;
  overflow-wrap: anywhere;
}
</style>
</head>
<body>

<div class="top-nav-bar">
  <header>
    <div class="header-brand-line">
      <div class="brand">
        <span class="logo-badge">KCI</span>
        <span class="version-badge" id="appVersionBadge">v0.1.2</span>
        <span class="brand-title">Kaspa Covenant Indexer</span>
      </div>
      <div class="header-btns">
        <button class="btn btn-header" id="btnRefresh" title="刷新状态">🔄<span class="btn-text"> 刷新</span></button>
        <button class="btn btn-header" id="btnLogout" title="设置/更换管理员令牌">🔐<span class="btn-text"> 登录管理</span></button>
      </div>
    </div>
    <div class="header-status-line">
      <div id="headerStatusPill" class="status-pill"><span class="dot"></span><span id="pillText">连接中...</span></div>
      <div id="networkSwitcher" style="display:none;"></div>
      <div id="headerDaaText" class="mono header-daa-text">选定链 DAA: --</div>
    </div>
  </header>

  <nav class="tabs">
    <button class="tab-btn active" data-tab="tab-overview">📊 运行概览</button>
    <button class="tab-btn" data-tab="tab-nodes">🛰️ 节点热备池</button>
    <button class="tab-btn" data-tab="tab-plugins">🧩 契约插件管理</button>
    <button class="tab-btn" data-tab="tab-compiler">🛠️ SilverScript 编译工坊</button>
    <button class="tab-btn" data-tab="tab-journal">📜 审计流水</button>
  </nav>
</div>

<main>
  <!-- TAB 1: 运行概览 -->
  <section id="tab-overview" class="view-panel active">
    <div class="grid-cards">
      <div class="card">
        <div class="card-label"><span>活跃共识节点</span><span>P2P ID</span></div>
        <div class="card-val mono" id="statNodeUrl" style="font-size:13.5px;word-break:break-all;">-</div>
        <div class="card-sub" id="statNodeDetails">延迟: - | 状态: -</div>
      </div>
      <div class="card">
        <div class="card-label"><span>已收录轮次统计</span><span>ROUNDS</span></div>
        <div class="card-val" id="statRoundsTotal">-</div>
        <div class="card-sub" id="statRoundsBreakdown">活跃进行中: - · 终局完成: -</div>
      </div>
      <div class="card">
        <div class="card-label"><span>选定链 DAA & 检查点</span><span>CONSENSUS</span></div>
        <div class="card-val mono" id="statVirtualDaa" style="font-size:18px;">-</div>
        <div class="card-sub" id="statCheckpointTime">最近检查点: -</div>
      </div>
      <div class="card">
        <div class="card-label"><span>系统内存与负载</span><span>PROCESS</span></div>
        <div class="card-val mono" id="statMemRss" style="font-size:18px;">-</div>
        <div class="card-sub" id="statUptime">已运行: -</div>
      </div>
    </div>

    <!-- 🎯 任意阶段交易全链路追溯与补录卡片 -->
    <div class="card" style="border-left:4px solid var(--kaspa);margin-bottom:16px;">
      <div class="card-label">
        <span style="color:var(--kaspa);">🎯 任意阶段交易全链路追溯与补录 (Universal TXID Tracer)</span>
        <span class="tag tag-active">ANY STAGE & DEDUP</span>
      </div>
      <div class="section-desc" style="margin-bottom:12px;font-size:12px;">
        输入合约生命周期中<strong>任意阶段的交易哈希 (TXID)</strong>（无论是创世 GENESIS、购买 BUY、封盘 CLOSE、开奖 DRAW 还是退款 REFUND）。系统将智能沿链回溯至创世、核验当前状态、与已有 Covenant ID (CID) 比对去重，并由索引器全自动追溯补齐。
      </div>
      <div style="display:flex;gap:8px;flex-wrap:wrap;">
        <input type="text" id="inputTraceTxid" class="mono" style="flex:1;min-width:240px;height:36px;" placeholder="输入任意阶段 64 位十六进制交易哈希 (TXID)...">
        <button class="btn btn-pri" id="btnTraceTxid" style="white-space:nowrap;height:36px;">🔍 智能追溯与去重核验</button>
      </div>
      <div id="traceResultBox" style="display:none;margin-top:12px;"></div>
    </div>

    <div class="grid-2col">
      <!-- 双节点状态速览 -->
      <div class="card" style="padding:0;overflow:hidden;">
        <div style="padding:12px 14px;border-bottom:1px solid var(--border);font-weight:700;display:flex;justify-content:space-between;align-items:center;">
          <span>🛰️ 双节点热备当前拓扑</span>
          <button class="btn btn-sm" onclick="document.querySelector('[data-tab=\\'tab-nodes\\']').click()">完整节点池 →</button>
        </div>
        <div class="table-wrap table-wrap-compact">
          <table>
            <thead>
              <tr>
                <th>节点角色</th>
                <th>地址 (wRPC)</th>
                <th>运行状态</th>
              </tr>
            </thead>
            <tbody id="tblOverviewDual">
              <tr><td colspan="3" style="text-align:center;color:var(--muted);">加载中...</td></tr>
            </tbody>
          </table>
        </div>
      </div>

      <!-- 契约版本分布速览 -->
      <div class="card" style="padding:0;overflow:hidden;">
        <div style="padding:12px 14px;border-bottom:1px solid var(--border);font-weight:700;display:flex;justify-content:space-between;align-items:center;">
          <span>🧩 Covenant 契约版本分布</span>
          <button class="btn btn-sm" onclick="document.querySelector('[data-tab=\\'tab-plugins\\']').click()">插件管理 →</button>
        </div>
        <div class="table-wrap table-wrap-compact">
          <table>
            <thead>
              <tr>
                <th>插件标识</th>
                <th>模式</th>
                <th>已索引轮次</th>
              </tr>
            </thead>
            <tbody id="tblContractSummary">
              <tr><td colspan="3" style="text-align:center;color:var(--muted);">加载中...</td></tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  </section>

  <!-- TAB 2: 节点热备池 (Kaswin 节点池模式) -->
  <section id="tab-nodes" class="view-panel">
    <div class="section-head">
      <div>
        <div class="section-title">🛰️ Kaspa TN10 节点池矩阵与双节点热备</div>
        <div class="section-desc">
          基于双节点热备模型（Primary + Standby，强制互斥 P2P 后端），实现零秒容灾转移与实时选定链重组追踪。
        </div>
      </div>
      <div style="display:flex;gap:8px;flex-wrap:wrap;">
        <button class="btn btn-sm" id="btnElectPool" style="white-space:nowrap;" title="并行测速候选池，自动切换到延迟最低且不同后端的两个健康节点">🔄 重新选举最优主备</button>
        <button class="btn btn-pri btn-sm" id="btnBenchmarkPool">⚡ 一键测速候选节点池 (Benchmark Pool)</button>
      </div>
    </div>

    <!-- 活跃双节点矩阵卡片 -->
    <div class="grid-2col">
      <div class="card" style="border-left:4px solid var(--kaspa);">
        <div class="card-label">
          <span style="color:var(--kaspa);">主连接节点 (Primary · Active)</span>
          <span class="tag tag-active">QUERY & EVENTS</span>
        </div>
        <div class="mono" id="dualPrimaryUrl" style="font-size:12.5px;font-weight:700;word-break:break-all;">-</div>
        <div class="card-sub" id="dualPrimarySub">状态: -</div>
      </div>
      <div class="card" style="border-left:4px solid var(--indigo);">
        <div class="card-label">
          <span style="color:var(--indigo);">热备节点 (Standby · Hot Backup)</span>
          <span class="tag tag-standby">SUBSCRIBED</span>
        </div>
        <div class="mono" id="dualStandbyUrl" style="font-size:12.5px;font-weight:700;word-break:break-all;">-</div>
        <div class="card-sub" id="dualStandbySub">状态: -</div>
      </div>
    </div>

    <!-- REST 辅助加速探针卡片 -->
    <div class="card" style="border-left:4px solid var(--amber);margin-bottom:16px;">
      <div style="display:flex;justify-content:space-between;align-items:flex-start;flex-wrap:wrap;gap:8px;">
        <div>
          <div class="card-label">
            <span style="color:var(--amber);">⚡ 官方 REST 定位加速探针 (Spender Hint Oracle)</span>
            <span id="restStatusTag" class="tag tag-active" style="background:rgba(245,158,11,0.15);color:var(--amber);">ENABLED</span>
          </div>
          <div class="mono" id="restEndpointUrl" style="font-size:12.5px;font-weight:700;word-break:break-all;">-</div>
          <div class="card-sub" id="restEndpointSub">角色: 三级极速线索定位探针 · 免深度块回溯 · 100% 由主节点共识复验</div>
        </div>
        <button class="btn btn-sm" id="btnProbeRest" style="border:1px solid var(--border);">⚡ 探测 REST 探针状态</button>
      </div>
    </div>

    <!-- 候选节点池列表 -->
    <div class="card" style="padding:0;overflow:hidden;margin-bottom:16px;">
      <div style="padding:12px 14px;border-bottom:1px solid var(--border);display:flex;justify-content:space-between;align-items:center;flex-wrap:wrap;gap:6px;">
        <span style="font-weight:700;">📌 已入池的自定义节点 (Managed Pool)</span>
        <span style="color:var(--muted);font-size:11px;">这些地址已持久化；可点击「设为主连接」立即接管，或由系统故障换血时选举</span>
      </div>
      <div class="table-wrap" style="border-bottom:1px solid var(--border);">
        <table>
          <thead>
            <tr><th style="min-width:180px;">节点地址</th><th style="white-space:nowrap;">加入时间</th><th style="white-space:nowrap;">当前长连接角色</th><th style="white-space:nowrap;min-width:196px;">操作</th></tr>
          </thead>
          <tbody id="tblManagedPool">
            <tr><td colspan="4" style="text-align:center;color:var(--muted);">暂无自定义池节点（可在下方测速后点「+ 加入池」，或在上方自定义探测里加入）</td></tr>
          </tbody>
        </table>
      </div>
      <div style="padding:12px 14px;border-bottom:1px solid var(--border);display:flex;justify-content:space-between;align-items:center;flex-wrap:wrap;gap:6px;">
        <span style="font-weight:700;">🌐 候选节点池健康矩阵 (TN10 Seeds & Pool)</span>
        <span style="color:var(--muted);font-size:11px;">注：标记【共享后端】的节点属于同一物理实例，不可互为双备</span>
      </div>
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>候选节点地址</th>
              <th style="white-space:nowrap;">延迟 RTT</th>
              <th style="white-space:nowrap;">落后 Lag</th>
              <th style="white-space:nowrap;">同步 / UTXO</th>
              <th style="white-space:nowrap;">P2P 后端</th>
              <th style="white-space:nowrap;min-width:168px;">操作</th>
            </tr>
          </thead>
          <tbody id="tblCandidatePool">
            <tr><td colspan="6" style="text-align:center;color:var(--muted);">点击上方【一键测速候选节点池】进行网络体检</td></tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- 自定义节点体检与测试 -->
    <div class="card">
      <div class="section-title" style="margin-bottom:6px;">🔍 自定义节点连通性探测 (Endpoint Probe)</div>
      <div class="section-desc" style="margin-bottom:12px;">
        输入任意自定义 Kaspa wRPC 节点（支持 ws:// 或 wss://，需开放 JSON 或 Borsh 端口），即时探测网络、延迟与 DAG Sink 状态：
      </div>
      <div style="display:flex;gap:8px;margin-bottom:10px;flex-wrap:wrap;">
        <input type="text" id="inputProbeUrl" class="mono" placeholder="ws://127.0.0.1:17210 或 wss://..." value="wss://quark-10.kaspa.red/kaspa/testnet-10/wrpc/json" style="flex:1;min-width:200px;">
        <button class="btn btn-sm" id="btnProbeNode" style="white-space:nowrap;">🚀 测速与体检</button>
        <button class="btn btn-sm btn-pri" id="btnProbeAddPool" style="white-space:nowrap;">＋ 加入候选池</button>
      </div>
      <div style="display:flex;gap:6px;align-items:center;flex-wrap:wrap;font-size:11px;color:var(--muted);">
        <span>快捷测试:</span>
        <button class="btn btn-sm" type="button" onclick="probeSingle('ws://127.0.0.1:17210')">本地 .201 (17210)</button>
        <button class="btn btn-sm" type="button" onclick="probeSingle('wss://vector-10.kaspa.green/kaspa/testnet-10/wrpc/json')">vector-10</button>
        <button class="btn btn-sm" type="button" onclick="probeSingle('wss://quark-10.kaspa.red/kaspa/testnet-10/wrpc/json')">quark-10</button>
        <button class="btn btn-sm" type="button" onclick="probeSingle('wss://muon-10.kaspa.blue/kaspa/testnet-10/wrpc/json')">muon-10</button>
      </div>
      <div id="probeResultBox" style="display:none;margin-top:12px;"></div>
    </div>
  </section>

  <!-- TAB 3: 契约插件管理 (配置与运维中心) -->
  <section id="tab-plugins" class="view-panel">
    <div class="section-head">
      <div>
        <div class="section-title">
          🧩 Covenant 状态机插件管理 (Contract Plugins)
          <span id="pluginsNetBadge" class="tag tag-active mono" style="font-size:11px;margin-left:6px;display:none;"></span>
        </div>
        <div class="section-desc">已装载的契约生命周期规则插件。支持配置创世守门断言、指定监听注册表地址，以及热切换主导/退役状态。</div>
      </div>
      <button class="btn btn-sm" id="btnReloadPlugins">🔄 刷新列表</button>
    </div>

    <div class="card" style="padding:0;overflow:hidden;margin-bottom:16px;">
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>契约标识</th>
              <th>模块文件</th>
              <th>Profile 指纹 (SHA256)</th>
              <th>监听地址</th>
              <th>运行角色</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody id="tblPluginList">
            <tr><td colspan="6" style="text-align:center;color:var(--muted);">加载中...</td></tr>
          </tbody>
        </table>
      </div>
    </div>
  </section>

  <!-- TAB 4: SilverScript 编译工坊 (纯编译与产物生成) -->
  <section id="tab-compiler" class="view-panel">
    <div class="section-head">
      <div>
        <div class="section-title">🛠️ SilverScript 在线编译器 (SilverScript Studio)</div>
        <div class="section-desc">
          在线编写、语法分析并编译 SilverScript 契约，由 <code>silverc</code> 生成字节码、模版哈希与标准插件代码。
        </div>
      </div>
    </div>

    <div class="card" style="margin-bottom:16px;">
      <div class="compiler-toolbar">
        <div class="compiler-toolbar-left">
          <label style="color:var(--muted);font-size:12px;font-weight:600;white-space:nowrap;">模版:</label>
          <select id="selectContractPreset" style="padding:5px 8px;font-size:12px;">
            <option value="counter">计数器合约 (Counter.sil · 单例状态机)</option>
            <option value="v201-open">Kaswin V2.0.1 · Open 开放售票帧 (KaswinOpen.sil)</option>
            <option value="v201-sealed">Kaswin V2.0.1 · Sealed 封存待抽帧 (KaswinSealed.sil)</option>
            <option value="v201-refunding">Kaswin V2.0.1 · Refunding 批量退款帧 (KaswinRefunding.sil)</option>
            <option value="empty">空白契约模版 (Custom.sil)</option>
          </select>
        </div>
        <div class="compiler-toolbar-right">
          <input type="file" id="fileUploadSil" accept=".sil" style="display:none;">
          <button class="btn btn-sm" id="btnUploadSil">📂 载入 .sil</button>
          <span class="tag tag-active" style="font-size:11px;">silverc v0.1.0</span>
        </div>
      </div>

      <div style="margin-bottom:12px;">
        <textarea id="silEditor" class="mono" style="height:210px;font-size:12px;line-height:1.6;" spellcheck="false"></textarea>
      </div>

      <div class="compiler-controls">
        <div style="width:100%;">
          <label style="display:block;font-size:11.5px;color:var(--muted);margin-bottom:4px;">
            构造函数参数 (JSON 数组，如 <code>[0]</code> 或 <code>[]</code>):
          </label>
          <input type="text" id="inputCtorArgs" class="mono" value="[0]">
        </div>
        <div class="compiler-btn-group">
          <button class="btn btn-sm" id="btnAstOnly">🔍 语法解析 (AST)</button>
          <button class="btn btn-pri btn-sm" id="btnCompileSil">⚡ 执行编译 (Compile)</button>
        </div>
      </div>
    </div>

    <div id="compilerResultBox" style="display:none;margin-top:16px;"></div>
  </section>

  <!-- TAB 5: 审计流水 (事件流 Feed 卡片) -->
  <section id="tab-journal" class="view-panel">
    <div class="section-head">
      <div>
        <div class="section-title">📜 实时系统事件与共识审计流水 (Consensus Journal)</div>
        <div class="section-desc">
          由 SQLite 原子记录的 Kaspa 链上 Covenant 状态机演进日志：包含交易接受（APPLIED）、UTXO 花费（SPENT_EVENT）、链重组回退（ROLLBACK）与节点异常（ERROR）。
        </div>
      </div>
      <div style="display:flex;gap:8px;flex-wrap:wrap;">
        <button class="btn btn-pri btn-sm" id="btnRecheckDiscoveries" title="重新复审所有未匹配的历史创世任务">⚡ 重新复审历史创世并自动追块</button>
        <button class="btn btn-sm" id="btnRefreshJournal">🔄 刷新流水</button>
      </div>
    </div>

    <div id="journalFeed" class="journal-feed">
      <div class="card" style="text-align:center;color:var(--muted);">加载中...</div>
    </div>
  </section>
</main>

<!-- 身份鉴权弹窗 -->
<div class="modal-backdrop" id="authModal" style="display:none;">
  <div class="modal-box">
    <div class="section-title" style="margin-bottom:8px;">🔐 管理员身份验证</div>
    <div class="section-desc" style="margin-bottom:16px;">
      当前控制台接口受管理员令牌保护。请输入服务配置的 <code>ADMIN_TOKEN</code>：
    </div>
    <form id="authForm">
      <div style="margin-bottom:14px;">
        <input type="password" id="inputAdminToken" placeholder="输入 32 位 Token..." required autofocus>
      </div>
      <div style="display:flex;gap:10px;">
        <button type="submit" class="btn btn-pri" style="flex:1;justify-content:center;">确认登录</button>
        <button type="button" class="btn" id="btnCancelAuth" style="justify-content:center;">暂不登录</button>
      </div>
    </form>
  </div>
</div>

<!-- 契约守门规则与监听地址配置弹窗 (在插件管理页面打开) -->
<div class="modal-backdrop" id="pluginConfigModal" style="display:none;">
  <div class="modal-box" style="max-width:680px;max-height:92vh;overflow-y:auto;">
    <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:12px;">
      <div class="section-title">⚙️ 配置契约守门规则与监听地址</div>
      <button class="btn btn-sm" onclick="document.getElementById('pluginConfigModal').style.display='none'">✕</button>
    </div>

    <!-- 插件基本信息展示 -->
    <div class="card" style="padding:10px 14px;background:var(--bg-panel-elevated);margin-bottom:12px;">
      <div style="display:flex;justify-content:space-between;align-items:center;flex-wrap:wrap;gap:6px;">
        <div>
          <span style="color:var(--muted);font-size:11px;">契约标识:</span>
          <strong class="mono" id="cfgModalPluginKey" style="color:var(--kaspa);font-size:13px;">-</strong>
        </div>
        <label style="display:flex;align-items:center;gap:6px;font-size:12px;cursor:pointer;">
          <input type="checkbox" id="cfgActiveToggle">
          <span>设为主导发现新轮次 (Active)</span>
        </label>
      </div>
    </div>

    <!-- 1. 创世监听与注册表地址配置 -->
    <div style="background:var(--bg-panel-elevated);border:1px solid var(--border);border-radius:var(--radius-sm);padding:10px 12px;margin-bottom:12px;">
      <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:6px;flex-wrap:wrap;gap:4px;">
        <span style="font-weight:700;font-size:12px;color:var(--kaspa);">📡 创世监听与注册表配置 (Genesis Discovery)</span>
        <button class="btn btn-sm" type="button" id="btnFillRegistry">填入当前网络官方 Registry</button>
      </div>
      <div class="grid-2col" style="margin-bottom:0;">
        <div>
          <label style="font-size:11px;color:var(--muted);display:block;margin-bottom:2px;">监听的注册表地址 (逗号分隔):</label>
          <input type="text" id="cfgRegistryAddr" class="mono" style="font-size:11.5px;" placeholder="kaspatest:qz... (留空需手动 track 创世)">
        </div>
        <div>
          <label style="font-size:11px;color:var(--muted);display:block;margin-bottom:2px;">创世登记费 (sompi, 0为不限):</label>
          <input type="text" id="cfgRegSompi" class="mono" style="font-size:11.5px;" value="5000000" placeholder="5000000 (0.05 TKAS)">
        </div>
      </div>
    </div>

    <!-- 2. 合法创世状态与参数守门断言 -->
    <div style="background:var(--bg-panel-elevated);border:1px solid var(--border);border-radius:var(--radius-sm);padding:10px 12px;margin-bottom:14px;">
      <div style="font-weight:700;font-size:12px;color:var(--emerald);margin-bottom:6px;">
        🛡️ 合法创世守门断言 (Genesis State Invariants & Guards)
      </div>
      <div class="grid-2col" style="margin-bottom:8px;">
        <div>
          <label style="font-size:11px;color:var(--muted);display:block;margin-bottom:2px;">创世质押金额 (sompi, 留空不限):</label>
          <input type="text" id="cfgReqDeposit" class="mono" style="font-size:11.5px;" placeholder="如 400000000 (4 TKAS) 或留空">
        </div>
        <div>
          <label style="font-size:11px;color:var(--muted);display:block;margin-bottom:2px;">允许的创建者地址 (白名单, 选填):</label>
          <input type="text" id="cfgAllowedCreators" class="mono" style="font-size:11.5px;" placeholder="留空允许任何人创世">
        </div>
      </div>
      <div>
        <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:6px;flex-wrap:wrap;gap:4px;">
          <label style="font-size:11px;color:var(--text);font-weight:700;">
            状态参数严格约束列表 (勾选并指定创世必须满足的值):
          </label>
          <button class="btn btn-sm" type="button" onclick="addCustomStateGuardRow()">+ 添加字段</button>
        </div>
        <div id="cfgDynamicFieldsList" style="display:flex;flex-direction:column;gap:6px;">
          <!-- 动态行 -->
        </div>
      </div>
    </div>

    <!-- 底部按钮 -->
    <div style="display:flex;gap:10px;width:100%;box-sizing:border-box;">
      <button class="btn btn-pri" id="btnSavePluginConfig" style="flex:2;justify-content:center;height:40px;font-size:13px;">💾 保存配置并热生效</button>
      <button class="btn" style="flex:1;justify-content:center;height:40px;font-size:13px;" onclick="document.getElementById('pluginConfigModal').style.display='none'">取消</button>
    </div>
  </div>
</div>

<!-- 查看插件源码弹窗 -->
<div class="modal-backdrop" id="pluginCodeModal" style="display:none;">
  <div class="modal-box" style="max-width:680px;max-height:90vh;overflow-y:auto;">
    <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:12px;">
      <div class="section-title">📜 查看生成的插件源代码</div>
      <button class="btn btn-sm" onclick="document.getElementById('pluginCodeModal').style.display='none'">✕</button>
    </div>
    <div style="margin-bottom:12px;">
      <textarea id="pluginViewCodeArea" class="mono" style="height:280px;font-size:11.5px;line-height:1.5;background:#050912;" readonly></textarea>
    </div>
    <div style="display:flex;gap:10px;">
      <button class="btn btn-pri" style="flex:1;justify-content:center;height:38px;" onclick="copyPluginViewCode()">📋 复制源代码</button>
      <button class="btn" style="height:38px;" onclick="document.getElementById('pluginCodeModal').style.display='none'">关闭</button>
    </div>
  </div>
</div>

<div class="toast" id="toastMsg"></div>

<script>
let currentToken = sessionStorage.getItem('kci_admin_token') || '';
const urlToken = new URLSearchParams(window.location.search).get('token');
if (urlToken) {
  currentToken = urlToken;
  sessionStorage.setItem('kci_admin_token', urlToken);
}

function updateAuthUI() {
  const btn = document.getElementById('btnLogout');
  if (!btn) return;
  if (currentToken) {
    btn.innerHTML = '🔓<span class="btn-text"> 管理已解锁</span>';
    btn.style.color = 'var(--emerald)';
  } else {
    btn.innerHTML = '🔐<span class="btn-text"> 登录管理</span>';
    btn.style.color = 'var(--text)';
  }
}
updateAuthUI();

function showToast(text, color) {
  const el = document.getElementById('toastMsg');
  el.innerText = text;
  el.style.borderColor = color || 'var(--kaspa)';
  el.style.display = 'block';
  setTimeout(() => el.style.display = 'none', 3000);
}

const p = window.location.pathname;
const adminIdx = p.indexOf('/admin');
const apiPrefix = (adminIdx > 0 ? p.slice(0, adminIdx) : '') + '/admin/api';

// Selected network view (multi-network mode). Persisted so a reload keeps the operator's choice.
let selectedNetwork = null;
try { selectedNetwork = localStorage.getItem('kci.network') || null; } catch {}

function withNetwork(path) {
  if (!selectedNetwork) return path;
  return path + (path.includes('?') ? '&' : '?') + 'network=' + encodeURIComponent(selectedNetwork);
}

async function api(path, method, body) {
  method = method || 'GET';
  const headers = { 'Accept': 'application/json' };
  if (currentToken) headers['Authorization'] = 'Bearer ' + currentToken;
  if (body) headers['Content-Type'] = 'application/json';

  const res = await fetch(apiPrefix + withNetwork(path), {
    method,
    headers,
    body: body ? JSON.stringify(body) : null
  });

  if (res.status === 401) {
    document.getElementById('authModal').style.display = 'flex';
    throw new Error('UNAUTHORIZED');
  }
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err.detail || err.error || ('HTTP ' + res.status));
  }
  return res.json();
}

// 1. Status & Overview
async function refreshAll() {
  try {
    const data = await api('/status');
    document.getElementById('authModal').style.display = 'none';

    // Multi-network switcher: shown only when this process monitors more than one network.
    const sw = document.getElementById('networkSwitcher');
    const netKeys = data.networks || [];
    if (sw && netKeys.length > 1) {
      sw.style.display = 'inline-flex';
      sw.innerHTML = netKeys.map(k =>
        '<button class="btn btn-sm" type="button" data-net="' + k + '" style="padding:2px 8px;font-size:11px;' +
        (k === (data.networkKey || netKeys[0]) ? 'border-color:var(--kaspa);color:var(--kaspa);' : 'opacity:.55;') +
        '">' + k + '</button>').join('');
      sw.querySelectorAll('button[data-net]').forEach(b => b.onclick = async () => {
        selectedNetwork = b.dataset.net;
        try { localStorage.setItem('kci.network', selectedNetwork); } catch {}
        window._poolLoaded = false;
        await refreshAll();
        // Immediately refresh the currently active tab content so switching networks always updates the view!
        const activeTab = document.querySelector('.view-panel.active')?.id;
        if (activeTab === 'tab-plugins') {
          await loadPluginsTab();
        } else if (activeTab === 'tab-nodes') {
          await loadNodePoolBenchmark();
        }
      });
    } else if (sw) { sw.style.display = 'none'; }

    const netLabel = data.networkKey ? data.networkKey.toUpperCase() : 'TN10';

    // Header Pill
    const pill = document.getElementById('headerStatusPill');
    const pillText = document.getElementById('pillText');
    const daaText = document.getElementById('headerDaaText');
    if (data.activeNode && data.activeNode.isSynced) {
      pill.className = 'status-pill';
      if (pillText) pillText.innerText = netLabel + ' · 已同步';
    } else {
      pill.className = 'status-pill degraded';
      if (pillText) pillText.innerText = netLabel + ' · 追块/降级';
    }
    if (daaText) {
      const curDaa = data.activeNode?.virtualDaaScore || data.checkpoint?.virtualDaa;
      daaText.innerText = curDaa ? ('选定链 DAA: ' + curDaa) : '选定链就绪';
    }

    // Top KPI
    if (data.version) {
      const elV = document.getElementById('appVersionBadge');
      if (elV) elV.innerText = data.version.startsWith('v') ? data.version : ('v' + data.version);
    }
    document.getElementById('statNodeUrl').innerText = (data.activeNode && data.activeNode.url) || '无连接';
    document.getElementById('statNodeDetails').innerText = 
      '版本: ' + (data.activeNode?.serverVersion || '-') + ' | 同步: ' + (data.activeNode?.isSynced ? '是' : '否') + ' | UTXO: ' + (data.activeNode?.hasUtxoIndex ? '已开启' : '未开启');

    document.getElementById('statRoundsTotal').innerText = data.rounds?.total ?? 0;
    document.getElementById('statRoundsBreakdown').innerText = 
      '活跃进行中: ' + (data.rounds?.live ?? 0) + ' · 终局完成: ' + (data.rounds?.byStatus?.close ?? 0);

    document.getElementById('statVirtualDaa').innerText = data.activeNode?.virtualDaaScore || data.checkpoint?.virtualDaa || '-';
    document.getElementById('statCheckpointTime').innerText = 
      data.checkpoint?.at ? ('检查点: ' + new Date(data.checkpoint.at).toLocaleTimeString()) : '无近期检查点';

    document.getElementById('statMemRss').innerText = (data.memory?.rssMb || '-') + ' MB';
    const hrs = Math.floor(data.uptimeSec / 3600), mins = Math.floor((data.uptimeSec % 3600) / 60);
    document.getElementById('statUptime').innerText = '已运行: ' + hrs + '小时 ' + mins + '分钟';

    // Overview Dual-Node Table
    const dualTbody = document.getElementById('tblOverviewDual');
    const pool = data.nodePool || [];
    if (!pool.length) {
      dualTbody.innerHTML = '<tr><td colspan="3" style="text-align:center;color:var(--muted);">无活跃节点</td></tr>';
    } else {
      dualTbody.innerHTML = pool.map(m => '<tr>' +
        '<td>' + (m.active ? '<span class="tag tag-active">PRIMARY (主)</span>' : '<span class="tag tag-standby">STANDBY (备)</span>') + '</td>' +
        '<td class="mono" style="font-size:12px;">' + m.url + '</td>' +
        '<td>' + (m.up ? '<span style="color:var(--emerald);">● 在线正常</span>' : '<span style="color:var(--rose);">○ 掉线重试</span>') + '</td>' +
      '</tr>').join('');
    }

    // Update Dual Cards in Tab 2
    const prim = pool.find(m => m.active) || pool[0];
    const stby = pool.find(m => !m.active) || pool[1];
    if (prim) {
      document.getElementById('dualPrimaryUrl').innerText = prim.url;
      document.getElementById('dualPrimarySub').innerHTML = '运行状态: ' + (prim.up ? '<b style="color:var(--emerald);">● 在线同步中</b>' : '<b style="color:var(--rose);">○ 异常</b>') + ' | 重试: ' + prim.attempts + ' 次';
    }
    if (stby) {
      document.getElementById('dualStandbyUrl').innerText = stby.url;
      document.getElementById('dualStandbySub').innerHTML = '运行状态: ' + (stby.up ? '<b style="color:var(--emerald);">● 热备就绪中</b>' : '<b style="color:var(--rose);">○ 掉线重连中</b>') + ' | 重试: ' + stby.attempts + ' 次';
    }

    // Update REST Hint Card in Tab 2
    if (data.rest) {
      const elUrl = document.getElementById('restEndpointUrl');
      const elTag = document.getElementById('restStatusTag');
      const elSub = document.getElementById('restEndpointSub');
      if (elUrl) elUrl.innerText = data.rest.url || '未配置 / 已禁用';
      if (elTag) {
        elTag.innerText = data.rest.enabled ? 'ENABLED' : 'DISABLED';
        elTag.style.background = data.rest.enabled ? 'rgba(245,158,11,0.15)' : 'rgba(100,116,139,0.15)';
        elTag.style.color = data.rest.enabled ? 'var(--amber)' : 'var(--muted)';
      }
    }

    // Overview Contract Summary
    const cTbody = document.getElementById('tblContractSummary');
    const contractsList = data.contracts || [];
    if (!contractsList.length) {
      cTbody.innerHTML = '<tr><td colspan="3" style="text-align:center;color:var(--muted);">未配置契约插件</td></tr>';
    } else {
      cTbody.innerHTML = contractsList.map(c => {
        const count = data.rounds?.byContract?.[c.key] || 0;
        return '<tr>' +
          '<td class="mono"><b>' + c.key + '</b></td>' +
          '<td>' + (c.retired ? '<span class="tag tag-retired">RETIRED (兼容)</span>' : '<span class="tag tag-active">ACTIVE (主导)</span>') + '</td>' +
          '<td><b style="color:var(--kaspa);">' + count + '</b> 轮次</td>' +
        '</tr>';
      }).join('');
    }

    // Render Journal Feed
    renderJournalFeed(data.journal || []);

  } catch (err) {
    if (err.message !== 'UNAUTHORIZED') {
      showToast('拉取状态失败: ' + err.message, 'var(--rose)');
    }
  }
}

window.copyHash = function(text, label) {
  label = label || '哈希';
  if (!text) return;
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).then(function() {
      showToast('已复制完整 ' + label + ' 到剪贴板', 'var(--emerald)');
    }).catch(function() {
      prompt('请按 Ctrl+C 复制完整 ' + label + '：', text);
    });
  } else {
    prompt('请按 Ctrl+C 复制完整 ' + label + '：', text);
  }
};

function renderHash(val, label) {
  if (!val) return '-';
  label = label || '哈希';
  const clean = String(val);
  return '<span class="mono" style="word-break:break-all;user-select:all;cursor:pointer;" title="点击复制完整 ' + label + '" onclick="copyHash(' + JSON.stringify(clean) + ', ' + JSON.stringify(label) + ')">' +
    clean +
    ' <span style="font-size:11px;opacity:0.75;cursor:pointer;" title="点击复制完整内容">📋</span>' +
  '</span>';
}

// Render Journal Feed into clean human-readable cards
function renderJournalFeed(logs) {
  const container = document.getElementById('journalFeed');
  if (!container) return;
  if (!logs || !logs.length) {
    container.innerHTML = '<div class="card" style="text-align:center;color:var(--muted);">暂无审计事件日志</div>';
    return;
  }

  const seenDiscoveryTxids = new Set();
  const rendered = logs.map(l => {
    const timeStr = new Date(l.at).toLocaleTimeString();
    const d = l.data || l.detail || {};

    let tagClass = 'tag-active';
    let kindLabel = l.kind;
    let borderStyle = '';
    let bodyHtml = '';

    if (l.kind === 'APPLIED') {
      tagClass = 'tag-active';
      kindLabel = '✓ 链上转移生效 (' + (d.kind || 'ACTION') + ')';
      bodyHtml = '<div class="journal-grid">' +
        '<div class="journal-field"><span class="journal-field-label">轮次 CID:</span><span class="journal-field-val mono">' + renderHash(d.cid || '-', '轮次 CID') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">创世 TXID:</span><span class="journal-field-val mono">' + renderHash(d.round, '创世 TXID') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">序号:</span><span class="journal-field-val mono">Seq #' + (d.seq ?? '-') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">交易 TXID:</span><span class="journal-field-val mono">' + renderHash(d.txid, '交易 TXID') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">接受块:</span><span class="journal-field-val mono">' + renderHash(d.accepting, '接受块哈希') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">状态核验:</span><span class="journal-field-val mono" style="color:var(--kaspa);">' + (d.utxoCheck || 'PASS') + '</span></div>' +
      '</div>';
    } else if (l.kind === 'SPENT_EVENT') {
      tagClass = 'tag-standby';
      kindLabel = '⚡ 链尖 UTXO 花费事件';
      const op = d.outpoint ? renderHash(d.outpoint.transactionId + ':' + d.outpoint.index, 'Outpoint') : '-';
      bodyHtml = '<div class="journal-grid">' +
        '<div class="journal-field"><span class="journal-field-label">创世 TXID:</span><span class="journal-field-val mono">' + renderHash(d.round, '创世 TXID') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">轮次 CID:</span><span class="journal-field-val mono">' + renderHash(d.cid || '-', '轮次 CID') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">已花费 Tip:</span><span class="journal-field-val mono">' + op + '</span></div>' +
      '</div><div style="font-size:11px;color:var(--muted);margin-top:4px;">节点推送此 UTXO 被花费，触发后继状态机解析</div>';
    } else if (l.kind === 'ROLLBACK' || l.kind === 'ROLLBACK_REPLAY') {
      tagClass = 'tag-warn';
      borderStyle = 'border-left: 3px solid var(--amber);';
      kindLabel = l.kind === 'ROLLBACK' ? '⚠️ 选定链重组回退 (Rollback)' : '🔄 重组快速重放恢复 (Replay)';
      bodyHtml = '<div class="journal-grid">' +
        '<div class="journal-field"><span class="journal-field-label">轮次 ID:</span><span class="journal-field-val mono">' + renderHash(d.round, '轮次 ID') + '</span></div>' +
        (d.fromSeq !== undefined ? '<div class="journal-field"><span class="journal-field-label">回退起始:</span><span class="journal-field-val mono">Seq #' + d.fromSeq + '</span></div>' : '') +
        (d.txid ? '<div class="journal-field"><span class="journal-field-label">重放 TXID:</span><span class="journal-field-val mono">' + renderHash(d.txid, '重放 TXID') + '</span></div>' : '') +
      '</div>';
    } else if (l.kind === 'DISCOVERY_MARKER') {
      const discKey = (d.txid || '') + ':' + (d.contract || '');
      if (discKey && seenDiscoveryTxids.has(discKey)) return '';
      if (discKey) seenDiscoveryTxids.add(discKey);
      tagClass = 'tag-standby';
      kindLabel = '🔍 发现新契约创世 (Discovery)';
      bodyHtml = '<div class="journal-grid">' +
        '<div class="journal-field"><span class="journal-field-label">合约规则:</span><span class="journal-field-val mono">' + (d.contract || '-') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">创世 TXID:</span><span class="journal-field-val mono">' + renderHash(d.txid, '创世 TXID') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">发生 DAA:</span><span class="journal-field-val mono">' + (d.daa || '-') + '</span></div>' +
      '</div>';
    } else if (l.kind === 'GENESIS_IGNORED') {
      tagClass = 'tag-retired';
      kindLabel = '⚪ 忽略其他版本创世 (Unmatched Genesis)';
      const reasonText = d.reason === 'no pinned creation template matched' ? 'Profile 版本指纹不属于当前活动契约' : (d.reason || '模版不匹配');
      const rejText = Array.isArray(d.rejected) ? d.rejected.map(r => (r.contract ? r.contract.slice(0, 24) + '...' : '契约') + ': ' + (r.reason || '不匹配')).join('; ') : '';
      const btnTrack = d.txid ? ('<div style="margin-top:8px;"><button class="btn btn-sm" style="padding:3px 10px;font-size:11px;" onclick="trackGenesis(&quot;' + d.txid + '&quot;)">🎯 让索引器自动追此轮</button></div>') : '';
      bodyHtml = '<div class="journal-grid">' +
        '<div class="journal-field"><span class="journal-field-label">创世 TXID:</span><span class="journal-field-val mono">' + renderHash(d.txid, '创世 TXID') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">过滤说明:</span><span class="journal-field-val" style="color:var(--muted);">' + reasonText + '</span></div>' +
      '</div>' +
      (rejText ? '<div class="mono" style="font-size:11px;color:var(--dim);margin-top:4px;word-break:break-all;overflow-wrap:anywhere;">拒绝详情: ' + rejText + '</div>' : '') +
      btnTrack;
    } else if (l.kind === 'WINDOW_SCAN_FAILED') {
      tagClass = 'tag-warn';
      borderStyle = 'border-left: 3px solid var(--amber);';
      kindLabel = '⚠️ 链上滑动窗口检索异常 (Window Scan)';
      bodyHtml = '<div class="journal-grid">' +
        '<div class="journal-field"><span class="journal-field-label">起始块:</span><span class="journal-field-val mono">' + renderHash(d.start, '起始块哈希') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">异常原因:</span><span class="journal-field-val mono" style="color:var(--rose);">' + (d.message || '-') + '</span></div>' +
      '</div>';
    } else if (l.kind === 'REST_HINT_GENESIS_FOUND') {
      tagClass = 'tag-active';
      kindLabel = '⚡ REST 辅助探针命中创世 (Genesis Oracle)';
      bodyHtml = '<div class="journal-grid">' +
        '<div class="journal-field"><span class="journal-field-label">创世 TXID:</span><span class="journal-field-val mono">' + renderHash(d.txid, '创世 TXID') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">接受块:</span><span class="journal-field-val mono">' + renderHash(d.accepting, '接受块哈希') + '</span></div>' +
      '</div>';
    } else if (l.kind === 'REST_HINT_SPENDER_FOUND') {
      tagClass = 'tag-standby';
      kindLabel = '⚡ REST 辅助探针命中花费 (Spender Oracle)';
      bodyHtml = '<div class="journal-grid">' +
        '<div class="journal-field"><span class="journal-field-label">轮次 CID:</span><span class="journal-field-val mono">' + renderHash(d.cid || d.round, '轮次 CID') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">花费 TXID:</span><span class="journal-field-val mono">' + renderHash(d.txid, '花费 TXID') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">接受块:</span><span class="journal-field-val mono">' + renderHash(d.accepting, '接受块哈希') + '</span></div>' +
      '</div>';
    } else if (l.kind === 'ERROR') {
      tagClass = 'tag-rose';
      borderStyle = 'border-left: 3px solid var(--rose);';
      kindLabel = '❌ 节点通信/执行异常';
      bodyHtml = '<div style="color:var(--rose);font-size:12px;word-break:break-all;overflow-wrap:anywhere;">' +
        '组件: <b>[' + (d.key || 'system') + ']</b> · 错误: <b>' + (d.message || JSON.stringify(d)) + '</b>' +
        '<div style="color:var(--muted);font-size:11px;margin-top:2px;">(节点瞬时抖动时索引器会自动切入热备节点重试)</div>' +
      '</div>';
    } else {
      tagClass = 'tag-retired';
      kindLabel = l.kind;
      bodyHtml = '<div class="mono" style="font-size:11.5px;color:var(--muted);word-break:break-all;overflow-wrap:anywhere;white-space:pre-wrap;">' + JSON.stringify(d) + '</div>';
    }

    return '<div class="journal-card" style="' + borderStyle + '">' +
      '<div class="journal-card-top">' +
        '<span class="tag ' + tagClass + '">' + kindLabel + '</span>' +
        '<span class="journal-time mono">' + timeStr + '</span>' +
      '</div>' +
      '<div class="journal-body">' + bodyHtml + '</div>' +
    '</div>';
  }).filter(Boolean);

  container.innerHTML = rendered.length ? rendered.join('') : '<div class="card" style="text-align:center;color:var(--muted);padding:24px;">暂无审计流水记录</div>';
}

document.getElementById('btnRefreshJournal').onclick = () => {
  refreshAll();
  showToast('已拉取最新审计流水');
};

window.trackGenesis = async function(txid) {
  try {
    const res = await api('/track', 'POST', {txid});
    showToast('已提交给索引器，后台正在自动追块...', 'var(--emerald)');
    setTimeout(refreshAll, 1500);
  } catch (e) {
    showToast('追块提交失败: ' + e.message, 'var(--rose)');
  }
};

document.getElementById('btnRecheckDiscoveries').onclick = async () => {
  const btn = document.getElementById('btnRecheckDiscoveries');
  btn.disabled = true;
  btn.innerText = '正在触发索引器自动追块...';
  try {
    const res = await api('/recheck-discoveries', 'POST', {});
    showToast(res.message || '已触发索引器自动追块', 'var(--emerald)');
    setTimeout(refreshAll, 2000);
  } catch (e) {
    showToast('追块触发失败: ' + e.message, 'var(--rose)');
  } finally {
    btn.disabled = false;
    btn.innerText = '⚡ 重新复审历史创世并自动追块';
  }
};

// Universal TXID Tracer Handler
document.getElementById('btnTraceTxid').onclick = async () => {
  const input = document.getElementById('inputTraceTxid');
  const box = document.getElementById('traceResultBox');
  const btn = document.getElementById('btnTraceTxid');
  const txid = input.value.trim();
  if (!txid) {
    showToast('请输入交易哈希 TXID', 'var(--rose)');
    return;
  }
  btn.disabled = true;
  btn.innerText = '正在沿链追溯与去重核验...';
  box.style.display = 'block';
  box.innerHTML = '<div style="color:var(--muted);font-size:12px;padding:8px 0;">正在连接节点与 REST 进行跨状态回溯与 CID 去重比对...</div>';

  try {
    const res = await api('/track', 'POST', {txid});
    const isNew = res.status === 'TRACKED_NEW';
    const tagClass = isNew ? 'tag-active' : 'tag-standby';
    const tagText = isNew ? '⚡ 发现新契约并启动追块' : '✓ 已收录 (CID 比对去重)';
    const borderColor = isNew ? 'var(--kaspa)' : 'var(--indigo)';

    let fieldsHtml = '';
    if (res.cid) fieldsHtml += '<div class="journal-field"><span class="journal-field-label">Covenant ID:</span><span class="journal-field-val mono"><b>' + renderHash(res.cid, 'Covenant ID') + '</b></span></div>';
    if (res.genesisTxid) fieldsHtml += '<div class="journal-field"><span class="journal-field-label">创世 TXID:</span><span class="journal-field-val mono">' + renderHash(res.genesisTxid, '创世 TXID') + '</span></div>';
    if (res.contract) fieldsHtml += '<div class="journal-field"><span class="journal-field-label">契约版本:</span><span class="journal-field-val mono">' + res.contract + '</span></div>';
    if (res.phase !== undefined && res.phase !== null) fieldsHtml += '<div class="journal-field"><span class="journal-field-label">当前阶段:</span><span class="journal-field-val">Phase ' + res.phase + '</span></div>';
    if (res.terminal) fieldsHtml += '<div class="journal-field"><span class="journal-field-label">终局状态:</span><span class="journal-field-val" style="color:var(--kaspa);font-weight:700;">' + res.terminal + '</span></div>';
    if (res.transitionsCount !== undefined) fieldsHtml += '<div class="journal-field"><span class="journal-field-label">已记录转移:</span><span class="journal-field-val">' + res.transitionsCount + ' 笔</span></div>';

    box.innerHTML = '<div class="card" style="background:var(--bg-panel-elevated);border-color:' + borderColor + ';padding:14px;box-sizing:border-box;max-width:100%;overflow:hidden;">' +
      '<div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;flex-wrap:wrap;gap:6px;">' +
        '<span class="tag ' + tagClass + '">' + tagText + '</span>' +
        '<span class="mono" style="font-size:11.5px;color:var(--muted);">' + (res.traceHops ? ('回溯跨越 ' + res.traceHops + ' 阶状态') : '即时命中') + '</span>' +
      '</div>' +
      '<div style="font-size:12.5px;line-height:1.7;margin-bottom:8px;word-break:break-all;">' + res.message + '</div>' +
      '<div class="journal-grid" style="font-size:12px;background:var(--bg-card);padding:10px;border-radius:var(--radius-sm);">' +
        fieldsHtml +
      '</div>' +
    '</div>';

    showToast(isNew ? '已成功收录并启动追块！' : '该轮次已收录（去重完成）', 'var(--emerald)');
    setTimeout(refreshAll, 1500);
  } catch (err) {
    box.innerHTML = '<div class="card" style="border-color:var(--rose);background:rgba(239,68,68,0.1);padding:12px;">' +
      '<strong style="color:var(--rose);">❌ 追溯失败</strong>' +
      '<div style="font-size:12px;margin-top:4px;color:var(--text);word-break:break-all;">' + err.message + '</div>' +
    '</div>';
    showToast('追溯失败: ' + err.message, 'var(--rose)');
  } finally {
    btn.disabled = false;
    btn.innerText = '🔍 智能追溯与去重核验';
  }
};

// 2. Node Pool Benchmark
async function loadNodePoolBenchmark() {
  const btn = document.getElementById('btnBenchmarkPool');
  btn.disabled = true;
  btn.innerText = '⚡ 正在并发测速全部节点池...';
  const tbody = document.getElementById('tblCandidatePool');
  tbody.innerHTML = '<tr><td colspan="6" style="text-align:center;color:var(--kaspa);">正在并行握手所有候选节点，探测 RTT、Sink 及 DAA 落后...</td></tr>';

  try {
    const res = await api('/probe-pool', 'POST', {});
    const list = res.candidates || [];
    if (!list.length) {
      tbody.innerHTML = '<tr><td colspan="6" style="text-align:center;color:var(--muted);">无可用候选节点</td></tr>';
      return;
    }

    tbody.innerHTML = list.map(c => {
      let rttBadge = '<span class="tag tag-rose">超时 / 离线</span>';
      if (c.up && c.rttMs !== null) {
        if (c.rttMs < 350) rttBadge = '<span class="tag tag-active mono"><b>' + c.rttMs + ' ms</b> 极快</span>';
        else if (c.rttMs < 700) rttBadge = '<span class="tag tag-warn mono"><b>' + c.rttMs + ' ms</b> 良好</span>';
        else rttBadge = '<span class="tag tag-rose mono"><b>' + c.rttMs + ' ms</b> 较高</span>';
      }

      let lagBadge = '<span class="tag tag-retired">-</span>';
      if (c.up) {
        lagBadge = (c.lag === '0') ? '<span class="tag tag-active">已对齐 (0 块)</span>' : '<span class="tag tag-warn mono">落后 ' + c.lag + ' 块</span>';
      }

      let statusHtml = '<span style="color:var(--rose);">○ 探针超时 (' + (c.error || 'ERROR') + ')</span>';
      if (c.up) {
        statusHtml = '<span style="color:var(--emerald);">🟢 探针握手通过</span> · ' + (c.isSynced ? 'Synced' : 'Catching-up') + (c.hasUtxoIndex ? ' · UTXO✓' : '');
      }

      let backendHtml = '<span class="mono" style="font-size:11px;color:var(--muted);">' + (c.p2pId ? c.p2pId.slice(0, 8) + '...' : '-') + '</span>';
      if (c.sharedBackend) {
        backendHtml += ' <span class="tag tag-warn" title="与其他候选节点共用相同后端，不可配置为双备！">⚠️ 共享后端</span>';
      }

      return '<tr>' +
        '<td class="mono" style="word-break:break-all;overflow-wrap:anywhere;min-width:180px;"><b>' + c.url + '</b>' +
          (c.managed ? ' <span class="tag tag-active" title="已持久化到候选池，会参与主备选举">✓ 池中</span>' : '') +
        '</td>' +
        '<td style="white-space:nowrap;">' + rttBadge + '</td>' +
        '<td style="white-space:nowrap;">' + lagBadge + '</td>' +
        '<td style="white-space:nowrap;">' + statusHtml + '</td>' +
        '<td style="white-space:nowrap;">' + backendHtml + '</td>' +
        '<td style="white-space:nowrap;">' +
          '<div style="display:flex;gap:4px;flex-wrap:nowrap;align-items:center;">' +
            '<button class="btn btn-sm" style="padding:2px 7px;font-size:11px;white-space:nowrap;" title="在自定义探测中诊断该节点" onclick="probeSingle(\\'' + c.url + '\\')">诊断</button>' +
            (c.managed
              ? '<button class="btn btn-sm" style="padding:2px 7px;font-size:11px;white-space:nowrap;color:var(--rose);border-color:rgba(239,68,68,0.3);" title="从候选池移除" onclick="poolRemove(\\'' + c.url + '\\')">移出</button>'
              : '<button class="btn btn-sm btn-pri" style="padding:2px 7px;font-size:11px;white-space:nowrap;"' + (c.up ? '' : ' disabled title="节点离线，不能加入池"') + ' onclick="poolAdd(\\'' + c.url + '\\')">＋加入</button>') +
            (c.up ? '<button class="btn btn-sm" style="padding:2px 7px;font-size:11px;white-space:nowrap;" title="切换为当前主连接" onclick="poolActivate(\\'' + c.url + '\\')">⚡主连</button>' : '') +
          '</div>' +
        '</td>' + +
      '</tr>';
    }).join('');

    showToast('节点池测速完成，已按延迟排序！', 'var(--emerald)');
  } catch (err) {
    tbody.innerHTML = '<tr><td colspan="6" style="text-align:center;color:var(--rose);">测速失败: ' + err.message + '</td></tr>';
  } finally {
    btn.disabled = false;
    btn.innerText = '⚡ 一键测速候选节点池 (Benchmark Pool)';
  }
}

// Managed pool: the authoritative list of operator-added nodes (independent of a probe run).
async function loadManagedPool() {
  const tbody = document.getElementById('tblManagedPool');
  if (!tbody) return;
  try {
    const res = await api('/pool');
    const list = res.managed || [];
    if (!list.length) {
      tbody.innerHTML = '<tr><td colspan="4" style="text-align:center;color:var(--muted);">暂无自定义池节点（可在下方测速后点「+ 加入池」，或在上方自定义探测里加入）</td></tr>';
      return;
    }
    const norm = u => (u || '').replace('/json', '').replace('/borsh', '');
    const activeNorm = norm(res.activeUrl || res.active);
    const standbyNorm = norm(res.standbyUrl);
    tbody.innerHTML = list.map(n => {
      const when = n.addedAt ? new Date(n.addedAt).toLocaleString() : '-';
      const uNorm = norm(n.url);
      const isPrimary = Boolean(activeNorm && activeNorm === uNorm);
      const isStandby = Boolean(standbyNorm && standbyNorm === uNorm);
      let roleHtml = '<span class="tag tag-retired">⏸️ 候选待命 (Standby Candidate)</span>';
      if (isPrimary) {
        roleHtml = '<span class="tag tag-active" style="background:var(--emerald);color:#000;font-weight:700;">👑 主连接 (PRIMARY)</span>';
      } else if (isStandby) {
        roleHtml = '<span class="tag tag-active">🛡️ 热备连接 (STANDBY)</span>';
      }

      const actBtn = isPrimary
        ? '<span style="color:var(--emerald);font-size:11px;font-weight:700;white-space:nowrap;">✓ 正在采信</span>'
        : ('<button class="btn btn-sm btn-pri" style="padding:2px 7px;font-size:11px;white-space:nowrap;" title="切换为当前主连接" onclick="poolActivate(' + JSON.stringify(n.url) + ')">⚡ 主连</button>');

      return '<tr>' +
        '<td class="mono" style="word-break:break-all;overflow-wrap:anywhere;"><b>' + n.url + '</b></td>' +
        '<td class="mono" style="font-size:11px;color:var(--muted);white-space:nowrap;">' + when + '</td>' +
        '<td style="white-space:nowrap;">' + roleHtml + '</td>' +
        '<td style="white-space:nowrap;">' +
          '<div style="display:flex;gap:4px;flex-wrap:nowrap;align-items:center;">' +
            actBtn +
            '<button class="btn btn-sm" style="padding:2px 7px;font-size:11px;white-space:nowrap;color:var(--rose);border-color:rgba(239,68,68,0.3);" title="从候选池移除" onclick="poolRemove(' + JSON.stringify(n.url) + ')">移出</button>' +
          '</div>' +
        '</td>' +
      '</tr>';
    }).join('');
  } catch (err) {
    tbody.innerHTML = '<tr><td colspan="4" style="text-align:center;color:var(--rose);">读取池失败: ' + err.message + '</td></tr>';
  }
}

document.getElementById('btnBenchmarkPool').onclick = loadNodePoolBenchmark;

const btnElect = document.getElementById('btnElectPool');
if (btnElect) {
  btnElect.onclick = async () => {
    btnElect.disabled = true;
    btnElect.innerText = '🔄 正在全网测速并重选主备...';
    try {
      const res = await api('/pool/elect', 'POST', {});
      if (res.ok) {
        showToast('主备节点已重新选举：' + (res.chosen || []).join(' + '), 'var(--emerald)');
        await refreshAll();
        await loadManagedPool();
        await loadNodePoolBenchmark();
      }
    } catch (err) {
      showToast('重新选举失败: ' + err.message, 'var(--rose)');
    } finally {
      btnElect.disabled = false;
      btnElect.innerText = '🔄 重新选举最优主备';
    }
  };
}

window.poolActivate = async function (url) {
  if (!confirm('确认将以下节点切换为当前网络的【主连接节点】？ ' + url)) return;
  try {
    const res = await api('/pool/activate', 'POST', {url});
    if (res.ok) {
      showToast('已成功切换主连接节点为：' + url, 'var(--emerald)');
      await refreshAll();
      await loadManagedPool();
    }
  } catch (err) {
    showToast('切换主连接失败: ' + err.message, 'var(--rose)');
  }
};

// Operator-managed pool nodes: persist a probed endpoint into this network's candidate pool, or drop it.
window.poolAdd = async function (url) {
  try {
    const res = await api('/pool/add', 'POST', {url});
    if (res.ok) {
      const v = res.verified || {};
      showToast('已加入候选池：' + url + ' · ' + (v.networkId || '') + ' · p2p ' + String(v.p2pId || '-').slice(0, 8) + '. 下次探测或换血时参与选举。', 'var(--emerald)');
      await loadManagedPool();
      await loadNodePoolBenchmark();
    }
  } catch (err) {
    showToast('加入候选池失败: ' + err.message, 'var(--rose)');
  }
};

window.poolRemove = async function (url) {
  if (!confirm('确认从候选池移除该节点？ ' + url + ' —— 已建立的连接不受影响，下次探测或换血时生效。')) return;
  try {
    const res = await api('/pool/remove', 'POST', {url});
    if (res.ok) {
      showToast('已从候选池移除：' + url, 'var(--emerald)');
      await loadManagedPool();
      await loadNodePoolBenchmark();
    }
  } catch (err) {
    showToast('移除失败: ' + err.message, 'var(--rose)');
  }
};

// REST Hint Probe
document.getElementById('btnProbeRest').onclick = async () => {
  const btn = document.getElementById('btnProbeRest');
  const sub = document.getElementById('restEndpointSub');
  btn.disabled = true;
  btn.innerText = '正在握手 REST...';
  try {
    const res = await api('/probe-rest', 'POST', {});
    if (res.ok) {
      sub.innerHTML = '延迟: <b style="color:var(--emerald);">' + res.latencyMs + 'ms</b> · 版本: <b class="mono">' + res.serverVersion + '</b> · 同步: <b>' + (res.isSynced ? '🟢 已同步' : '🟡 同步中') + '</b> · UTXO索引: <b>' + (res.isUtxoIndexed ? '已开启' : '未开启') + '</b> · 内存池: <b>' + res.mempoolSize + ' 笔</b>';
      showToast('REST 探针在线 (RTT: ' + res.latencyMs + 'ms)', 'var(--emerald)');
    } else {
      sub.innerHTML = '<span style="color:var(--rose);">探测异常: ' + (res.error || res.message || '失败') + ' (RTT: ' + (res.latencyMs || '-') + 'ms)</span>';
      showToast('REST 探针连接失败', 'var(--rose)');
    }
  } catch (err) {
    sub.innerHTML = '<span style="color:var(--rose);">探测错误: ' + err.message + '</span>';
  } finally {
    btn.disabled = false;
    btn.innerText = '⚡ 探测 REST 探针状态';
  }
};

window.probeSingle = function(url) {
  document.getElementById('inputProbeUrl').value = url;
  document.getElementById('btnProbeNode').click();
};

// 3. Custom Node Probe
document.getElementById('btnProbeAddPool').onclick = async () => {
  const url = document.getElementById('inputProbeUrl').value.trim();
  const box = document.getElementById('probeResultBox');
  box.style.display = 'block';
  if (!(url.startsWith('ws://') || url.startsWith('wss://'))) { box.innerHTML = '<div class="card" style="border-color:var(--rose);">请输入以 ws:// 或 wss:// 开头的完整节点地址。</div>'; return; }
  box.innerHTML = '<div style="color:var(--muted);">正在校验该节点网络身份并写入候选池...</div>';
  try {
    const r = await api('/pool/add', 'POST', { url });
    if (!r.ok) throw new Error(r.error);
    const v = r.verified || {};
    box.innerHTML = '<div class="card" style="background:var(--bg-panel-elevated);border-color:var(--emerald);">' +
      '<strong style="color:var(--emerald);">✓ 已加入候选池</strong>' +
      '<div style="font-size:12px;line-height:1.7;margin-top:6px;">' +
        '节点: <span class="mono" style="word-break:break-all;">' + url + '</span><br>' +
        '网络: <b class="mono">' + (v.networkId || '-') + '</b> · p2p: <b class="mono">' + String(v.p2pId || '-').slice(0, 12) + '</b> · 版本: <b class="mono">' + (v.serverVersion || '-') + '</b><br>' +
        'DAA: <b class="mono">' + (v.virtualDaaScore || '-') + '</b> · 同步 ✓ · UTXO 索引 ✓' +
      '</div>' +
      '<div style="font-size:11px;color:var(--muted);margin-top:6px;">已持久化到当前网络的数据库；下次探测或主备换血时参与选举。</div>' +
    '</div>';
    showToast('已加入候选池：' + url, 'var(--emerald)');
  } catch (err) {
    box.innerHTML = '<div class="card" style="border-color:var(--rose);background:rgba(239,68,68,0.1);">' +
      '<strong style="color:var(--rose);">❌ 未能加入候选池</strong>' +
      '<div style="font-size:12px;margin-top:6px;color:var(--muted);word-break:break-all;">' + err.message + '</div>' +
      '<div style="font-size:11px;color:var(--muted);margin-top:6px;">节点必须与当前查看的网络一致、已同步、且开启 UTXO 索引。</div>' +
    '</div>';
  }
};

document.getElementById('btnProbeNode').onclick = async () => {
  const url = document.getElementById('inputProbeUrl').value.trim();
  const box = document.getElementById('probeResultBox');
  box.style.display = 'block';
  box.innerHTML = '<div style="color:var(--muted);">正在握手连接并探测 DAA 与高度...</div>';

  try {
    const r = await api('/test-node', 'POST', { url });
    if (!r.ok) throw new Error(r.error);
    box.innerHTML = '<div class="card" style="background:var(--bg-panel-elevated);border-color:var(--kaspa);">' +
      '<div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;">' +
        '<strong style="color:var(--emerald);">✓ 节点连接成功</strong>' +
        '<span class="mono" style="color:var(--kaspa);font-weight:700;">' + r.rttMs + ' ms 往返延迟</span>' +
      '</div>' +
      '<div style="font-size:12px;line-height:1.7;">' +
        '网络: <b class="mono">' + r.info?.networkId + '</b> · 服务端版本: <b class="mono">' + r.info?.serverVersion + '</b><br>' +
        '同步状态: <b>' + (r.info?.isSynced ? '🟢 已同步 (Synced)' : '🟡 追块中 (Catching-up)') + '</b> · UTXO 索引: <b>' + (r.info?.hasUtxoIndex ? '已开启' : '未开启') + '</b><br>' +
        '链尖 DAA: <b class="mono">' + (r.info?.virtualDaaScore || '-') + '</b> · 当前 Sink 块: <span class="mono" style="font-size:11px;color:var(--muted);word-break:break-all;">' + r.dag?.sink + '</span>' +
      '</div>' +
    '</div>';
  } catch (err) {
    box.innerHTML = '<div class="card" style="border-color:var(--rose);background:rgba(239,68,68,0.1);">' +
      '<strong style="color:var(--rose);">❌ 节点探测失败</strong>' +
      '<div style="font-size:12px;margin-top:6px;color:var(--muted);word-break:break-all;">' + err.message + '</div>' +
    '</div>';
  }
};

// 4. Contract Plugins Management
async function loadPluginsTab() {
  try {
    const res = await api('/plugins');
    window._pluginsData = res;

    // Show currently viewed network badge in Tab 3 header
    const netBadge = document.getElementById('pluginsNetBadge');
    if (netBadge) {
      const curNet = res.networkKey || selectedNetwork || 'tn10';
      netBadge.style.display = 'inline-block';
      netBadge.innerText = curNet.toUpperCase();
    }

    const tbody = document.getElementById('tblPluginList');
    const list = res.plugins || [];
    const cfgList = res.config || [];

    tbody.innerHTML = cfgList.map((cfg, idx) => {
      const p = list[idx] || {};
      const watchAddrs = p.config?.registryAddresses || p.watchAddresses || cfg.options?.registryAddresses || [];
      const regCount = watchAddrs.length;
      let watchText = '<span style="color:var(--dim);">无监听</span>';
      if (regCount > 0) {
        const first = watchAddrs[0];
        const shortAddr = first.length > 26 ? (first.slice(0, 14) + '...' + first.slice(-8)) : first;
        watchText = '<span class="tag tag-active mono" title="' + watchAddrs.join(', ') + ' (点击复制)" onclick="copyHash(' + JSON.stringify(first) + ', \\'监听地址\\')" style="font-size:11px;cursor:pointer;">' +
          (regCount === 1 ? shortAddr : (regCount + ' 个地址 (' + shortAddr + ')')) + ' 📋' +
          '</span>';
      }
      const keyName = p.key || (cfg.options?.id ? (cfg.options.id + '@' + (cfg.options.version || '1.0.0')) : cfg.module);

      return '<tr>' +
        '<td class="mono" style="word-break:break-all;"><b>' + keyName + '</b></td>' +
        '<td class="mono" style="white-space:nowrap;">' + (cfg.module || '-') + '</td>' +
        '<td class="mono" style="font-size:12px;color:var(--muted);word-break:break-all;overflow-wrap:anywhere;">' + (p.profileId ? renderHash(p.profileId, 'Profile ID') : '-') + '</td>' +
        '<td style="white-space:nowrap;">' + watchText + '</td>' +
        '<td style="white-space:nowrap;">' + (cfg.retired ? '<span class="tag tag-retired">RETIRED (兼容)</span>' : '<span class="tag tag-active">ACTIVE (主导)</span>') + '</td>' +
        '<td>' +
          '<div style="display:flex;gap:4px;flex-wrap:nowrap;align-items:center;">' +
            '<button class="btn btn-sm btn-pri" style="padding:2px 7px;font-size:11px;white-space:nowrap;" onclick="openPluginConfigModal(' + idx + ')">⚙️ 配置</button>' +
            '<button class="btn btn-sm" style="padding:2px 7px;font-size:11px;white-space:nowrap;" onclick="togglePlugin(' + idx + ', ' + (!cfg.retired) + ')">' +
              (cfg.retired ? '🟢 激活' : '⏸️ 退役') +
            '</button>' +
            '<button class="btn btn-sm" style="padding:2px 7px;font-size:11px;white-space:nowrap;color:var(--rose);border-color:rgba(239,68,68,0.3);" onclick="removePlugin(' + idx + ', \\'' + (p.key || cfg.module) + '\\')">🗑️ 卸载</button>' +
          '</div>' +
        '</td>' +
      '</tr>';
    }).join('');
  } catch (err) {
    console.error(err);
  }
}

window.removePlugin = async function(index, name) {
  if (!confirm('确认要卸载并移除插件 [' + name + '] 吗？\\n此操作将从索引器清单中注销该插件，停止其对新轮次的监听。')) return;
  try {
    const res = await api('/plugins/remove', 'POST', { index });
    if (res.ok) {
      showToast('插件 [' + name + '] 已成功卸载！', 'var(--emerald)');
      await refreshAll();
      await loadPluginsTab();
    }
  } catch (err) {
    showToast('卸载失败: ' + err.message, 'var(--rose)');
  }
};

// Open Configuration Modal for any plugin in Tab 3
window.openPluginConfigModal = function(idx) {
  const data = window._pluginsData;
  if (!data || !data.config || !data.config[idx]) return;

  window._currentConfigIndex = idx;
  const cfg = data.config[idx] || {};
  const p = (data.plugins || []).find(x => x.module === cfg.module || x.module === './' + cfg.module) || data.plugins[idx] || {};
  const opts = cfg.options || {};
  // EFFECTIVE runtime config of the running plugin (registry already resolved for the selected network).
  const eff = p.config || {};
  const configurable = Array.isArray(p.configurable) ? p.configurable : null;
  const inert = k => configurable && !configurable.includes(k);

  document.getElementById('cfgModalPluginKey').innerText = p.key || (opts.id + '@' + (opts.version || '1.0.0'));
  document.getElementById('cfgActiveToggle').checked = !cfg.retired;

  // Registry address: ALWAYS show what the running plugin actually watches (per-network, injected). Never a TN10 default.
  const effRegistry = eff.registryAddresses || [];
  const effNetwork = eff.network || data.networkKey || null;
  const curRegistry = (opts.registryAddresses && opts.registryAddresses.length) ? opts.registryAddresses : effRegistry;
  document.getElementById('cfgRegistryAddr').value = curRegistry.join(', ');

  // The "fill official registry" button: fills the currently viewed network's official registry!
  const fillBtn = document.getElementById('btnFillRegistry');
  if (fillBtn) {
    fillBtn.innerText = '填入当前网络 (' + (effNetwork ? effNetwork.toUpperCase() : '当前') + ') 官方 Registry';
    fillBtn.onclick = () => {
      if (effRegistry.length) {
        document.getElementById('cfgRegistryAddr').value = effRegistry.join(', ');
      }
    };
  }
  const regLabel = document.querySelector('label[for="cfgRegistryAddr"]') || document.getElementById('cfgRegistryAddr').previousElementSibling;
  if (regLabel) regLabel.innerText = '监听的注册表地址 (逗号分隔)' + (effNetwork ? ' · 当前网络: ' + effNetwork : '');

  // Registration fee (sompi, default 20000000 = 0.2 KAS)
  document.getElementById('cfgRegSompi').value = opts.registrationSompi || eff.registrationSompi || '20000000';

  // Required deposit: 400000000 (4 KAS)
  document.getElementById('cfgReqDeposit').value = opts.requiredDeposit || eff.requiredDeposit || '400000000';

  // Allowed creators (optional whitelist)
  document.getElementById('cfgAllowedCreators').value = (opts.allowedCreators || []).join(', ');

  // Render dynamic field guards
  const listEl = document.getElementById('cfgDynamicFieldsList');
  const savedGuards = opts.initialStateGuards || eff.initialStateGuards || {};
  const hasSavedGuards = Boolean(savedGuards && Object.keys(savedGuards).length > 0);

  // Standard Kaswin fields specification:
  // - sold=0, phase=1, purchaseCount=0, cursor=0 are true fixed invariants -> checked by default!
  // - ticketPrice, ticketCap, minTickets, purchaseCap are per-round parameters -> UNCHECKED unless set!
  const standardFields = [
    { key: 'phase', label: '阶段 (phase)', defaultVal: '1', defaultChecked: true, note: '创世初始必须为 1-OPEN' },
    { key: 'sold', label: '已售票数 (sold)', defaultVal: '0', defaultChecked: true, note: '创世初始必须为 0' },
    { key: 'purchaseCount', label: '购买次数 (purchaseCount)', defaultVal: '0', defaultChecked: true, note: '创世初始必须为 0' },
    { key: 'cursor', label: '退款游标 (cursor)', defaultVal: '0', defaultChecked: true, note: '创世初始必须为 0' },
    { key: 'ticketPrice', label: '单票价格 (ticketPrice)', defaultVal: '100000000', defaultChecked: false, note: '创世自定义 (未指定则不限)' },
    { key: 'ticketCap', label: '最大总票数 (ticketCap)', defaultVal: '4', defaultChecked: false, note: '创世自定义 (未指定则不限)' },
    { key: 'minTickets', label: '最低成团票数 (minTickets)', defaultVal: '3', defaultChecked: false, note: '创世自定义 (未指定则不限)' },
    { key: 'purchaseCap', label: '最大购买人次 (purchaseCap)', defaultVal: '256', defaultChecked: false, note: '创世自定义 (未指定则不限)' }
  ];

  let html = '<div style="color:var(--kaspa);font-size:11px;font-weight:700;margin-bottom:6px;">📦 创世业务状态机参数断言 (勾选的项将被严格比对，未勾选不限制):</div>';

  html += standardFields.map(f => {
    let isChecked, val;
    if (hasSavedGuards) {
      isChecked = (savedGuards[f.key] !== undefined);
      val = isChecked ? savedGuards[f.key] : f.defaultVal;
    } else {
      isChecked = f.defaultChecked;
      val = f.defaultVal;
    }
    return '<div class="field-invariant-row" style="display:flex;align-items:center;gap:8px;background:rgba(0,0,0,0.25);padding:6px 10px;border-radius:var(--radius-sm);border:1px solid var(--border);flex-wrap:wrap;margin-bottom:4px;">' +
      '<label style="display:flex;align-items:center;gap:6px;min-width:180px;cursor:pointer;">' +
        '<input type="checkbox" class="field-guard-toggle" data-name="' + f.key + '"' + (isChecked ? ' checked' : '') + '>' +
        '<strong class="mono" style="color:var(--text);font-size:12px;">' + f.key + '</strong>' +
        '<span style="font-size:10.5px;color:var(--muted);">' + (f.note ? '(' + f.note + ')' : '') + '</span>' +
      '</label>' +
      '<div style="flex:1;display:flex;align-items:center;gap:6px;min-width:160px;">' +
        '<span style="color:var(--muted);font-size:11px;white-space:nowrap;">必须等于:</span>' +
        '<input type="text" class="field-guard-val mono" data-name="' + f.key + '" value="' + val + '" style="padding:4px 8px;font-size:12px;height:28px;">' +
      '</div>' +
    '</div>';
  }).join('');

  // Also render any custom non-standard fields that were saved
  for (const [k, v] of Object.entries(savedGuards)) {
    if (!standardFields.some(f => f.key === k)) {
      html += '<div class="field-invariant-row" style="display:flex;align-items:center;gap:8px;background:rgba(0,0,0,0.25);padding:6px 10px;border-radius:var(--radius-sm);border:1px solid var(--border);flex-wrap:wrap;margin-bottom:4px;">' +
        '<label style="display:flex;align-items:center;gap:6px;min-width:180px;cursor:pointer;">' +
          '<input type="checkbox" class="field-guard-toggle" data-name="' + k + '" checked>' +
          '<strong class="mono" style="color:var(--text);font-size:12px;">' + k + '</strong>' +
        '</label>' +
        '<div style="flex:1;display:flex;align-items:center;gap:6px;min-width:160px;">' +
          '<span style="color:var(--muted);font-size:11px;white-space:nowrap;">必须等于:</span>' +
          '<input type="text" class="field-guard-val mono" data-name="' + k + '" value="' + v + '" style="padding:4px 8px;font-size:12px;height:28px;">' +
        '</div>' +
      '</div>';
    }
  }

  listEl.innerHTML = html;

  document.getElementById('pluginConfigModal').style.display = 'flex';
};

window.addCustomStateGuardRow = function() {
  const name = prompt('请输入要严格约束的初始状态变量名 (例如 phase, sold, step):');
  if (!name || !name.trim()) return;
  const cleanName = name.trim();
  const listEl = document.getElementById('cfgDynamicFieldsList');
  if (!listEl) return;

  const row = document.createElement('div');
  row.className = 'field-invariant-row';
  row.style.cssText = 'display:flex;align-items:center;gap:8px;background:rgba(0,0,0,0.25);padding:6px 10px;border-radius:var(--radius-sm);border:1px solid var(--border);flex-wrap:wrap;';
  row.innerHTML = '<label style="display:flex;align-items:center;gap:6px;min-width:140px;cursor:pointer;">' +
      '<input type="checkbox" class="field-guard-toggle" data-name="' + cleanName + '" checked>' +
      '<strong class="mono" style="color:var(--text);font-size:12px;">' + cleanName + '</strong>' +
      '<span class="tag tag-retired" style="font-size:10px;">guard</span>' +
    '</label>' +
    '<div style="flex:1;display:flex;align-items:center;gap:6px;min-width:160px;">' +
      '<span style="color:var(--muted);font-size:11px;white-space:nowrap;">创世必须等于:</span>' +
      '<input type="text" class="field-guard-val mono" data-name="' + cleanName + '" value="0" style="padding:4px 8px;font-size:12px;height:28px;">' +
    '</div>';
  listEl.appendChild(row);
};

// Save Configuration from Modal
document.getElementById('btnSavePluginConfig').onclick = async function() {
  const idx = window._currentConfigIndex;
  if (idx === undefined || idx === null) return;

  const regRaw = document.getElementById('cfgRegistryAddr').value.trim();
  const registryAddresses = regRaw ? regRaw.split(',').map(s => s.trim()).filter(Boolean) : [];
  const registrationSompi = document.getElementById('cfgRegSompi').value.trim() || '0';
  const requiredDeposit = document.getElementById('cfgReqDeposit').value.trim() || null;
  const creatorRaw = document.getElementById('cfgAllowedCreators').value.trim();
  const allowedCreators = creatorRaw ? creatorRaw.split(',').map(s => s.trim()).filter(Boolean) : [];
  const retired = !document.getElementById('cfgActiveToggle').checked;

  const initialStateGuards = {};
  document.querySelectorAll('#cfgDynamicFieldsList .field-invariant-row').forEach(row => {
    const toggle = row.querySelector('.field-guard-toggle');
    const input = row.querySelector('.field-guard-val');
    if (toggle && toggle.checked && input && input.value.trim() !== '') {
      initialStateGuards[toggle.dataset.name] = input.value.trim();
    }
  });

  const btn = this;
  btn.disabled = true;
  btn.innerText = '正在保存配置并热生效...';

  try {
    const res = await api('/plugins/update-config', 'POST', {
      index: idx,
      retired,
      options: {
        registryAddresses,
        registrationSompi,
        requiredDeposit,
        allowedCreators,
        initialStateGuards
      }
    });

    if (res.ok) {
      document.getElementById('pluginConfigModal').style.display = 'none';
      showToast('🎉 契约规则配置已保存并热生效！', 'var(--emerald)');
      await refreshAll();
      await loadPluginsTab();
    }
  } catch (err) {
    showToast('保存配置失败: ' + err.message, 'var(--rose)');
  } finally {
    btn.disabled = false;
    btn.innerText = '💾 保存配置并热生效';
  }
};

window.togglePlugin = async function(index, newRetired) {
  try {
    await api('/plugins/toggle', 'POST', { index, retired: newRetired });
    showToast('插件状态已更新！', 'var(--emerald)');
    loadPluginsTab();
    refreshAll();
  } catch (err) {
    showToast('切换失败: ' + err.message, 'var(--rose)');
  }
};

document.getElementById('btnReloadPlugins').onclick = () => {
  loadPluginsTab();
  showToast('插件列表已刷新');
};

// 5. SilverScript Studio Workbench (Pure Compiler & Generator)
const PRESET_CONTRACTS = {
  counter: "contract Decls(int init_value) {\\n    int value = init_value;\\n\\n    #[covenant.singleton(mode = transition)]\\n    function bump(State prev_state, int delta) : (State) {\\n        return(State { value: prev_state.value + delta });\\n    }\\n}",
  empty: "contract CustomContract() {\\n    int count = 0;\\n\\n    #[covenant.singleton(mode = transition)]\\n    function step(State prev_state) : (State) {\\n        return(State { count: prev_state.count + 1 });\\n    }\\n}"
};

const editor = document.getElementById('silEditor');
if (editor) editor.value = PRESET_CONTRACTS.counter;

const presetSelect = document.getElementById('selectContractPreset');
if (presetSelect) {
  presetSelect.onchange = async e => {
    const v = e.target.value;
    if (v.startsWith('v201-')) {
      showToast('正在从本地载入 V2.0.1 契约源码与参数...', 'var(--kaspa)');
      try {
        const res = await api('/preset-contract?name=' + v);
        if (res.ok && editor) {
          editor.value = res.source;
          const ctorInput = document.getElementById('inputCtorArgs');
          if (ctorInput) ctorInput.value = JSON.stringify(res.constructorArgs);
          showToast('已载入 ' + res.name + ' 契约源码！', 'var(--emerald)');
        }
      } catch (err) {
        showToast('载入预设失败: ' + err.message, 'var(--rose)');
      }
      return;
    }
    if (PRESET_CONTRACTS[v] && editor) {
      editor.value = PRESET_CONTRACTS[v];
      const ctorInput = document.getElementById('inputCtorArgs');
      if (ctorInput) ctorInput.value = v === 'counter' ? '[0]' : '[]';
    }
  };
}

const btnUpload = document.getElementById('btnUploadSil');
const fileInput = document.getElementById('fileUploadSil');
if (btnUpload && fileInput) {
  btnUpload.onclick = () => fileInput.click();
  fileInput.onchange = e => {
    const file = e.target.files && e.target.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = evt => {
      if (editor) editor.value = evt.target.result;
      showToast('已载入本地文件: ' + file.name, 'var(--emerald)');
    };
    reader.readAsText(file);
  };
}

window.copyTemplateHash = function(el) {
  const hash = el.getAttribute('data-hash');
  if (hash && navigator.clipboard) {
    navigator.clipboard.writeText(hash);
    showToast('已复制模版哈希！', 'var(--emerald)');
  }
};

async function runCompilation(astOnly) {
  const box = document.getElementById('compilerResultBox');
  if (!box || !editor) return;
  box.style.display = 'block';
  box.innerHTML = '<div class="card" style="color:var(--muted);">正在调用 silverc 编译器执行' + (astOnly ? '语法解析' : '全量编译') + '...</div>';

  const source = editor.value;
  let constructorArgs = null;
  const ctorRaw = (document.getElementById('inputCtorArgs')?.value || '').trim();
  if (ctorRaw) {
    try {
      constructorArgs = JSON.parse(ctorRaw);
      window._lastConstructorArgs = constructorArgs;
    } catch (err) {
      box.innerHTML = '<div class="card" style="border-color:var(--rose);color:var(--rose);">构造函数参数不是合法的 JSON 格式: ' + err.message + '</div>';
      return;
    }
  }

  try {
    const res = await api('/compile', 'POST', {
      source,
      filename: 'contract.sil',
      constructorArgs,
      astOnly: Boolean(astOnly)
    });

    if (astOnly) {
      renderAstResult(res, box);
    } else {
      renderCompileResult(res, box);
    }
  } catch (err) {
    box.innerHTML = '<div class="card" style="border-color:var(--rose);background:rgba(239,68,68,0.1);">' +
      '<strong style="color:var(--rose);">❌ 编译失败 (Compiler Error)</strong>' +
      '<pre class="mono" style="margin-top:10px;font-size:12px;color:var(--text);white-space:pre-wrap;word-break:break-all;">' +
      (err.message || '未知编译错误') +
      '</pre></div>';
  }
}

function renderAstResult(data, box) {
  const fields = (data.fields || []).map(f => '<tr><td class="mono">' + f.name + '</td><td class="mono">' + f.type + '</td></tr>').join('') || '<tr><td colspan="2" style="color:var(--muted);">无独立字段</td></tr>';
  const functions = (data.functions || []).map(fn => '<tr><td class="mono"><b>' + fn.name + '</b></td><td class="mono">' + (fn.params || []).map(p => (p.type || '') + ' ' + (p.name || '')).join(', ') + '</td></tr>').join('') || '<tr><td colspan="2" style="color:var(--muted);">无方法</td></tr>';

  box.innerHTML = '<div class="section-title" style="margin-bottom:12px;">🔍 语法解析成功 (AST 解析结果)</div>' +
    '<div class="grid-cards" style="margin-bottom:16px;">' +
      '<div class="card"><div class="card-label">契约名称</div><div class="card-val mono" style="font-size:18px;">' + data.contractName + '</div></div>' +
      '<div class="card"><div class="card-label">状态字段数</div><div class="card-val mono">' + (data.fields ? data.fields.length : 0) + '</div></div>' +
      '<div class="card"><div class="card-label">方法入口数</div><div class="card-val mono">' + (data.functions ? data.functions.length : 0) + '</div></div>' +
    '</div>' +
    '<div class="grid-2col">' +
      '<div class="card" style="padding:0;overflow:hidden;">' +
        '<div style="padding:12px 14px;font-weight:700;border-bottom:1px solid var(--border);">📊 状态变量声明 (State Fields)</div>' +
        '<div class="table-wrap table-wrap-compact"><table><thead><tr><th>字段名</th><th>类型</th></tr></thead><tbody>' + fields + '</tbody></table></div>' +
      '</div>' +
      '<div class="card" style="padding:0;overflow:hidden;">' +
        '<div style="padding:12px 14px;font-weight:700;border-bottom:1px solid var(--border);">⚡ 方法声明 (Functions)</div>' +
        '<div class="table-wrap table-wrap-compact"><table><thead><tr><th>方法名</th><th>参数签名</th></tr></thead><tbody>' + functions + '</tbody></table></div>' +
      '</div>' +
    '</div>';
}

function renderCompileResult(data, box) {
  const c = data.contracts && data.contracts[0];
  if (!c) {
    box.innerHTML = '<div class="card">未生成有效合约产物</div>';
    return;
  }

  window._lastArtifact = data.artifact;
  window._lastContractName = c.name;

  const entriesHtml = (c.entries || []).map(e => '<tr>' +
    '<td class="mono"><b>' + e.key + '</b></td>' +
    '<td><span class="tag tag-active mono">' + e.dispatchTag + '</span></td>' +
    '<td class="mono">' + (e.params || []).map(p => (p.type?.kind || 'any') + ' ' + p.name).join(', ') + '</td>' +
  '</tr>').join('') || '<tr><td colspan="3" style="color:var(--muted);">无外部入口</td></tr>';

  box.innerHTML = '<div class="section-title" style="margin-bottom:12px;">🎉 契约编译成功 (SilverScript Artifact)</div>' +
    '<div class="card" style="border-color:var(--kaspa);background:rgba(112,199,186,0.06);margin-bottom:16px;">' +
      '<div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;flex-wrap:wrap;gap:6px;">' +
        '<div class="card-label" style="color:var(--kaspa);font-weight:700;margin:0;">模版哈希 (Template Hash · 32-Byte)</div>' +
        '<button class="btn btn-sm" data-hash="' + c.templateHashHex + '" onclick="window.copyTemplateHash(this)">📋 复制哈希</button>' +
      '</div>' +
      '<div class="mono" style="font-size:14px;color:var(--text);word-break:break-all;font-weight:600;">' + c.templateHashHex + '</div>' +
      '<div style="font-size:11.5px;color:var(--muted);margin-top:6px;">字节码大小: ' + c.bytecodeLength + ' 字节 | 状态切片偏移: ' + c.stateSpan.offset + ' (长度 ' + c.stateSpan.len + ')</div>' +
    '</div>' +
    '<div class="card" style="padding:0;overflow:hidden;margin-bottom:16px;">' +
      '<div style="padding:12px 14px;font-weight:700;border-bottom:1px solid var(--border);">⚡ 方法入口与调度标签 (Entrypoints & Dispatch Tags)</div>' +
      '<div class="table-wrap"><table><thead><tr><th>方法标识</th><th>Dispatch Tag</th><th>入参</th></tr></thead><tbody>' + entriesHtml + '</tbody></table></div>' +
    '</div>' +
    '<div style="display:flex;gap:10px;margin-bottom:16px;flex-wrap:wrap;">' +
      '<button class="btn btn-pri" style="flex:2;min-width:220px;justify-content:center;height:40px;" onclick="saveAndGoToPlugins()">' +
        '💾 保存插件并前往【插件管理】配置规则 →' +
      '</button>' +
      '<button class="btn" style="flex:1;min-width:120px;justify-content:center;height:40px;" onclick="viewGeneratedPluginCode()">' +
        '📜 查看插件源码' +
      '</button>' +
    '</div>' +
    '<details class="card" style="cursor:pointer;"><summary style="font-weight:600;color:var(--muted);">📦 查看完整编译器 JSON Artifact</summary>' +
      '<pre class="mono" style="margin-top:12px;font-size:11px;overflow:auto;max-height:300px;">' + JSON.stringify(data.artifact, null, 2) + '</pre>' +
    '</details>';
}

// Save compiled plugin directly and open configuration in Tab 3
window.saveAndGoToPlugins = async function() {
  if (!window._lastArtifact) return;
  const cName = window._lastContractName || Object.keys(window._lastArtifact.contracts || {})[0];
  const pId = (cName || 'contract').toLowerCase();
  const filename = pId + '.mjs';

  showToast('正在生成并保存插件 ' + filename + '...', 'var(--kaspa)');

  try {
    const genRes = await api('/generate-plugin', 'POST', {
      artifact: window._lastArtifact,
      contractName: cName,
      pluginId: pId
    });

    if (!genRes.ok) throw new Error(genRes.detail || '生成插件失败');

    // Auto-extract initial parameters if available
    const decoded = decodeLedgerFromArgs(window._lastConstructorArgs);
    let initialGuards = null;
    let reqDeposit = null;
    let regAddresses = [];
    let regSompi = '5000000';

    if (decoded) {
      reqDeposit = '400000000';
      regAddresses = ['kaspatest:qztrjpfpuf6g9jw76e6ay909z43hndpv5maqkker6ur4enz7llarsh82ezggl'];
      initialGuards = {
        phase: '1',
        sold: '0',
        purchaseCount: '0',
        cursor: '0',
        ticketPrice: decoded.ticketPrice,
        ticketCap: String(decoded.ticketCap),
        minTickets: String(decoded.minTickets),
        purchaseCap: String(decoded.purchaseCap)
      };
    } else {
      const cData = window._lastArtifact?.contracts?.[cName];
      const fields = cData?.runtime_state?.fields || [];
      if (fields.length) {
        initialGuards = {};
        for (const f of fields) {
          initialGuards[f.name] = (f.name === 'phase') ? '1' : '0';
        }
      }
    }

    const instRes = await api('/install-plugin', 'POST', {
      filename,
      pluginId: pId,
      code: genRes.code,
      templateHash: genRes.templateHashHex,
      requiredDeposit: reqDeposit,
      registryAddresses: regAddresses,
      registrationSompi: regSompi,
      initialStateGuards: initialGuards,
      activate: true
    });

    if (instRes.ok) {
      showToast('🎉 插件已成功保存！已自动提取并注入创世参数', 'var(--emerald)');
      await refreshAll();
      await loadPluginsTab();
      // Switch to Plugins Tab
      document.querySelector('[data-tab="tab-plugins"]').click();
      // Auto open config modal on the EXACT installed plugin
      const targetIdx = (window._pluginsData?.config || []).findIndex(c => c.module === filename || c.module === './' + filename);
      const openIdx = targetIdx >= 0 ? targetIdx : 0;
      setTimeout(() => openPluginConfigModal(openIdx), 200);
    }
  } catch (err) {
    showToast('保存插件失败: ' + err.message, 'var(--rose)');
  }
};

window.viewGeneratedPluginCode = async function() {
  if (!window._lastArtifact) return;
  const cName = window._lastContractName || Object.keys(window._lastArtifact.contracts || {})[0];
  const pId = (cName || 'contract').toLowerCase();

  try {
    const res = await api('/generate-plugin', 'POST', {
      artifact: window._lastArtifact,
      contractName: cName,
      pluginId: pId
    });
    if (res.ok) {
      document.getElementById('pluginViewCodeArea').value = res.code;
      document.getElementById('pluginCodeModal').style.display = 'flex';
    }
  } catch (err) {
    showToast('查看源码失败: ' + err.message, 'var(--rose)');
  }
};

window.copyPluginViewCode = function() {
  const code = document.getElementById('pluginViewCodeArea').value;
  if (navigator.clipboard) {
    navigator.clipboard.writeText(code);
    showToast('已复制插件源码！', 'var(--emerald)');
  }
};

const btnAst = document.getElementById('btnAstOnly');
if (btnAst) btnAst.onclick = () => runCompilation(true);
const btnComp = document.getElementById('btnCompileSil');
if (btnComp) btnComp.onclick = () => runCompilation(false);

// Tabs Switcher
document.querySelectorAll('.tab-btn').forEach(btn => {
  btn.onclick = () => {
    document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
    document.querySelectorAll('.view-panel').forEach(p => p.classList.remove('active'));
    btn.classList.add('active');
    document.getElementById(btn.dataset.tab).classList.add('active');
    if (btn.dataset.tab === 'tab-plugins') loadPluginsTab();
    if (btn.dataset.tab === 'tab-nodes') {
      loadManagedPool();
      if (!window._poolLoaded) { window._poolLoaded = true; loadNodePoolBenchmark(); }
    }
  };
});

// Auth Form Submit
document.getElementById('authForm').onsubmit = async e => {
  e.preventDefault();
  const token = document.getElementById('inputAdminToken').value.trim();
  if (token) {
    try {
      const chk = await fetch(apiPrefix + '/verify-token', {
        method: 'POST',
        headers: {'Authorization': 'Bearer ' + token}
      });
      if (!chk.ok) throw new Error('WRONG_PASSWORD');
      currentToken = token;
      sessionStorage.setItem('kci_admin_token', token);
      updateAuthUI();
      showToast('管理员已解锁！', 'var(--emerald)');
      document.getElementById('authModal').style.display = 'none';
      if (window._pendingAction) {
        const action = window._pendingAction;
        window._pendingAction = null;
        action();
      }
    } catch (err) {
      showToast('密码错误，请重新输入', 'var(--rose)');
    }
  }
};

document.getElementById('btnCancelAuth').onclick = () => {
  document.getElementById('authModal').style.display = 'none';
};

document.getElementById('btnLogout').onclick = () => {
  document.getElementById('authModal').style.display = 'flex';
};

document.getElementById('btnRefresh').onclick = async () => {
  await refreshAll();
  const activeTab = document.querySelector('.view-panel.active')?.id;
  if (activeTab === 'tab-plugins') await loadPluginsTab();
  else if (activeTab === 'tab-nodes') await loadNodePoolBenchmark();
  showToast('已刷新最新状态');
};

// Auto refresh overview every 5s
setInterval(() => {
  if (document.getElementById('tab-overview').classList.contains('active')) {
    refreshAll();
  }
}, 5000);

refreshAll();
</script>
</body>
</html>
`;
