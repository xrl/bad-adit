# Implementation Plan

Ordered checklist. Each item is a concrete deliverable. Later items depend on earlier ones.

## Phase 1: Project Scaffolding

- [x] **1.1** Initialize Tauri project (`cargo tauri init`) with Rust backend and web frontend
- [x] **1.2** Set up workspace layout:
  ```
  bad-adit/
  ├── src-tauri/
  │   ├── Cargo.toml
  │   ├── src/
  │   │   ├── main.rs              # Tauri entry point, app setup, tray init
  │   │   ├── lib.rs               # Library target for integration test imports
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
  │   ├── main.ts
  │   ├── views/
  │   │   ├── TunnelList.ts        # Tunnel Configurations View
  │   │   ├── TunnelForm.ts        # Add / Edit Tunnel View
  │   │   └── TunnelStats.ts       # Stats View (auto-refresh)
  │   └── styles.css
  ├── index.html
  ├── DESIGN.md
  ├── TESTING.md
  ├── IMPLEMENTATION.md
  ├── README.md
  └── LICENSE
  ```
- [x] **1.3** Add Rust dependencies to `Cargo.toml`:
  | Crate | Purpose |
  |---|---|
  | `tauri` | App framework, tray, IPC, window management |
  | `tauri-plugin-dialog` | File picker dialog for SSH key selection |
  | `serde` + `serde_json` | Config serialization |
  | `tokio` | Async runtime (SSH process mgmt, TCP proxy) |
  | `uuid` | Unique tunnel IDs |
  | `dirs` | Platform config directory (`~/Library/Application Support/bad-adit/`) |
  | `log` + `env_logger` | Logging |
  | `tempfile` (dev) | Temporary files for config tests |
- [x] **1.4** Add frontend dependencies (`package.json`):
  | Package | Purpose |
  |---|---|
  | `@tauri-apps/api` | Invoke Rust commands, listen to events |
  | `@tauri-apps/plugin-dialog` | File picker for SSH key |
  | `typescript` | Type safety |
  | `vite` | Frontend build tool (Tauri default) |
- [ ] **1.5** Verify `cargo tauri dev` launches an empty window with a tray icon

## Phase 2: Config Layer (`config.rs`)

- [x] **2.1** Define `TunnelConfig` struct:
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
- [x] **2.2** Implement `ConfigStore`:
  - `load() -> Vec<TunnelConfig>` — reads `tunnels.json` from app data dir
  - `save(configs: &[TunnelConfig])` — writes `tunnels.json`
  - Creates the directory + file on first run
- [x] **2.3** Validation:
  - Name non-empty
  - Ports in 1..=65535
  - No duplicate local ports across configs
  - SSH key path exists on disk (warn, don't block)
- [x] **2.4** Unit tests for config:
  - Round-trip serialize/deserialize
  - Validation rejects bad inputs
  - Duplicate local port detection

## Phase 3: Stats Tracker (`stats.rs`, `format.rs`)

- [x] **3.1** Define `TunnelStats`:
  ```rust
  pub struct TunnelStats {
      pub bytes_uploaded: AtomicU64,
      pub bytes_downloaded: AtomicU64,
      pub connections_open: AtomicU32,
      pub connections_total: AtomicU64,
      pub started_at: Instant,
      pub last_reconnect: Mutex<Option<Instant>>,
  }
  ```
  - All counters use atomics for lock-free updates from proxy tasks
- [x] **3.2** Methods: `record_upload(n)`, `record_download(n)`, `connection_opened()`, `connection_closed()`, `reset()`, `snapshot() -> StatsSnapshot`
- [x] **3.3** `StatsSnapshot` — a plain `Serialize` struct for sending to the frontend
- [x] **3.4** Implement `format.rs`:
  - `format_bytes(n: u64) -> String` — `"0 B"`, `"1.5 KB"`, `"3.2 MB"`, `"1.1 GB"`
  - `format_uptime(duration: Duration) -> String` — `"12s"`, `"5m 30s"`, `"2h 14m"`, `"3d 1h"`
- [x] **3.5** Unit tests:
  - Byte formatting edge cases (0, 1, 1023, 1024, 1536, u64::MAX)
  - Uptime formatting edge cases
  - Concurrent stat updates (spawn N tasks, each incrementing, verify final total)
  - Reset clears all counters

## Phase 4: SSH Process Manager (`ssh.rs`)

- [x] **4.1** Define `SshProcess` struct:
  - Holds `tokio::process::Child`
  - Tracks assigned ephemeral port
  - Captures stderr via mpsc channel
- [x] **4.2** `SshProcess::spawn(config, ephemeral_port, log_tx) -> Result<SshProcess>`:
  - Builds command: `ssh -N -L <ephemeral>:<target_host>:<target_port> -i <key> <user>@<host>`
  - `-o StrictHostKeyChecking=accept-new` for first-connect UX
  - `-o ExitOnForwardFailure=yes` so SSH exits if port binding fails
  - `-o ServerAliveInterval=15 -o ServerAliveCountMax=3` for health checking
  - Spawns with `tokio::process::Command`, captures stderr
  - Uses `kill_on_drop(true)` for safety
- [x] **4.3** `SshProcess::kill()` — kills the child process
- [x] **4.4** `SshProcess::wait_for_exit() -> ExitStatus` — used by the reconnect loop
- [x] **4.5** Ephemeral port allocation:
  - Bind a `TcpListener` to `127.0.0.1:0`, read the assigned port, drop the listener, pass to SSH
  - There's a small TOCTOU window; SSH's `ExitOnForwardFailure` handles the race
- [x] **4.6** Unit tests (limited — this module mostly needs integration tests):
  - Command construction produces expected args for a given config
  - Command construction without SSH key omits `-i` flag
  - Ephemeral port allocator returns valid ports

## Phase 5: TCP Proxy (`proxy.rs`)

- [x] **5.1** `ProxyListener` struct:
  - Owns a `TcpListener` on the user-configured local port
  - Holds a reference to the tunnel's `TunnelStats`
  - Knows the SSH ephemeral port to connect to
  - Uses `watch` channel for graceful shutdown
- [x] **5.2** Accept loop:
  - For each inbound connection, spawn a task that:
    1. Calls `stats.connection_opened()`
    2. Opens a `TcpStream` to `127.0.0.1:<ephemeral_port>`
    3. Runs `relay(client_stream, ssh_stream, stats)`
    4. On completion (either side closes/errors), calls `stats.connection_closed()`
- [x] **5.3** `relay()` function:
  - Manual two-task split with `tokio::join!`
  - Each direction reads into a buffer, records stats, and writes to the other side
  - Propagates shutdown when one side closes
- [x] **5.4** Graceful shutdown:
  - `ProxyListener::stop()` signals via watch channel
  - In-flight connections are allowed to drain (with a 5s timeout)
- [x] **5.5** Error behavior:
  - If connect to ephemeral port fails (SSH is down), immediately close the client connection
  - Log the error for the console view
- [x] **5.6** Unit tests:
  - Stats counting (connection open/close, byte counts)
  - Full relay test with real TCP (echo server + proxy, verify data and byte counts)

## Phase 6: Tunnel Manager (`tunnel.rs`)

- [x] **6.1** Define `TunnelManager` with a `tokio::sync::Mutex`:
  ```rust
  pub struct TunnelManager(pub Mutex<TunnelManagerInner>);

  pub struct TunnelManagerInner {
      pub config_store: ConfigStore,
      pub tunnels: HashMap<String, RunningTunnel>,
  }

  pub struct RunningTunnel {
      pub config: TunnelConfig,
      pub state: TunnelState,
      pub stats: Arc<TunnelStats>,
      pub proxy: Option<ProxyListener>,
      pub ssh: Option<SshProcess>,
      pub log_lines: Vec<String>,
      pub log_rx: Option<mpsc::UnboundedReceiver<String>>,
      pub reconnect_cancel: Option<watch::Sender<bool>>,
  }
  ```
  - Uses `tokio::sync::Mutex` (not `std::sync::Mutex`) so guards can be held across `.await` points
  - `Arc<TunnelStats>` lives outside the lock — proxy relay tasks update atomics directly
- [x] **6.2** `TunnelState` enum: `Starting`, `Running`, `Reconnecting`, `Stopped`, `Error(String)`
- [x] **6.3** `start_tunnel(config) -> Result<()>`:
  1. Allocate ephemeral port
  2. Spawn SSH process
  3. Wait for port readiness (10 retries, 200ms delay)
  4. Start proxy listener
- [x] **6.4** `stop_tunnel(id) -> Result<()>`:
  1. Cancel reconnect watcher if running
  2. Stop proxy listener (drain in-flight)
  3. Kill SSH process
  4. Reset stats
  5. Set state to `Stopped`
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
- [x] **6.6** `restart_tunnel(id, new_config)`:
  - Stop old tunnel, start with new config
  - Used for config-edit-while-running flow
- [x] **6.7** `get_all_status() -> Vec<TunnelStatus>`:
  - Returns ID, name, state, and stats snapshot for every configured tunnel
  - Used by both tray menu and frontend

## Phase 7: Tauri IPC Commands (`commands.rs`)

- [x] **7.1** Config commands:
  - `get_tunnels() -> Vec<TunnelConfig>`
  - `add_tunnel(config) -> Result<TunnelConfig>`
  - `update_tunnel(config) -> Result<TunnelConfig>`
  - `remove_tunnel(id) -> Result<()>`
- [x] **7.2** Tunnel control commands:
  - `start_tunnel(id) -> Result<()>`
  - `stop_tunnel(id) -> Result<()>`
  - `restart_tunnel(id) -> Result<()>`
- [x] **7.3** Stats commands:
  - `get_tunnel_stats(id) -> Result<StatsSnapshot>`
  - `get_all_tunnel_status() -> Vec<TunnelStatus>` (for tray menu refresh)
- [x] **7.4** Register all commands in `main.rs` via `tauri::Builder::invoke_handler`

## Phase 8: System Tray (`tray.rs`)

- [x] **8.1** Build initial tray menu on app start:
  - App title "Bad Adit"
  - Separator
  - One item per configured tunnel (all showing ○ initially)
  - Separator
  - "Edit Tunnel Configurations..." item
- [x] **8.2** Tray menu click handler:
  - Tunnel item clicked → toggle start/stop via `TunnelManager`
  - "Edit Tunnel Configurations..." → open/focus the main Tauri window
- [x] **8.3** Periodic tray refresh (every 2 seconds):
  - Rebuild menu items with current state (●/○/◐) and byte counters (↑/↓)
- [x] **8.4** Tray icon assets:
  - Green circle icon (some tunnels active)
  - Red circle icon (no tunnels active / all errored)
  - Included as Tauri resources

## Phase 9: Frontend — Tunnel Configurations View

- [x] **9.1** `TunnelList` view:
  - Fetch tunnel list via `invoke("get_tunnels")`
  - Render each tunnel: name, port mapping summary, [Edit] and [Remove] buttons
  - [Add] button at top
  - Click tunnel name → navigate to Stats view
- [x] **9.2** Remove flow:
  - Show confirmation dialog
  - Call `invoke("remove_tunnel")` (backend handles stopping if running)
- [x] **9.3** Navigation:
  - Simple state-based router (no framework needed)
  - Routes: list, add, edit/:id, stats/:id

## Phase 10: Frontend — Add / Edit Tunnel View

- [x] **10.1** `TunnelForm` view:
  - Fields: name, SSH host, SSH user, SSH key (with file picker), target host, target port, local port, auto-reconnect toggle
  - Pre-fill when editing (pass tunnel ID via route)
- [x] **10.2** Validation:
  - Client-side: required fields via HTML5 attributes
  - Server-side: `add_tunnel` / `update_tunnel` commands return errors displayed inline
- [x] **10.3** Save flow:
  - New tunnel → `invoke("add_tunnel")`
  - Edit → `invoke("update_tunnel")`
- [x] **10.4** File picker for SSH key:
  - Use `@tauri-apps/plugin-dialog` `open()` API to browse for the key file

## Phase 11: Frontend — Tunnel Stats View

- [x] **11.1** `TunnelStats` view:
  - Back button → return to list
  - Show tunnel name, port mapping, running state badge
  - Traffic section: uploaded, downloaded (formatted)
  - Connections section: currently open, total handled
  - Session section: uptime, last reconnect
- [x] **11.2** Auto-refresh:
  - `setInterval` every 1 second calling `invoke("get_all_tunnel_status")`
  - Clear interval on navigation away (cleanup function returned)
- [x] **11.3** Start/Stop control:
  - Button on the stats view to toggle the tunnel (reflects current state)

## Phase 12: Integration Tests

- [x] **12.1** Test harness setup (`tests/integration.rs`):
  - Read `TEST_SSH_PORT`, `TEST_SSH_KEY`, `TEST_ECHO_PORT` from env
  - Skip all tests if env vars are missing (allows `cargo test` locally without sshd)
  - Helper: `create_test_config(ssh_port, ssh_key, echo_port, local_port)`
- [x] **12.2** Basic tunnel lifecycle test
- [x] **12.3** Stats accuracy test (known 4096-byte payload, verify byte counts)
- [x] **12.4** Multiple concurrent connections test (open 5, close 3, verify counts)
- [ ] **12.5** Auto-reconnect test (kill SSH child, verify recovery)
- [ ] **12.6** SSH failure test (bad host, verify error state)
- [ ] **12.7** Config edit while running test (port change + restart)

## Phase 13: CI Pipeline

- [x] **13.1** Create `.github/workflows/ci.yml`:
  - `test` job: Ubuntu, sshd + socat setup, `cargo test --lib`, `cargo test --test integration`
  - `lint` job: `cargo fmt --check`, `cargo clippy -- -D warnings`
  - `frontend` job: `npx tsc --noEmit`
- [ ] **13.2** Verify CI passes on a clean push

## Phase 14: Polish & Release

- [ ] **14.1** App icon (custom traffic light icon for macOS dock + tray)
- [ ] **14.2** macOS code signing and notarization config (Tauri built-in support)
- [ ] **14.3** `cargo tauri build` produces a `.dmg`
- [ ] **14.4** README updated with screenshots and final install instructions
