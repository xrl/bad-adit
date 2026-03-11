import { invoke } from '@tauri-apps/api/core';
import { navigate } from '../main';

interface LogEntry {
  timestamp: number;
  level: string;
  message: string;
  tunnel_name: string | null;
}

export function renderErrorConsole(container: HTMLElement): () => void {
  container.innerHTML = `
    <button class="back-link" id="back-btn">← Back to Tunnels</button>
    <div class="console-header">
      <h1>Error Console</h1>
      <button id="clear-btn" class="danger">Clear</button>
    </div>
    <div id="console-content">
      <div class="empty-state">Loading...</div>
    </div>
  `;

  document.getElementById('back-btn')!.addEventListener('click', () => {
    navigate({ view: 'list' });
  });

  document.getElementById('clear-btn')!.addEventListener('click', async () => {
    await invoke('clear_error_log');
    refresh();
  });

  let intervalId: number | null = null;

  async function refresh() {
    try {
      const entries: LogEntry[] = await invoke('get_error_log');
      const content = document.getElementById('console-content')!;

      if (entries.length === 0) {
        content.innerHTML = `<div class="empty-state">No log entries.</div>`;
        return;
      }

      content.innerHTML = entries
        .slice()
        .reverse()
        .map(entry => {
          const date = new Date(entry.timestamp * 1000);
          const time = date.toLocaleTimeString();
          const levelClass = `log-${entry.level}`;
          const tunnel = entry.tunnel_name ? `<span class="log-tunnel">${escapeHtml(entry.tunnel_name)}</span>` : '';
          return `
            <div class="log-entry ${levelClass}">
              <span class="log-time">${time}</span>
              <span class="log-level">${entry.level.toUpperCase()}</span>
              ${tunnel}
              <span class="log-message">${escapeHtml(entry.message)}</span>
            </div>
          `;
        })
        .join('');
    } catch (err) {
      document.getElementById('console-content')!.innerHTML = `
        <div class="empty-state error-text">Failed to load logs: ${err}</div>
      `;
    }
  }

  refresh();
  intervalId = window.setInterval(refresh, 2000);

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
