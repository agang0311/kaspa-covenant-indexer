// Embedded single-file Web Dashboard for Kaspa Covenant Indexer (KCI)
// Fully responsive across Mobile & Desktop. Zero viewport spillover.
// Features Kaswin-style Node Pool Matrix, SilverScript Compiler Workbench, and Visual Consensus Event Feed.

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

/* Header Layout */
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
  gap: 10px;
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
  gap: 8px;
  min-width: 0;
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
.brand-title {
  font-size: 14px;
  font-weight: 700;
  color: var(--text);
  white-space: nowrap;
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
  .btn-header { padding: 4px 8px; }
  .brand-title { font-size: 13.5px; }
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

/* Compiler Workbench Specific Responsive Styles */
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
}
.journal-card {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 12px 14px;
  transition: border-color 0.15s;
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
}
.journal-body {
  font-size: 12px;
  line-height: 1.6;
}
.journal-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 4px 12px;
}
.journal-field {
  display: flex;
  gap: 6px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.journal-field-label {
  color: var(--muted);
  flex-shrink: 0;
}
.journal-field-val {
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
</head>
<body>

<div class="top-nav-bar">
  <header>
    <div class="header-brand-line">
      <div class="brand">
        <span class="logo-badge">KCI</span>
        <span class="brand-title">Kaspa Covenant Indexer</span>
      </div>
      <div class="header-btns">
        <button class="btn btn-header" id="btnRefresh" title="刷新状态">🔄<span class="btn-text"> 刷新</span></button>
        <button class="btn btn-header" id="btnLogout" title="设置/更换管理员令牌">🔐<span class="btn-text"> 令牌</span></button>
      </div>
    </div>
    <div class="header-status-line">
      <div id="headerStatusPill" class="status-pill"><span class="dot"></span><span id="pillText">连接中...</span></div>
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
      <button class="btn btn-pri btn-sm" id="btnBenchmarkPool">⚡ 一键测速候选节点池 (Benchmark Pool)</button>
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

    <!-- 候选节点池列表 -->
    <div class="card" style="padding:0;overflow:hidden;margin-bottom:16px;">
      <div style="padding:12px 14px;border-bottom:1px solid var(--border);display:flex;justify-content:space-between;align-items:center;flex-wrap:wrap;gap:6px;">
        <span style="font-weight:700;">🌐 候选节点池健康矩阵 (TN10 Seeds & Pool)</span>
        <span style="color:var(--muted);font-size:11px;">注：标记【共享后端】的节点属于同一物理实例，不可互为双备</span>
      </div>
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>候选节点地址</th>
              <th>往返延迟 RTT</th>
              <th>落后块数 (Lag)</th>
              <th>同步与 UTXO 状态</th>
              <th>P2P 后端标识</th>
              <th>操作</th>
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
        输入任意自定义 Kaspa wRPC 节点（支持 ws:// 或 wss://，需开放 Borsh 端口），即时探测网络、延迟与 DAG Sink 状态：
      </div>
      <div style="display:flex;gap:8px;margin-bottom:10px;flex-wrap:wrap;">
        <input type="text" id="inputProbeUrl" class="mono" placeholder="ws://127.0.0.1:17210 或 wss://..." value="wss://quark-10.kaspa.red/kaspa/testnet-10/wrpc/borsh" style="flex:1;min-width:200px;">
        <button class="btn btn-sm" id="btnProbeNode" style="white-space:nowrap;">🚀 测速与体检</button>
      </div>
      <div style="display:flex;gap:6px;align-items:center;flex-wrap:wrap;font-size:11px;color:var(--muted);">
        <span>快捷测试:</span>
        <button class="btn btn-sm" type="button" onclick="probeSingle('ws://127.0.0.1:17210')">本地 .201 (17210)</button>
        <button class="btn btn-sm" type="button" onclick="probeSingle('wss://vector-10.kaspa.green/kaspa/testnet-10/wrpc/borsh')">vector-10</button>
        <button class="btn btn-sm" type="button" onclick="probeSingle('wss://quark-10.kaspa.red/kaspa/testnet-10/wrpc/borsh')">quark-10</button>
        <button class="btn btn-sm" type="button" onclick="probeSingle('wss://muon-10.kaspa.blue/kaspa/testnet-10/wrpc/borsh')">muon-10</button>
      </div>
      <div id="probeResultBox" style="display:none;margin-top:12px;"></div>
    </div>
  </section>

  <!-- TAB 3: 契约插件管理 -->
  <section id="tab-plugins" class="view-panel">
    <div class="section-head">
      <div>
        <div class="section-title">🧩 Covenant 状态机插件管理 (Contract Plugins)</div>
        <div class="section-desc">已装载的契约生命周期规则插件。活跃插件主导发现链上 Genesis，退役插件保持历史轮次终态兼容。</div>
      </div>
      <button class="btn btn-sm" id="btnReloadPlugins">🔄 刷新列表</button>
    </div>

    <div class="card" style="padding:0;overflow:hidden;margin-bottom:16px;">
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>契约标识</th>
              <th>模块源文件</th>
              <th>Profile 指纹 (SHA256)</th>
              <th>当前角色</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody id="tblPluginList">
            <tr><td colspan="5" style="text-align:center;color:var(--muted);">加载中...</td></tr>
          </tbody>
        </table>
      </div>
    </div>
  </section>

  <!-- TAB 4: SilverScript 编译工坊 -->
  <section id="tab-compiler" class="view-panel">
    <div class="section-head">
      <div>
        <div class="section-title">🛠️ SilverScript 在线编译器与契约工坊 (SilverScript Studio)</div>
        <div class="section-desc">
          在线编写、语法分析并编译 SilverScript 契约，自动抽取状态字段、方法调度标签（Dispatch Tag）与 32 字节模版哈希。
        </div>
      </div>
    </div>

    <div class="card" style="margin-bottom:16px;">
      <!-- 紧凑响应式工具栏 -->
      <div class="compiler-toolbar">
        <div class="compiler-toolbar-left">
          <label style="color:var(--muted);font-size:12px;font-weight:600;white-space:nowrap;">模版:</label>
          <select id="selectContractPreset" style="padding:5px 8px;font-size:12px;">
            <option value="counter">计数器合约 (Counter.sil · 单例状态机)</option>
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

      <!-- 响应式参数输入与执行按钮 -->
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
      <button class="btn btn-sm" id="btnRefreshJournal">🔄 刷新流水</button>
    </div>

    <!-- 事件流水卡片流容器 -->
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

<div class="toast" id="toastMsg"></div>

<script>
let currentToken = sessionStorage.getItem('kci_admin_token') || '';
const urlToken = new URLSearchParams(window.location.search).get('token');
if (urlToken) {
  currentToken = urlToken;
  sessionStorage.setItem('kci_admin_token', urlToken);
}

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

async function api(path, method, body) {
  method = method || 'GET';
  const headers = { 'Accept': 'application/json' };
  if (currentToken) headers['Authorization'] = 'Bearer ' + currentToken;
  if (body) headers['Content-Type'] = 'application/json';

  const res = await fetch(apiPrefix + path, {
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

    // Header Pill
    const pill = document.getElementById('headerStatusPill');
    const pillText = document.getElementById('pillText');
    const daaText = document.getElementById('headerDaaText');
    if (data.activeNode && data.activeNode.isSynced) {
      pill.className = 'status-pill';
      if (pillText) pillText.innerText = 'TN10 · 已同步';
    } else {
      pill.className = 'status-pill degraded';
      if (pillText) pillText.innerText = 'TN10 · 追块/降级';
    }
    if (daaText) {
      const curDaa = data.activeNode?.virtualDaaScore || data.checkpoint?.virtualDaa;
      daaText.innerText = curDaa ? ('DAA: ' + curDaa) : '选定链就绪';
    }

    // Top KPI
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

// Render Journal Feed into clean human-readable cards
function renderJournalFeed(logs) {
  const container = document.getElementById('journalFeed');
  if (!container) return;
  if (!logs || !logs.length) {
    container.innerHTML = '<div class="card" style="text-align:center;color:var(--muted);">暂无审计事件日志</div>';
    return;
  }

  container.innerHTML = logs.map(l => {
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
        '<div class="journal-field"><span class="journal-field-label">轮次 ID:</span><span class="journal-field-val mono">' + (d.round ? d.round.slice(0, 16) + '...' : '-') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">序号:</span><span class="journal-field-val mono">Seq #' + (d.seq ?? '-') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">交易 TXID:</span><span class="journal-field-val mono">' + (d.txid ? d.txid.slice(0, 16) + '...' : '-') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">接受块:</span><span class="journal-field-val mono">' + (d.accepting ? d.accepting.slice(0, 16) + '...' : '-') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">状态核验:</span><span class="journal-field-val mono" style="color:var(--kaspa);">' + (d.utxoCheck || 'PASS') + '</span></div>' +
      '</div>';
    } else if (l.kind === 'SPENT_EVENT') {
      tagClass = 'tag-standby';
      kindLabel = '⚡ 链尖 UTXO 花费事件';
      const op = d.outpoint ? (d.outpoint.transactionId?.slice(0, 16) + '...:' + d.outpoint.index) : '-';
      bodyHtml = '<div class="journal-grid">' +
        '<div class="journal-field"><span class="journal-field-label">轮次 ID:</span><span class="journal-field-val mono">' + (d.round ? d.round.slice(0, 16) + '...' : '-') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">已花费 Tip:</span><span class="journal-field-val mono">' + op + '</span></div>' +
      '</div><div style="font-size:11px;color:var(--muted);margin-top:4px;">节点推送此 UTXO 被花费，触发后继状态机解析</div>';
    } else if (l.kind === 'ROLLBACK' || l.kind === 'ROLLBACK_REPLAY') {
      tagClass = 'tag-warn';
      borderStyle = 'border-left: 3px solid var(--amber);';
      kindLabel = l.kind === 'ROLLBACK' ? '⚠️ 选定链重组回退 (Rollback)' : '🔄 重组快速重放恢复 (Replay)';
      bodyHtml = '<div class="journal-grid">' +
        '<div class="journal-field"><span class="journal-field-label">轮次 ID:</span><span class="journal-field-val mono">' + (d.round ? d.round.slice(0, 16) + '...' : '-') + '</span></div>' +
        (d.fromSeq !== undefined ? '<div class="journal-field"><span class="journal-field-label">回退起始:</span><span class="journal-field-val mono">Seq #' + d.fromSeq + '</span></div>' : '') +
        (d.txid ? '<div class="journal-field"><span class="journal-field-label">重放 TXID:</span><span class="journal-field-val mono">' + d.txid.slice(0, 16) + '...</span></div>' : '') +
      '</div>';
    } else if (l.kind === 'DISCOVERY_MARKER') {
      tagClass = 'tag-standby';
      kindLabel = '🔍 发现新契约创世 (Discovery)';
      bodyHtml = '<div class="journal-grid">' +
        '<div class="journal-field"><span class="journal-field-label">合约规则:</span><span class="journal-field-val mono">' + (d.contract || '-') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">创世 TXID:</span><span class="journal-field-val mono">' + (d.txid ? d.txid.slice(0, 16) + '...' : '-') + '</span></div>' +
        '<div class="journal-field"><span class="journal-field-label">发生 DAA:</span><span class="journal-field-val mono">' + (d.daa || '-') + '</span></div>' +
      '</div>';
    } else if (l.kind === 'ERROR') {
      tagClass = 'tag-rose';
      borderStyle = 'border-left: 3px solid var(--rose);';
      kindLabel = '❌ 节点通信/执行异常';
      bodyHtml = '<div style="color:var(--rose);font-size:12px;">' +
        '组件: <b>[' + (d.key || 'system') + ']</b> · 错误: <b>' + (d.message || JSON.stringify(d)) + '</b>' +
        '<div style="color:var(--muted);font-size:11px;margin-top:2px;">(节点瞬时抖动时索引器会自动切入热备节点重试)</div>' +
      '</div>';
    } else {
      tagClass = 'tag-retired';
      kindLabel = l.kind;
      bodyHtml = '<div class="mono" style="font-size:11.5px;color:var(--muted);">' + JSON.stringify(d) + '</div>';
    }

    return '<div class="journal-card" style="' + borderStyle + '">' +
      '<div class="journal-card-top">' +
        '<span class="tag ' + tagClass + '">' + kindLabel + '</span>' +
        '<span class="journal-time mono">' + timeStr + '</span>' +
      '</div>' +
      '<div class="journal-body">' + bodyHtml + '</div>' +
    '</div>';
  }).join('');
}

document.getElementById('btnRefreshJournal').onclick = () => {
  refreshAll();
  showToast('已拉取最新审计流水');
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

      let statusHtml = '<span style="color:var(--rose);">○ 离线 (' + (c.error || 'ERROR') + ')</span>';
      if (c.up) {
        statusHtml = '<span style="color:var(--emerald);">● 已连接</span> · ' + (c.isSynced ? 'Synced' : 'Catching-up') + (c.hasUtxoIndex ? ' · UTXO✓' : '');
      }

      let backendHtml = '<span class="mono" style="font-size:11px;color:var(--muted);">' + (c.p2pId ? c.p2pId.slice(0, 8) + '...' : '-') + '</span>';
      if (c.sharedBackend) {
        backendHtml += ' <span class="tag tag-warn" title="与其他候选节点共用相同后端，不可配置为双备！">⚠️ 共享后端</span>';
      }

      return '<tr>' +
        '<td class="mono"><b>' + c.url + '</b></td>' +
        '<td>' + rttBadge + '</td>' +
        '<td>' + lagBadge + '</td>' +
        '<td>' + statusHtml + '</td>' +
        '<td>' + backendHtml + '</td>' +
        '<td>' +
          '<button class="btn btn-sm" onclick="probeSingle(\\'' + c.url + '\\')">诊断</button>' +
        '</td>' +
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

document.getElementById('btnBenchmarkPool').onclick = loadNodePoolBenchmark;

window.probeSingle = function(url) {
  document.getElementById('inputProbeUrl').value = url;
  document.getElementById('btnProbeNode').click();
};

// 3. Custom Node Probe
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

// 4. Contract Plugins
async function loadPluginsTab() {
  try {
    const res = await api('/plugins');
    const tbody = document.getElementById('tblPluginList');
    const list = res.plugins || [];
    tbody.innerHTML = list.map((p, idx) => '<tr>' +
      '<td class="mono"><b>' + p.key + '</b></td>' +
      '<td class="mono">' + (p.module || '-') + '</td>' +
      '<td class="mono" style="font-size:12px;color:var(--muted);">' + (p.profileId || '-') + '</td>' +
      '<td>' + (p.retired ? '<span class="tag tag-retired">RETIRED (兼容)</span>' : '<span class="tag tag-active">ACTIVE (主导)</span>') + '</td>' +
      '<td>' +
        '<button class="btn btn-sm" onclick="togglePlugin(' + idx + ', ' + (!p.retired) + ')">' +
          (p.retired ? '🟢 设为主导发现' : '⏸️ 设为退役兼容') +
        '</button>' +
      '</td>' +
    '</tr>').join('');
  } catch (err) {
    console.error(err);
  }
}

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

// 5. SilverScript Studio Workbench
const PRESET_CONTRACTS = {
  counter: "contract Decls(int init_value) {\\n    int value = init_value;\\n\\n    #[covenant.singleton(mode = transition)]\\n    function bump(State prev_state, int delta) : (State) {\\n        return(State { value: prev_state.value + delta });\\n    }\\n}",
  empty: "contract CustomContract() {\\n    int count = 0;\\n\\n    #[covenant.singleton(mode = transition)]\\n    function step(State prev_state) : (State) {\\n        return(State { count: prev_state.count + 1 });\\n    }\\n}"
};

const editor = document.getElementById('silEditor');
if (editor) editor.value = PRESET_CONTRACTS.counter;

const presetSelect = document.getElementById('selectContractPreset');
if (presetSelect) {
  presetSelect.onchange = e => {
    const v = e.target.value;
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
    '<details class="card" style="cursor:pointer;"><summary style="font-weight:600;color:var(--muted);">📦 查看完整编译器 JSON Artifact</summary>' +
      '<pre class="mono" style="margin-top:12px;font-size:11px;overflow:auto;max-height:300px;">' + JSON.stringify(data.artifact, null, 2) + '</pre>' +
    '</details>';
}

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
      if (!window._poolLoaded) { window._poolLoaded = true; loadNodePoolBenchmark(); }
    }
  };
});

// Auth Form Submit
document.getElementById('authForm').onsubmit = async e => {
  e.preventDefault();
  const token = document.getElementById('inputAdminToken').value.trim();
  if (token) {
    currentToken = token;
    sessionStorage.setItem('kci_admin_token', token);
    try {
      await refreshAll();
      showToast('登录成功！', 'var(--emerald)');
      document.getElementById('authModal').style.display = 'none';
    } catch (err) {
      showToast('Token 无效，请重新输入', 'var(--rose)');
    }
  }
};

document.getElementById('btnCancelAuth').onclick = () => {
  document.getElementById('authModal').style.display = 'none';
};

document.getElementById('btnLogout').onclick = () => {
  document.getElementById('authModal').style.display = 'flex';
};

document.getElementById('btnRefresh').onclick = () => {
  refreshAll();
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
