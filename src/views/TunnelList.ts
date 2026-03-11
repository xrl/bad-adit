import { invoke } from '@tauri-apps/api/core';
import { navigate } from '../main';

interface TunnelConfig {
  id: string;
  name: string;
  ssh_host: string;
  ssh_user: string;
  ssh_key_path: string;
  target_host: string;
  target_port: number;
  local_port: number;
  auto_reconnect: boolean;
}

export async function renderTunnelList(container: HTMLElement) {
  container.innerHTML = `
    <div class="header">
      <h1>Tunnels</h1>
      <div style="display:flex;gap:8px;align-items:center">
        <span id="error-badge" class="error-badge" style="display:none"></span>
        <button class="primary" id="add-btn">Add</button>
      </div>
    </div>
    <div class="tunnel-list" id="tunnel-list">
      <div class="empty-state">Loading...</div>
    </div>
  `;

  document.getElementById('add-btn')!.addEventListener('click', () => {
    navigate({ view: 'add' });
  });

  // Show error badge if there are errors
  try {
    const errorCount: number = await invoke('get_error_count');
    if (errorCount > 0) {
      const badge = document.getElementById('error-badge')!;
      badge.textContent = `${errorCount} error${errorCount === 1 ? '' : 's'}`;
      badge.style.display = 'inline-flex';
      badge.addEventListener('click', () => navigate({ view: 'console' }));
    }
  } catch { /* ignore */ }

  try {
    const tunnels: TunnelConfig[] = await invoke('get_tunnels');
    const list = document.getElementById('tunnel-list')!;

    if (tunnels.length === 0) {
      list.innerHTML = `
        <div class="empty-state">
          No tunnels configured yet.<br>Click "Add" to create your first tunnel.
        </div>
      `;
      return;
    }

    list.innerHTML = '';
    for (const tunnel of tunnels) {
      const item = document.createElement('div');
      item.className = 'tunnel-item';
      item.innerHTML = `
        <div class="tunnel-info">
          <div class="tunnel-name">${escapeHtml(tunnel.name)}</div>
          <div class="tunnel-mapping">localhost:${tunnel.local_port} → ${escapeHtml(tunnel.target_host)}:${tunnel.target_port}</div>
        </div>
        <div class="tunnel-actions">
          <button class="edit-btn">Edit</button>
          <button class="danger remove-btn">Remove</button>
        </div>
      `;

      item.querySelector('.tunnel-info')!.addEventListener('click', () => {
        navigate({ view: 'stats', id: tunnel.id });
      });

      item.querySelector('.edit-btn')!.addEventListener('click', (e) => {
        e.stopPropagation();
        navigate({ view: 'edit', id: tunnel.id });
      });

      item.querySelector('.remove-btn')!.addEventListener('click', async (e) => {
        e.stopPropagation();
        if (confirm(`Remove tunnel "${tunnel.name}"?`)) {
          try {
            await invoke('remove_tunnel', { id: tunnel.id });
            renderTunnelList(container);
          } catch (err) {
            alert(`Failed to remove: ${err}`);
          }
        }
      });

      list.appendChild(item);
    }
  } catch (err) {
    document.getElementById('tunnel-list')!.innerHTML = `
      <div class="empty-state error-text">Failed to load tunnels: ${err}</div>
    `;
  }
}

function escapeHtml(text: string): string {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}
