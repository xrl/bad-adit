import { invoke } from '@tauri-apps/api/core';
import { navigate } from '../main';

interface StatsSnapshot {
  bytes_uploaded: number;
  bytes_downloaded: number;
  connections_open: number;
  connections_total: number;
  uptime_seconds: number;
  bytes_uploaded_formatted: string;
  bytes_downloaded_formatted: string;
  uptime_formatted: string;
  last_reconnect_ago: string | null;
}

interface TunnelStatus {
  id: string;
  name: string;
  state: string | { Error: string };
  stats: StatsSnapshot | null;
  local_port: number;
  target_host: string;
  target_port: number;
}

export function renderTunnelStats(container: HTMLElement, tunnelId: string): () => void {
  container.innerHTML = `
    <button class="back-link" id="back-btn">← Back to Tunnels</button>
    <div id="stats-content">
      <div class="empty-state">Loading...</div>
    </div>
  `;

  document.getElementById('back-btn')!.addEventListener('click', () => {
    navigate({ view: 'list' });
  });

  let intervalId: number | null = null;

  async function refresh() {
    try {
      const statuses: TunnelStatus[] = await invoke('get_all_tunnel_status');
      const status = statuses.find(s => s.id === tunnelId);
      if (!status) {
        document.getElementById('stats-content')!.innerHTML = `
          <div class="empty-state">Tunnel not found.</div>
        `;
        return;
      }

      const stateStr = typeof status.state === 'string' ? status.state : `Error: ${(status.state as { Error: string }).Error}`;
      const badgeClass = stateStr === 'Running' ? 'running' : stateStr === 'Stopped' ? 'stopped' : 'reconnecting';
      const stats = status.stats;

      document.getElementById('stats-content')!.innerHTML = `
        <div class="stats-header">
          <div>
            <h1>${escapeHtml(status.name)}</h1>
            <div class="tunnel-mapping">localhost:${status.local_port} → ${escapeHtml(status.target_host)}:${status.target_port}</div>
          </div>
          <div>
            <span class="status-badge ${badgeClass}">${stateStr}</span>
            <button id="toggle-btn" class="${stateStr === 'Running' ? 'danger' : 'primary'}" style="margin-left: 8px">
              ${stateStr === 'Running' ? 'Stop' : 'Start'}
            </button>
          </div>
        </div>
        ${stats ? `
        <div class="stats-grid">
          <div class="stats-section">
            <h3>Traffic</h3>
            <div class="stat-row">
              <span class="stat-label">Uploaded</span>
              <span class="stat-value">${stats.bytes_uploaded_formatted}</span>
            </div>
            <div class="stat-row">
              <span class="stat-label">Downloaded</span>
              <span class="stat-value">${stats.bytes_downloaded_formatted}</span>
            </div>
          </div>
          <div class="stats-section">
            <h3>Connections</h3>
            <div class="stat-row">
              <span class="stat-label">Currently open</span>
              <span class="stat-value">${stats.connections_open}</span>
            </div>
            <div class="stat-row">
              <span class="stat-label">Total handled</span>
              <span class="stat-value">${stats.connections_total}</span>
            </div>
          </div>
          <div class="stats-section">
            <h3>Session</h3>
            <div class="stat-row">
              <span class="stat-label">Uptime</span>
              <span class="stat-value">${stats.uptime_formatted}</span>
            </div>
            <div class="stat-row">
              <span class="stat-label">Last reconnect</span>
              <span class="stat-value">${stats.last_reconnect_ago || '—'}</span>
            </div>
          </div>
        </div>
        ` : ''}
      `;

      document.getElementById('toggle-btn')!.addEventListener('click', async () => {
        try {
          if (stateStr === 'Running') {
            await invoke('stop_tunnel', { id: tunnelId });
          } else {
            await invoke('start_tunnel', { id: tunnelId });
          }
          refresh();
        } catch (err) {
          alert(`${err}`);
        }
      });
    } catch (err) {
      document.getElementById('stats-content')!.innerHTML = `
        <div class="empty-state error-text">Failed to load stats: ${err}</div>
      `;
    }
  }

  refresh();
  intervalId = window.setInterval(refresh, 1000);

  return () => {
    if (intervalId !== null) {
      clearInterval(intervalId);
    }
  };
}

function escapeHtml(text: string): string {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}
