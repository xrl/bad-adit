# Design

## Screens

### Tray Icon Dropdown

```
Bad Adit
──────────────────────
● Production DB         ↑ 1.2 MB  ↓ 48 KB
○ Staging Redis
● Dev API Gateway       ↑ 320 B   ↓ 1.1 KB
──────────────────────
Edit Tunnel Configurations...
```

- Each tunnel shows a solid circle (●) when connected, hollow circle (○) when disconnected
- Active tunnels show compact upload/download totals (human-readable: B, KB, MB, GB)
- Single click on a tunnel toggles it on/off
- "Edit Tunnel Configurations..." opens the main Tauri window

### Tunnel Configurations View (main window)

```
Tunnels                                    [Add]
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Production DB (localhost:5432 → prod:5432)   [Edit] [Remove]
Staging Redis (localhost:6379 → staging:6379) [Edit] [Remove]
Dev API Gateway (localhost:8080 → dev:443)   [Edit] [Remove]
```

- Lists all configured tunnels with a summary of the port mapping
- [Add] opens the Add/Edit Tunnel view
- [Edit] opens the same view pre-filled with the tunnel's config
- [Remove] deletes the tunnel (with confirmation if the tunnel is currently active)

### Add / Edit Tunnel View (main window)

```
Tunnel Configuration
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Tunnel Name:     [________________________]
Target Host:     [________________________]
Target Port:     [________________________]
SSH Key File:    [________________________] [Browse]
Local Port:      [________________________]

                              [Cancel] [Save]
```

- Same view is used for both adding and editing tunnels
- When editing a tunnel that is currently active, saving shows a confirmation dialog:
  **"This tunnel is currently running. Restart it now with the new configuration?"**
  with [Restart Now] and [Restart Later] buttons

### Tunnel Stats View (main window)

Accessed by clicking a tunnel name from the Tunnel Configurations View.

```
← Back to Tunnels

Production DB                                    [● Running]
localhost:5432 → prod.example.com:5432
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Traffic
  Uploaded:          1.2 MB
  Downloaded:        48.3 KB

Connections
  Currently open:    3
  Total handled:     47

Session
  Uptime:            2h 14m
  Last reconnect:    —
```

- Auto-refreshes every 1 second while the view is open
- Stats reset when the tunnel is stopped
- "Currently open" = active TCP connections being proxied right now
- "Total handled" = cumulative connection count since tunnel was started

## Architecture

### Proxy Passthrough

Bad Adit does **not** bind the SSH tunnel directly to the user-configured local port. Instead:

1. SSH tunnel binds to a random ephemeral port on localhost (`ssh -L <ephemeral>:<target_host>:<target_port>`)
2. Bad Adit listens on the user-configured local port
3. For each incoming connection, Bad Adit opens a connection to the ephemeral SSH port and bidirectionally proxies data

This gives us:

- **Byte counting** — exact upload/download per tunnel (and per connection if needed)
- **Connection tracking** — open count, total handled, connection durations
- **Latency measurement** — time-to-first-byte on proxy connections as a rough RTT indicator
- **Graceful behavior** — if SSH dies, Bad Adit can queue/reject connections with a clear error instead of silent TCP resets

The overhead of the extra localhost hop is sub-millisecond and negligible for tunneled traffic.

## Behavior Notes

- Tunnel state (on/off) is ephemeral — it is not persisted across app restarts
- Tunnel configurations are persisted to disk
- Toggling a tunnel on from the tray menu starts the SSH process immediately
- If auto-reconnect is enabled and the SSH process dies, it is restarted automatically
