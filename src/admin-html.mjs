// Embedded single-file Web Dashboard for Kaspa Covenant Indexer (KCI)
// Zero external dependencies: vanilla JS, inline SVG, dark modern theme.

export const ADMIN_HTML = `<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Kaspa Covenant Indexer · 控制台</title>
<style>
:root {
  --bg: #090d16;
  --panel: #101726;
  --panel-hover: #162034;
  --border: #1f2b42;
  --border-light: #2c3c5c;
  --kaspa: #70c7ba;
  --kaspa-dim: rgba(112, 199, 186, 0.12);
  --text: #e2e8f0;
  --muted: #94a3b8;
  --emerald: #10b981;
  --amber: #f59e0b;
  --rose: #f43f5e;
  --indigo: #6366f1;
  --font-mono: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}
* { box-sizing: border-box; margin: 0; padding: 0; }
body {
  background: var(--bg);
  color: var(--text);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  line-height: 1.5;
  font-size: 14px;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}
.mono { font-family: var(--font-mono); }
header {
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  padding: 12px 24px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  position: sticky;
  top: 0;
  z-index: 50;
}
.brand {
  display: flex;
  align-items: center;
  gap: 12px;
  text-decoration: none;
  color: var(--text);
}
.logo-badge {
  background: var(--kaspa-dim);
  border: 1px solid var(--kaspa);
  color: var(--kaspa);
  font-size: 11px;
  font-weight: 700;
  padding: 2px 8px;
  border-radius: 6px;
  letter-spacing: 0.5px;
}
.brand-title {
  font-size: 16px;
  font-weight: 700;
  display: flex;
  align-items: center;
  gap: 8px;
}
.header-right {
  display: flex;
  align-items: center;
  gap: 14px;
}
.status-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  background: rgba(16, 185, 129, 0.12);
  color: var(--emerald);
  border: 1px solid rgba(16, 185, 129, 0.3);
  padding: 4px 10px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 600;
}
.status-pill.degraded {
  background: rgba(245, 158, 11, 0.12);
  color: var(--amber);
  border-color: rgba(245, 158, 11, 0.3);
}
.status-pill.down {
  background: rgba(244, 63, 94, 0.12);
  color: var(--rose);
  border-color: rgba(244, 63, 94, 0.3);
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: currentColor;
}
nav.tabs {
  background: rgba(16, 23, 38, 0.6);
  border-bottom: 1px solid var(--border);
  padding: 0 24px;
  display: flex;
  gap: 4px;
}
.tab-btn {
  background: transparent;
  border: none;
  border-bottom: 2px solid transparent;
  color: var(--muted);
  padding: 12px 16px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.15s;
}
.tab-btn:hover { color: var(--text); }
.tab-btn.active {
  color: var(--kaspa);
  border-bottom-color: var(--kaspa);
}
main {
  flex: 1;
  padding: 24px;
  max-width: 1320px;
  width: 100%;
  margin: 0 auto;
}
.view-panel { display: none; }
.view-panel.active { display: block; }

.grid-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: 16px;
  margin-bottom: 24px;
}
.card {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 18px 20px;
}
.card-label {
  font-size: 12px;
  color: var(--muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin-bottom: 8px;
  display: flex;
  justify-content: space-between;
}
.card-val {
  font-size: 22px;
  font-weight: 700;
  color: var(--text);
}
.card-sub {
  font-size: 12px;
  color: var(--muted);
  margin-top: 6px;
}

.section-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin: 24px 0 14px;
}
.section-title {
  font-size: 16px;
  font-weight: 700;
  display: flex;
  align-items: center;
  gap: 8px;
}

table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}
th {
  background: rgba(16, 23, 38, 0.9);
  color: var(--muted);
  font-weight: 600;
  text-align: left;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
}
td {
  padding: 12px 14px;
  border-bottom: 1px solid var(--border);
}
tr:hover td { background: var(--panel-hover); }

.tag {
  display: inline-block;
  padding: 2px 7px;
  border-radius: 5px;
  font-size: 11px;
  font-weight: 600;
}
.tag-active { background: rgba(16, 185, 129, 0.15); color: var(--emerald); border: 1px solid rgba(16, 185, 129, 0.3); }
.tag-retired { background: rgba(148, 163, 184, 0.15); color: var(--muted); border: 1px solid rgba(148, 163, 184, 0.3); }
.tag-standby { background: rgba(99, 102, 241, 0.15); color: var(--indigo); border: 1px solid rgba(99, 102, 241, 0.3); }

.btn {
  background: var(--panel-hover);
  color: var(--text);
  border: 1px solid var(--border);
  padding: 7px 14px;
  border-radius: 7px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.15s;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.btn:hover { border-color: var(--kaspa); color: var(--kaspa); }
.btn-pri {
  background: var(--kaspa);
  color: #04100c;
  border-color: var(--kaspa);
}
.btn-pri:hover { background: #5db6a8; color: #04100c; }
.btn-sm { padding: 4px 9px; font-size: 12px; }

input, select, textarea {
  background: #0d131f;
  border: 1px solid var(--border);
  color: var(--text);
  padding: 9px 12px;
  border-radius: 7px;
  font-size: 13px;
  width: 100%;
}
input:focus, select:focus, textarea:focus {
  outline: none;
  border-color: var(--kaspa);
}

.modal-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(4, 7, 13, 0.8);
  backdrop-filter: blur(4px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
  padding: 20px;
}
.modal-box {
  background: var(--panel);
  border: 1px solid var(--border-light);
  border-radius: 14px;
  max-width: 480px;
  width: 100%;
  padding: 24px;
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.5);
}
.modal-head {
  font-size: 17px;
  font-weight: 700;
  margin-bottom: 12px;
  display: flex;
  align-items: center;
  gap: 8px;
}
.modal-desc {
  font-size: 13px;
  color: var(--muted);
  margin-bottom: 20px;
}

.toast {
  position: fixed;
  bottom: 24px;
  right: 24px;
  background: var(--panel);
  border: 1px solid var(--kaspa);
  color: var(--text);
  padding: 12px 20px;
  border-radius: 9px;
  box-shadow: 0 10px 25px rgba(0,0,0,0.4);
  font-size: 13px;
  font-weight: 600;
  z-index: 200;
  display: none;
}
</style>
</head>
<body>

<header>
  <div class="brand">
    <span class="logo-badge">KCI</span>
    <div class="brand-title">Kaspa Covenant Indexer <span style="font-weight:400;color:var(--muted);font-size:13px;">控制台</span></div>
  </div>
  <div class="header-right">
    <div id="headerStatusPill" class="status-pill"><span class="dot"></span>连接中...</div>
    <button class="btn btn-sm" id="btnRefresh" title="刷新状态">🔄 刷新</button>
    <button class="btn btn-sm" id="btnLogout" title="退出鉴权">退出登录</button>
  </div>
</header>

<nav class="tabs">
  <button class="tab-btn active" data-tab="tab-overview">📊 运行概览</button>
  <button class="tab-btn" data-tab="tab-nodes">⚡ 节点控制与测速</button>
  <button class="tab-btn" data-tab="tab-plugins">🧩 合约插件管理</button>
  <button class="tab-btn" data-tab="tab-journal">📜 事件日志</button>
</nav>

<main>
  <!-- TAB 1: 运行概览 -->
  <section id="tab-overview" class="view-panel active">
    <div class="grid-cards">
      <div class="card">
        <div class="card-label"><span>活跃共识节点</span><span>P2P ID</span></div>
        <div class="card-val mono" id="statNodeUrl" style="font-size:15px;word-break:break-all;">-</div>
        <div class="card-sub" id="statNodeDetails">延迟: - | 状态: -</div>
      </div>
      <div class="card">
        <div class="card-label"><span>收录轮次统计</span><span>ROUNDS</span></div>
        <div class="card-val" id="statRoundsTotal">-</div>
        <div class="card-sub" id="statRoundsBreakdown">进行中: - · 终局: -</div>
      </div>
      <div class="card">
        <div class="card-label"><span>选定链 DAA & 检查点</span><span>CONSENSUS</span></div>
        <div class="card-val mono" id="statVirtualDaa" style="font-size:20px;">-</div>
        <div class="card-sub" id="statCheckpointTime">最近检查点: -</div>
      </div>
      <div class="card">
        <div class="card-label"><span>系统内存与负载</span><span>PROCESS</span></div>
        <div class="card-val mono" id="statMemRss" style="font-size:20px;">-</div>
        <div class="card-sub" id="statUptime">已运行: -</div>
      </div>
    </div>

    <div class="section-head">
      <div class="section-title">📦 合约插件状态分布</div>
    </div>
    <div class="card" style="padding:0;overflow:hidden;">
      <table>
        <thead>
          <tr>
            <th>合约插件</th>
            <th>Profile 指纹</th>
            <th>角色状态</th>
            <th>已索引轮次</th>
          </tr>
        </thead>
        <tbody id="tblContractSummary">
          <tr><td colspan="4" style="text-align:center;color:var(--muted);">加载中...</td></tr>
        </tbody>
      </table>
    </div>
  </section>

  <!-- TAB 2: 节点控制与测速 -->
  <section id="tab-nodes" class="view-panel">
    <div class="card" style="margin-bottom:20px;">
      <div class="section-title" style="margin-bottom:12px;">⚡ 在线节点测速与健康体检</div>
      <p style="color:var(--muted);font-size:13px;margin-bottom:14px;">
        输入任意 Kaspa TN10 wRPC 节点地址（支持 Borsh 与 JSON 端口），实时测试延迟、同步状态及 UTXO 索引完整性：
      </p>
      <div style="display:flex;gap:10px;">
        <input type="text" id="inputProbeUrl" class="mono" placeholder="wss://... 或 ws://127.0.0.1:17210" value="wss://vector-10.kaspa.green/kaspa/testnet-10/wrpc/borsh">
        <button class="btn btn-pri" id="btnProbeNode" style="white-space:nowrap;">🚀 测试连通性</button>
      </div>
      <div id="probeResultBox" style="margin-top:14px;display:none;"></div>
    </div>

    <div class="section-head">
      <div class="section-title">🛰️ 当前活跃节点池成员</div>
    </div>
    <div class="card" style="padding:0;overflow:hidden;">
      <table>
        <thead>
          <tr>
            <th>节点地址</th>
            <th>当前角色</th>
            <th>在线状态</th>
            <th>失败重试</th>
          </tr>
        </thead>
        <tbody id="tblNodePool">
          <tr><td colspan="4" style="text-align:center;color:var(--muted);">加载中...</td></tr>
        </tbody>
      </table>
    </div>
  </section>

  <!-- TAB 3: 合约插件管理 -->
  <section id="tab-plugins" class="view-panel">
    <div class="section-head">
      <div class="section-title">🧩 已装载的 Covenant 插件列表</div>
      <button class="btn btn-sm" id="btnReloadPlugins">🔄 热重载插件配置</button>
    </div>
    <div class="card" style="padding:0;overflow:hidden;margin-bottom:24px;">
      <table>
        <thead>
          <tr>
            <th>插件标识</th>
            <th>模块源文件</th>
            <th>Profile 指纹 (SHA256)</th>
            <th>运行模式</th>
            <th>操作</th>
          </tr>
        </thead>
        <tbody id="tblPluginList">
          <tr><td colspan="5" style="text-align:center;color:var(--muted);">加载中...</td></tr>
        </tbody>
      </table>
    </div>

    <!-- Phase 2 预告展板 -->
    <div class="card" style="border:1px dashed var(--border-light);background:rgba(16,23,38,0.4);">
      <div class="section-title" style="color:var(--kaspa);">🛠️ SilverScript (.sil) 在线编译与插件生成 (Phase 2 规划)</div>
      <p style="color:var(--muted);font-size:13px;margin:10px 0;">
        即将支持：直接拖拽上传合约 <code>.sil</code> 源码，服务端自动调用 <code>silverc</code> 离线编译，生成对应 ABI、State 结构及模板哈希，并一键组装为可运行的 Indexer 插件。
      </p>
    </div>
  </section>

  <!-- TAB 4: 事件日志 -->
  <section id="tab-journal" class="view-panel">
    <div class="section-head">
      <div class="section-title">📜 实时系统事件与共识审计流水</div>
    </div>
    <div class="card" style="padding:0;overflow:hidden;">
      <table>
        <thead>
          <tr>
            <th>发生时间</th>
            <th>事件类别</th>
            <th>详细上下文</th>
          </tr>
        </thead>
        <tbody id="tblJournal">
          <tr><td colspan="3" style="text-align:center;color:var(--muted);">加载中...</td></tr>
        </tbody>
      </table>
    </div>
  </section>
</main>

<!-- 鉴权登录弹窗 -->
<div class="modal-backdrop" id="authModal" style="display:none;">
  <div class="modal-box">
    <div class="modal-head">🔐 管理员身份验证</div>
    <div class="modal-desc">
      当前控制台接口受管理员令牌保护。请输入服务配置的 <code>ADMIN_TOKEN</code>：
    </div>
    <form id="authForm">
      <div style="margin-bottom:14px;">
        <input type="password" id="inputAdminToken" placeholder="输入 32 位 Token 或自定义密码..." required autofocus>
      </div>
      <button type="submit" class="btn btn-pri" style="width:100%;justify-content:center;">确认登录</button>
    </form>
  </div>
</div>

<div class="toast" id="toastMsg"></div>

<script>
let currentToken = sessionStorage.getItem('kci_admin_token') || '';
// Check URL param ?token=...
const urlToken = new URLSearchParams(window.location.search).get('token');
if (urlToken) {
  currentToken = urlToken;
  sessionStorage.setItem('kci_admin_token', urlToken);
}

function showToast(text, color = '') {
  const el = document.getElementById('toastMsg');
  el.innerText = text;
  el.style.borderColor = color || 'var(--kaspa)';
  el.style.display = 'block';
  setTimeout(() => el.style.display = 'none', 3000);
}

const apiPrefix = (window.location.pathname.replace(/\/admin(?:\/.*)?$/, '') || '') + '/admin/api';

async function api(path, method = 'GET', body = null) {
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

async function refreshAll() {
  try {
    const data = await api('/status');
    document.getElementById('authModal').style.display = 'none';

    // 1. Header Pill
    const pill = document.getElementById('headerStatusPill');
    if (data.activeNode?.isSynced) {
      pill.className = 'status-pill';
      pill.innerHTML = '<span class="dot"></span>' + data.network.toUpperCase() + ' · 已同步';
    } else {
      pill.className = 'status-pill degraded';
      pill.innerHTML = '<span class="dot"></span>' + data.network.toUpperCase() + ' · 追块中/降级';
    }

    // 2. Overview Stats
    document.getElementById('statNodeUrl').innerText = data.activeNode?.url || '无连接';
    document.getElementById('statNodeDetails').innerText = 
      \`版本: \${data.activeNode?.serverVersion || '-'} | 同步: \${data.activeNode?.isSynced ? '是' : '否'} | UTXO索引: \${data.activeNode?.hasUtxoIndex ? '已开启' : '未开启'}\`;

    document.getElementById('statRoundsTotal').innerText = data.rounds?.total ?? 0;
    document.getElementById('statRoundsBreakdown').innerText = 
      \`活跃进行中: \${data.rounds?.live ?? 0} · 终局完成: \${data.rounds?.byStatus?.close ?? 0}\`;

    document.getElementById('statVirtualDaa').innerText = data.activeNode?.virtualDaaScore || data.checkpoint?.virtualDaa || '-';
    document.getElementById('statCheckpointTime').innerText = 
      data.checkpoint?.at ? \`检查点: \${new Date(data.checkpoint.at).toLocaleTimeString()}\` : '无近期检查点';

    document.getElementById('statMemRss').innerText = (data.memory?.rssMb || '-') + ' MB';
    const hrs = Math.floor(data.uptimeSec / 3600), mins = Math.floor((data.uptimeSec % 3600) / 60);
    document.getElementById('statUptime').innerText = \`已运行: \${hrs}小时 \${mins}分钟\`;

    // 3. Contract Breakdown Table
    const cTbody = document.getElementById('tblContractSummary');
    const contractsList = data.contracts || [];
    if (!contractsList.length) {
      cTbody.innerHTML = '<tr><td colspan="4" style="text-align:center;color:var(--muted);">未配置契约插件</td></tr>';
    } else {
      cTbody.innerHTML = contractsList.map(c => {
        const count = data.rounds?.byContract?.[c.key] || 0;
        return \`<tr>
          <td class="mono"><b>\${c.key}</b></td>
          <td class="mono" style="font-size:12px;color:var(--muted);">\${c.profileId || '-'}</td>
          <td>\${c.retired ? '<span class="tag tag-retired">RETIRED (退役兼容)</span>' : '<span class="tag tag-active">ACTIVE (主导发现)</span>'}</td>
          <td><b style="color:var(--kaspa);">\${count}</b> 轮次</td>
        </tr>\`;
      }).join('');
    }

    // 4. Node Pool Table
    const nTbody = document.getElementById('tblNodePool');
    const pool = data.nodePool || [];
    if (!pool.length) {
      nTbody.innerHTML = '<tr><td colspan="4" style="text-align:center;color:var(--muted);">无可用节点信息</td></tr>';
    } else {
      nTbody.innerHTML = pool.map(m => \`<tr>
        <td class="mono">\${m.url}</td>
        <td>\${m.active ? '<span class="tag tag-active">ACTIVE (主连接)</span>' : '<span class="tag tag-standby">STANDBY (热备)</span>'}</td>
        <td>\${m.up ? '<span style="color:var(--emerald);">● 在线正常</span>' : '<span style="color:var(--rose);">○ 掉线重连中</span>'}</td>
        <td class="mono">\${m.attempts || 0} 次</td>
      </tr>\`).join('');
    }

    // 5. Journal Table
    const jTbody = document.getElementById('tblJournal');
    const logs = data.journal || [];
    if (!logs.length) {
      jTbody.innerHTML = '<tr><td colspan="3" style="text-align:center;color:var(--muted);">暂无事件日志</td></tr>';
    } else {
      jTbody.innerHTML = logs.map(l => \`<tr>
        <td style="white-space:nowrap;color:var(--muted);">\${new Date(l.at).toLocaleTimeString()}</td>
        <td><span class="mono" style="font-weight:600;color:var(--kaspa);">\${l.kind}</span></td>
        <td class="mono" style="font-size:12px;word-break:break-all;">\${typeof l.detail === 'object' ? JSON.stringify(l.detail) : l.detail}</td>
      </tr>\`).join('');
    }

  } catch (err) {
    if (err.message !== 'UNAUTHORIZED') {
      showToast('拉取状态失败: ' + err.message, 'var(--rose)');
    }
  }
}

async function loadPluginsTab() {
  try {
    const res = await api('/plugins');
    const tbody = document.getElementById('tblPluginList');
    const list = res.plugins || [];
    tbody.innerHTML = list.map((p, idx) => \`<tr>
      <td class="mono"><b>\${p.key}</b></td>
      <td class="mono">\${p.module || '-'}</td>
      <td class="mono" style="font-size:12px;color:var(--muted);">\${p.profileId || '-'}</td>
      <td>\${p.retired ? '<span class="tag tag-retired">RETIRED</span>' : '<span class="tag tag-active">ACTIVE</span>'}</td>
      <td>
        <button class="btn btn-sm" onclick="togglePlugin(\${idx}, \${!p.retired})">
          \${p.retired ? '🟢 设为主导发现' : '⏸️ 设为退役兼容'}
        </button>
      </td>
    </tr>\`).join('');
  } catch (err) {
    console.error(err);
  }
}

window.togglePlugin = async function(index, newRetired) {
  try {
    await api('/plugins/toggle', 'POST', { index, retired: newRetired });
    showToast('插件状态已切换并重新加载！', 'var(--emerald)');
    loadPluginsTab();
    refreshAll();
  } catch (err) {
    showToast('切换失败: ' + err.message, 'var(--rose)');
  }
};

// Node Probe Button
document.getElementById('btnProbeNode').onclick = async () => {
  const url = document.getElementById('inputProbeUrl').value.trim();
  const box = document.getElementById('probeResultBox');
  box.style.display = 'block';
  box.innerHTML = '<div style="color:var(--muted);">正在握手连接并探测 DAA 与高度...</div>';

  try {
    const r = await api('/test-node', 'POST', { url });
    if (!r.ok) throw new Error(r.error);
    box.innerHTML = \`<div class="card" style="background:var(--panel-hover);border-color:var(--kaspa);">
      <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;">
        <strong style="color:var(--emerald);">✓ 节点连接成功</strong>
        <span class="mono" style="color:var(--kaspa);font-weight:700;">\${r.rttMs} ms 往返延迟</span>
      </div>
      <div style="font-size:13px;line-height:1.7;">
        网络: <b class="mono">\${r.info?.networkId}</b> · 服务端版本: <b class="mono">\${r.info?.serverVersion}</b><br>
        同步状态: <b>\${r.info?.isSynced ? '🟢 已同步 (Synced)' : '🟡 追块中 (Not Synced)'}</b> · UTXO 索引: <b>\${r.info?.hasUtxoIndex ? '已开启' : '未开启'}</b><br>
        链尖 DAA: <b class="mono">\${r.info?.virtualDaaScore || '-'}</b> · 当前 Sink 块: <span class="mono" style="font-size:11px;color:var(--muted);">\${r.dag?.sink}</span>
      </div>
    </div>\`;
  } catch (err) {
    box.innerHTML = \`<div class="card" style="border-color:var(--rose);background:rgba(244,63,94,0.1);">
      <strong style="color:var(--rose);">❌ 节点探测失败</strong>
      <div style="font-size:12px;margin-top:6px;color:var(--muted);word-break:break-all;">\${err.message}</div>
    </div>\`;
  }
};

// Tabs Switcher
document.querySelectorAll('.tab-btn').forEach(btn => {
  btn.onclick = () => {
    document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
    document.querySelectorAll('.view-panel').forEach(p => p.classList.remove('active'));
    btn.classList.add('active');
    document.getElementById(btn.dataset.tab).classList.add('active');
    if (btn.dataset.tab === 'tab-plugins') loadPluginsTab();
  };
});

// Auth Form Submit
document.getElementById('authForm').onsubmit = e => {
  e.preventDefault();
  const token = document.getElementById('inputAdminToken').value.trim();
  if (token) {
    currentToken = token;
    sessionStorage.setItem('kci_admin_token', token);
    refreshAll();
  }
};

document.getElementById('btnLogout').onclick = () => {
  sessionStorage.removeItem('kci_admin_token');
  currentToken = '';
  document.getElementById('authModal').style.display = 'flex';
};

document.getElementById('btnRefresh').onclick = () => {
  refreshAll();
  showToast('已刷新最新数据');
};

document.getElementById('btnReloadPlugins').onclick = () => {
  loadPluginsTab();
  showToast('插件列表已刷新');
};

// Auto refresh every 5 seconds
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
