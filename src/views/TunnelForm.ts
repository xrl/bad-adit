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
    ${!isEdit ? '<button type="button" id="import-btn" class="import-btn">Import <code>ssh -L</code> command</button>' : ''}
    <div id="import-modal" class="modal-overlay" style="display:none">
      <div class="modal">
        <h2>Import SSH Command</h2>
        <p class="modal-hint">Paste an <code>ssh -L</code> command, e.g.<br><code>ssh -N -L 443:localhost:443 ec2-user@52.5.125.19</code></p>
        <div class="form-group">
          <textarea id="import-input" rows="3" placeholder="ssh -N -L 5432:db.internal:5432 deploy@bastion.example.com"></textarea>
        </div>
        <div id="import-preview" style="display:none"></div>
        <div id="import-error" class="error-text" style="display:none"></div>
        <div class="form-actions">
          <button type="button" id="import-cancel">Cancel</button>
          <button type="button" id="import-apply" class="primary" disabled>Apply</button>
        </div>
      </div>
    </div>
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
    try {
      const homeDir = await invoke<string>('get_home_dir');
      const selected = await open({
        multiple: false,
        directory: false,
        defaultPath: `${homeDir}/.ssh`,
      });
      if (selected) {
        (document.getElementById('ssh_key_path') as HTMLInputElement).value = selected as string;
      }
    } catch (e) {
      console.error('File picker error:', e);
    }
  });

  // Import SSH command modal
  const importBtn = document.getElementById('import-btn');
  const importModal = document.getElementById('import-modal')!;
  const importInput = document.getElementById('import-input') as HTMLTextAreaElement | null;
  const importPreview = document.getElementById('import-preview')!;
  const importError = document.getElementById('import-error')!;
  const importApply = document.getElementById('import-apply') as HTMLButtonElement | null;

  let parsedImport: ParsedSshCommand | null = null;

  if (importBtn) {
    importBtn.addEventListener('click', () => {
      importModal.style.display = 'flex';
      importInput?.focus();
    });
  }

  document.getElementById('import-cancel')?.addEventListener('click', () => {
    importModal.style.display = 'none';
    if (importInput) importInput.value = '';
    importPreview.style.display = 'none';
    importError.style.display = 'none';
    parsedImport = null;
  });

  importModal.addEventListener('click', (e) => {
    if (e.target === importModal) {
      importModal.style.display = 'none';
    }
  });

  importInput?.addEventListener('input', () => {
    const val = importInput.value.trim();
    importError.style.display = 'none';
    importPreview.style.display = 'none';
    parsedImport = null;
    if (importApply) importApply.disabled = true;

    if (!val) return;

    const result = parseSshCommand(val);
    if (result.error) {
      importError.textContent = result.error;
      importError.style.display = 'block';
      return;
    }

    parsedImport = result;
    const warnings: string[] = [];
    if (result.local_port! <= 1024) {
      warnings.push(`Port ${result.local_port} requires <code>sudo</code> (privileged port)`);
    }
    if (result.target_port! <= 1024 && result.target_port !== result.local_port) {
      warnings.push(`Target port ${result.target_port} is a privileged port`);
    }

    importPreview.innerHTML = `
      <div class="import-fields">
        <div class="import-field"><span class="import-label">SSH Host</span><span class="import-value">${escapeAttr(result.ssh_host!)}</span></div>
        <div class="import-field"><span class="import-label">SSH User</span><span class="import-value">${escapeAttr(result.ssh_user!)}</span></div>
        <div class="import-field"><span class="import-label">Target Host</span><span class="import-value">${escapeAttr(result.target_host!)}</span></div>
        <div class="import-field"><span class="import-label">Target Port</span><span class="import-value">${result.target_port}</span></div>
        <div class="import-field"><span class="import-label">Local Port</span><span class="import-value">${result.local_port}</span></div>
        ${result.ssh_key ? `<div class="import-field"><span class="import-label">SSH Key</span><span class="import-value">${escapeAttr(result.ssh_key)}</span></div>` : ''}
      </div>
      ${warnings.length ? `<div class="import-warnings">${warnings.map(w => `<div class="import-warning">${w}</div>`).join('')}</div>` : ''}
    `;
    importPreview.style.display = 'block';
    if (importApply) importApply.disabled = false;
  });

  importApply?.addEventListener('click', () => {
    if (!parsedImport) return;

    (document.getElementById('ssh_host') as HTMLInputElement).value = parsedImport.ssh_host || '';
    (document.getElementById('ssh_user') as HTMLInputElement).value = parsedImport.ssh_user || '';
    (document.getElementById('target_host') as HTMLInputElement).value = parsedImport.target_host || '';
    (document.getElementById('target_port') as HTMLInputElement).value = String(parsedImport.target_port || '');
    (document.getElementById('local_port') as HTMLInputElement).value = String(parsedImport.local_port || '');
    if (parsedImport.ssh_key) {
      (document.getElementById('ssh_key_path') as HTMLInputElement).value = parsedImport.ssh_key;
    }
    // Auto-generate a name from the command
    const nameInput = document.getElementById('name') as HTMLInputElement;
    if (!nameInput.value) {
      nameInput.value = `${parsedImport.ssh_host}:${parsedImport.target_port}`;
    }

    importModal.style.display = 'none';
    if (importInput) importInput.value = '';
    importPreview.style.display = 'none';
    parsedImport = null;
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

interface ParsedSshCommand {
  ssh_host?: string;
  ssh_user?: string;
  ssh_key?: string;
  target_host?: string;
  target_port?: number;
  local_port?: number;
  error?: string;
}

function parseSshCommand(input: string): ParsedSshCommand {
  // Normalize: strip leading whitespace, handle multi-line pastes
  const cmd = input.trim().replace(/\\\n/g, ' ');

  // Must contain ssh somewhere
  if (!cmd.includes('ssh')) {
    return { error: 'Not an SSH command' };
  }

  // Find -L argument: local_port:target_host:target_port
  const lMatch = cmd.match(/-L\s+(\d+):([^:\s]+):(\d+)/);
  if (!lMatch) {
    return { error: 'No -L port forward found. Expected format: ssh -L local:host:remote user@host' };
  }

  const local_port = parseInt(lMatch[1]);
  const target_host = lMatch[2];
  const target_port = parseInt(lMatch[3]);

  if (local_port < 1 || local_port > 65535 || target_port < 1 || target_port > 65535) {
    return { error: 'Port numbers must be between 1 and 65535' };
  }

  // Find user@host — the last non-flag argument
  const userHostMatch = cmd.match(/(\S+)@(\S+)\s*$/);
  if (!userHostMatch) {
    return { error: 'No user@host found at end of command' };
  }

  const ssh_user = userHostMatch[1];
  const ssh_host = userHostMatch[2];

  // Find optional -i key_path
  const keyMatch = cmd.match(/-i\s+(\S+)/);
  const ssh_key = keyMatch ? keyMatch[1] : undefined;

  return { ssh_host, ssh_user, ssh_key, target_host, target_port, local_port };
}
