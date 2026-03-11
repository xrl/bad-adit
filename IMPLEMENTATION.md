# Implementation Plan

Ordered checklist. Each item is a concrete deliverable. Later items depend on earlier ones.

## Phase 1: Project Scaffolding

- [ ] **1.1** Initialize Tauri project (`cargo tauri init`) with Rust backend and web frontend
- [ ] **1.2** Set up workspace layout:
  ```
  bad-adit/
  ├── src-tauri/
  │   ├── Cargo.toml
  │   ├── src/
  │   │   ├── main.rs              # Tauri entry point, app setup, tray init
  │   │   ├── config.rs            # TunnelConfig struct, load/save, validation
  │   │   ├── tunnel.rs            # TunnelManager: owns all running tunnels
  │   │   ├── ssh.rs               # SSH child process spawn, kill, stdout capture
  │   │   ├── proxy.rs             # TCP listener + bidirectional relay
  │   │   ├── stats.rs             # TunnelStats: byte counters, connection tracking
  │   │   ├── format.rs            # Human-readable bytes, uptime formatting
  │   │   ├── tray.rs              # System tray menu construction and updates
  │   │   └── commands.rs          # Tauri IPC commands (frontend ↔ backend)
  │   └── tests/
  │       └── integration.rs       # Integration tests (sshd + echo server)
  ├── src/                         # Frontend (HTML/CSS/JS or TS)
  │   ├── index.html
  │   ├── main.ts
  │   ├── views/
  │   │   ├── TunnelList.ts        # Tunnel Configurations View
  │   │   ├── TunnelForm.ts        # Add / Edit Tunnel View
  │   │   └── TunnelStats.ts       # Stats View (auto-refresh)
  │   └── styles.css
  ├── DESIGN.md
  ├── TESTING.md
  ├── IMPLEMENTATION.md
  ├── README.md
  └── LICENSE
  ```
- [ ] **1.3** Add Rust dependencies to `Cargo.toml`:
  | Crate | Purpose |
  |---|---|
  | `tauri` | App framework, tray, IPC, window management |
  | `serde` + `serde_json` | Config serialization |
  | `tokio` | Async runtime (SSH process mgmt, TCP proxy) |
  | `uuid` | Unique tunnel IDs |
  | `dirs` | Platform config directory (`~/Library/Application Support/bad-adit/`) |
  | `log` + `env_logger` | Logging |
- [ ] **1.4** Add frontend dependencies (`package.json`):
  | Package | Purpose |
  |---|---|
  | `@tauri-apps/api` | Invoke Rust commands, listen to events |
  | `typescript` | Type safety |
  | `vite` | Frontend build tool (Tauri default) |
- [ ] **1.5** Verify `cargo tauri dev` launches an empty window with a tray icon

## Phase 2: Config Layer (`config.rs`)

- [ ] **2.1** Define `TunnelConfig` struct:
  ```rust
  pub struct TunnelConfig {
      pub id: String,           // UUID
      pub name: String,
      pub ssh_host: String,
      pub ssh_user: String,
      pub ssh_key_path: String,
      pub target_host: String,  // remote side, default "localhost"
      pub target_port: u16,
      pub local_port: u16,
      pub auto_reconnect: bool,
  }
  ```
- [ ] **2.2** Implement `ConfigStore`:
  - `load() -> Vec<TunnelConfig>` — reads `tunnels.json` from app data dir
  - `save(configs: &[TunnelConfig])` — writes `tunnels.json`
  - Creates the directory + file on first run
- [ ] **2.3** Validation:
  - Name non-empty
  - Ports in 1..=65535
  - No duplicate local ports across configs
  - SSH key path exists on disk (warn, don't block)
- [ ] **2.4** Unit tests for config:
  - Round-trip serialize/deserialize
  - Validation rejects bad inputs
  - Duplicate local port detection

## Phase 3: Stats Tracker (`stats.rs`, `format.rs`)

- [ ] **3.1** Define `TunnelStats`:
  ```rust
  pub struct TunnelStats {
      pub bytes_uploaded: AtomicU64,
      pub bytes_downloaded: AtomicU64,
      pub connections_open: AtomicU32,
      pub connections_total: AtomicU64,
      pub started_at: Instant,
      pub last_reconnect: Option<Instant>,
  }
  ```
  - All counters use atomics for lock-free updates from proxy tasks
- [ ] **3.2** Methods: `record_upload(n)`, `record_download(n)`, `connection_opened()`, `connection_closed()`, `reset()`, `snapshot() -> StatsSnapshot`
- [ ] **3.3** `StatsSnapshot` — a plain `Serialize` struct for sending to the frontend
- [ ] **3.4** Implement `format.rs`:
  - `format_bytes(n: u64) -> String` — `"0 B"`, `"1.5 KB"`, `"3.2 MB"`, `"1.1 GB"`
  - `format_uptime(duration: Duration) -> String` — `"12s"`, `"5m 30s"`, `"2h 14m"`, `"3d 1h"`
- [ ] **3.5** Unit tests:
  - Byte formatting edge cases (0, 1, 1023, 1024, 1536, u64::MAX)
  - Uptime formatting edge cases
  - Concurrent stat updates (spawn N tasks, each incrementing, verify final total)
  - Reset clears all counters

## Phase 4: SSH Process Manager (`ssh.rs`)

- [ ] **4.1** Define `SshProcess` struct:
  - Holds `tokio::process::Child`
  - Tracks assigned ephemeral port
  - Captures stdout/stderr into a ring buffer
- [ ] **4.2** `SshProcess::spawn(config, ephemeral_port) -> Result<SshProcess>`:
  - Builds command: `ssh -N -L <ephemeral>:<target_host>:<target_port> -i <key> -p 22 <user>@<host>`
  - `-o StrictHostKeyChecking=accept-new` for first-connect UX
  - `-o ExitOnForwardFailure=yes` so SSH exits if port binding fails
  - `-o ServerAliveInterval=15 -o ServerAliveCountMax=3` for health checking
  - Spawns with `tokio::process::Command`, captures stderr
- [ ] **4.3** `SshProcess::kill()` — sends SIGTERM, waits briefly, then SIGKILL
- [ ] **4.4** `SshProcess::wait_for_exit() -> ExitStatus` — used by the reconnect loop
- [ ] **4.5** Ephemeral port allocation:
  - Bind a `TcpListener` to `127.0.0.1:0`, read the assigned port, drop the listener, pass to SSH
  - There's a small TOCTOU window; SSH's `ExitOnForwardFailure` handles the race
- [ ] **4.6** Unit tests (limited — this module mostly needs integration tests):
  - Command construction produces expected args for a given config
  - Ephemeral port allocator returns valid ports

## Phase 5: TCP Proxy (`proxy.rs`)

- [ ] **5.1** `ProxyListener` struct:
  - Owns a `TcpListener` on the user-configured local port
  - Holds a reference to the tunnel's `TunnelStats`
  - Knows the SSH ephemeral port to connect to
- [ ] **5.2** Accept loop:
  - For each inbound connection, spawn a task that:
    1. Calls `stats.connection_opened()`
    2. Opens a `TcpStream` to `127.0.0.1:<ephemeral_port>`
    3. Runs `relay(client_stream, ssh_stream, stats)`
    4. On completion (either side closes/errors), calls `stats.connection_closed()`
- [ ] **5.3** `relay()` function:
  - Uses `tokio::io::copy_bidirectional` or a manual two-task split
  - Wraps each direction in a counting adapter that calls `stats.record_upload(n)` / `stats.record_download(n)` after each `write`
- [ ] **5.4** Graceful shutdown:
  - `ProxyListener::stop()` drops the listener socket
  - In-flight connections are allowed to drain (with a timeout)
- [ ] **5.5** Error behavior:
  - If connect to ephemeral port fails (SSH is down), immediately close the client connection
  - Log the error for the console view
- [ ] **5.6** Unit tests:
  - Relay with in-memory duplex streams, verify data and byte counts
  - Connection open/close counting
  - Half-close propagation (one side closes, other side sees EOF)

## Phase 6: Tunnel Manager (`tunnel.rs`)

- [ ] **6.1** Define `TunnelManager` with a coarse `Mutex`:
  ```rust
  // Singleton registered via tauri::Builder::manage()
  pub struct TunnelManager(pub Mutex<TunnelManagerInner>);

  pub struct TunnelManagerInner {
      config_store: ConfigStore,
      tunnels: HashMap<String, RunningTunnel>,
  }

  pub struct RunningTunnel {
      pub config: TunnelConfig,
      pub state: TunnelState,
      pub ssh: SshProcess,
      pub proxy: ProxyListener,
      pub stats: Arc<TunnelStats>,    // shared with proxy relay tasks
      pub reconnect_handle: JoinHandle<()>,
  }
  ```
  - `TunnelManager` is the single top-level container, wrapped in `Arc` by Tauri's `manage()`
  - All IPC commands lock the `Mutex`, do their work, and release it
  - `Arc<TunnelStats>` is the one thing that lives *outside* the lock — proxy
    relay tasks hold a clone and update atomics directly, no contention with IPC
  - This is simple and sufficient: IPC commands are fast (start/stop/read stats),
    and the tray refresh (every 1–2s) is the most frequent caller
- [ ] **6.2** `TunnelState` enum: `Starting`, `Running`, `Reconnecting`, `Stopped`, `Error(String)`
- [ ] **6.3** `start_tunnel(config) -> Result<()>`:
  1. Allocate ephemeral port
  2. Spawn SSH process
  3. Brief delay / port-readiness check (try connecting to ephemeral port with retries)
  4. Start proxy listener
  5. Spawn reconnect watcher task
- [ ] **6.4** `stop_tunnel(id) -> Result<()>`:
  1. Stop proxy listener (drain in-flight)
  2. Kill SSH process
  3. Reset stats
  4. Set state to `Stopped`
- [ ] **6.5** Reconnect watcher (per tunnel):
  - Awaits `SshProcess::wait_for_exit()`
  - If `auto_reconnect` is enabled and tunnel wasn't explicitly stopped:
    1. Set state to `Reconnecting`
    2. Backoff retry: spawn new SSH process
    3. Wait for port readiness
    4. Update `ProxyListener` with new ephemeral port
    5. Set `last_reconnect` in stats, reset counters
    6. Set state to `Running`
  - If reconnect fails after N attempts, set state to `Error`
- [ ] **6.6** `restart_tunnel(id, new_config)`:
  - Stop old tunnel, start with new config
  - Used for config-edit-while-running flow
- [ ] **6.7** `get_all_status() -> Vec<TunnelStatus>`:
  - Returns ID, name, state, and stats snapshot for every configured tunnel
  - Used by both tray menu and frontend

## Phase 7: Tauri IPC Commands (`commands.rs`)

- [ ] **7.1** Config commands:
  - `get_tunnels() -> Vec<TunnelConfig>`
  - `add_tunnel(config) -> Result<TunnelConfig>`
  - `update_tunnel(config) -> Result<TunnelConfig>`
  - `remove_tunnel(id) -> Result<()>`
- [ ] **7.2** Tunnel control commands:
  - `start_tunnel(id) -> Result<()>`
  - `stop_tunnel(id) -> Result<()>`
  - `restart_tunnel(id) -> Result<()>`
- [ ] **7.3** Stats commands:
  - `get_tunnel_stats(id) -> Result<StatsSnapshot>`
  - `get_all_tunnel_status() -> Vec<TunnelStatus>` (for tray menu refresh)
- [ ] **7.4** Register all commands in `main.rs` via `tauri::Builder::invoke_handler`

## Phase 8: System Tray (`tray.rs`)

- [ ] **8.1** Build initial tray menu on app start:
  - App title "Bad Adit"
  - Separator
  - One item per configured tunnel (all showing ○ initially)
  - Separator
  - "Edit Tunnel Configurations..." item
- [ ] **8.2** Tray menu click handler:
  - Tunnel item clicked → toggle start/stop via `TunnelManager`
  - "Edit Tunnel Configurations..." → open/focus the main Tauri window
- [ ] **8.3** Periodic tray refresh (every 1–2 seconds):
  - Rebuild menu items with current state (●/○) and byte counters (↑/↓)
  - Update tray icon: green if any tunnel is running, red if all stopped/errored
- [ ] **8.4** Tray icon assets:
  - Green traffic light icon (some tunnels active)
  - Red traffic light icon (no tunnels active / all errored)
  - Include as Tauri resources

## Phase 9: Frontend — Tunnel Configurations View

- [ ] **9.1** `TunnelList` view:
  - Fetch tunnel list via `invoke("get_tunnels")`
  - Render each tunnel: name, port mapping summary, [Edit] and [Remove] buttons
  - [Add] button at top
  - Click tunnel name → navigate to Stats view
- [ ] **9.2** Remove flow:
  - Check if tunnel is running → show confirmation dialog if so
  - Call `invoke("stop_tunnel")` if running, then `invoke("remove_tunnel")`
- [ ] **9.3** Navigation:
  - Simple client-side router (hash-based or state-based, no framework needed)
  - Routes: `/` (list), `/add`, `/edit/:id`, `/stats/:id`

## Phase 10: Frontend — Add / Edit Tunnel View

- [ ] **10.1** `TunnelForm` view:
  - Fields: name, SSH host, SSH user, SSH key (with file picker), target host, target port, local port, auto-reconnect toggle
  - Pre-fill when editing (pass tunnel ID via route)
- [ ] **10.2** Validation:
  - Client-side: required fields, port ranges
  - Server-side: `add_tunnel` / `update_tunnel` commands return errors
- [ ] **10.3** Save flow:
  - New tunnel → `invoke("add_tunnel")`
  - Edit → `invoke("update_tunnel")`, then check if tunnel was running
  - If running → show "Restart now?" dialog → `invoke("restart_tunnel")` or defer
- [ ] **10.4** File picker for SSH key:
  - Use Tauri's `dialog.open()` API to browse for the key file

## Phase 11: Frontend — Tunnel Stats View

- [ ] **11.1** `TunnelStats` view:
  - Back button → return to list
  - Show tunnel name, port mapping, running state badge
  - Traffic section: uploaded, downloaded (formatted)
  - Connections section: currently open, total handled
  - Session section: uptime, last reconnect
- [ ] **11.2** Auto-refresh:
  - `setInterval` every 1 second calling `invoke("get_tunnel_stats", { id })`
  - Clear interval on navigation away
- [ ] **11.3** Start/Stop control:
  - Button on the stats view to toggle the tunnel (reflects current state)

## Phase 12: Integration Tests

- [ ] **12.1** Test harness setup (`tests/integration.rs`):
  - Read `TEST_SSH_PORT`, `TEST_SSH_KEY`, `TEST_ECHO_PORT` from env
  - Skip all tests if env vars are missing (allows `cargo test` locally without sshd)
  - Helper: `create_test_config(local_port) -> TunnelConfig`
- [ ] **12.2** Basic tunnel lifecycle test
- [ ] **12.3** Stats accuracy test (known payload, verify byte counts)
- [ ] **12.4** Multiple concurrent connections test
- [ ] **12.5** Auto-reconnect test (kill SSH child, verify recovery)
- [ ] **12.6** SSH failure test (bad host, verify error state)
- [ ] **12.7** Config edit while running test (port change + restart)

## Phase 13: CI Pipeline

- [ ] **13.1** Create `.github/workflows/ci.yml`:
  - `test` job: Ubuntu, sshd + socat setup, `cargo test --lib`, `cargo test --test integration`
  - `lint` job: `cargo fmt --check`, `cargo clippy -- -D warnings`
- [ ] **13.2** Verify CI passes on a clean push

## Phase 14: Polish & Release

- [ ] **14.1** App icon (custom traffic light icon for macOS dock + tray)
- [ ] **14.2** macOS code signing and notarization config (Tauri built-in support)
- [ ] **14.3** `cargo tauri build` produces a `.dmg`
- [ ] **14.4** README updated with screenshots and final install instructions
