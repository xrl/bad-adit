import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
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

export async function renderTunnelForm(container: HTMLElement, editId: string | null) {
  const isEdit = editId !== null;
  let existing: TunnelConfig | null = null;

  if (isEdit) {
    try {
      const tunnels: TunnelConfig[] = await invoke('get_tunnels');
      existing = tunnels.find(t => t.id === editId) || null;
    } catch {
      // ignore
    }
  }

  container.innerHTML = `
    <button class="back-link" id="back-btn">← Back to Tunnels</button>
    <h1>${isEdit ? 'Edit Tunnel' : 'Add Tunnel'}</h1>
    <form id="tunnel-form">
      <div class="form-group">
        <label for="name">Tunnel Name</label>
        <input type="text" id="name" value="${escapeAttr(existing?.name || '')}" placeholder="e.g. Production DB" required>
      </div>
      <div class="form-row">
        <div class="form-group">
          <label for="ssh_host">SSH Host</label>
          <input type="text" id="ssh_host" value="${escapeAttr(existing?.ssh_host || '')}" placeholder="e.g. bastion.example.com" required>
        </div>
        <div class="form-group">
          <label for="ssh_user">SSH User</label>
          <input type="text" id="ssh_user" value="${escapeAttr(existing?.ssh_user || '')}" placeholder="e.g. deploy" required>
        </div>
      </div>
      <div class="form-group">
        <label for="ssh_key_path">SSH Key File</label>
        <div class="file-input-row">
          <input type="text" id="ssh_key_path" value="${escapeAttr(existing?.ssh_key_path || '')}" placeholder="~/.ssh/id_ed25519">
          <button type="button" id="browse-btn">Browse</button>
        </div>
      </div>
      <div class="form-row">
        <div class="form-group">
          <label for="target_host">Target Host</label>
          <input type="text" id="target_host" value="${escapeAttr(existing?.target_host || 'localhost')}" placeholder="localhost">
        </div>
        <div class="form-group">
          <label for="target_port">Target Port</label>
          <input type="number" id="target_port" value="${existing?.target_port || ''}" placeholder="5432" required min="1" max="65535">
        </div>
      </div>
      <div class="form-group">
        <label for="local_port">Local Port</label>
        <input type="number" id="local_port" value="${existing?.local_port || ''}" placeholder="5432" required min="1" max="65535">
      </div>
      <div class="form-group">
        <div class="checkbox-group">
          <input type="checkbox" id="auto_reconnect" ${existing?.auto_reconnect ? 'checked' : ''}>
          <label for="auto_reconnect">Auto-reconnect on disconnect</label>
        </div>
      </div>
      <div id="form-error" class="error-text" style="display:none"></div>
      <div class="form-actions">
        <button type="button" id="cancel-btn">Cancel</button>
        <button type="submit" class="primary">Save</button>
      </div>
    </form>
  `;

  document.getElementById('back-btn')!.addEventListener('click', () => {
    navigate({ view: 'list' });
  });

  document.getElementById('cancel-btn')!.addEventListener('click', () => {
    navigate({ view: 'list' });
  });

  document.getElementById('browse-btn')!.addEventListener('click', async () => {
    const selected = await open({
      multiple: false,
      directory: false,
    });
    if (selected) {
      (document.getElementById('ssh_key_path') as HTMLInputElement).value = selected as string;
    }
  });

  document.getElementById('tunnel-form')!.addEventListener('submit', async (e) => {
    e.preventDefault();
    const errorEl = document.getElementById('form-error')!;
    errorEl.style.display = 'none';

    const config: TunnelConfig = {
      id: existing?.id || '',
      name: (document.getElementById('name') as HTMLInputElement).value.trim(),
      ssh_host: (document.getElementById('ssh_host') as HTMLInputElement).value.trim(),
      ssh_user: (document.getElementById('ssh_user') as HTMLInputElement).value.trim(),
      ssh_key_path: (document.getElementById('ssh_key_path') as HTMLInputElement).value.trim(),
      target_host: (document.getElementById('target_host') as HTMLInputElement).value.trim() || 'localhost',
      target_port: parseInt((document.getElementById('target_port') as HTMLInputElement).value) || 0,
      local_port: parseInt((document.getElementById('local_port') as HTMLInputElement).value) || 0,
      auto_reconnect: (document.getElementById('auto_reconnect') as HTMLInputElement).checked,
    };

    try {
      if (isEdit) {
        await invoke('update_tunnel', { config });
      } else {
        await invoke('add_tunnel', { config });
      }
      navigate({ view: 'list' });
    } catch (err) {
      errorEl.textContent = `${err}`;
      errorEl.style.display = 'block';
    }
  });
}

function escapeAttr(text: string): string {
  return text.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
