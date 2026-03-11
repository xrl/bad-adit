# Bad Adit

> An **adit** is a horizontal tunnel dug into a hillside. A *bad* one? That's the SSH tunnel you forgot about three terminals ago.

Bad Adit is a macOS menu bar app for managing SSH tunnels. No more lost terminal tabs, forgotten port numbers, or mysterious `ssh -L` invocations buried in your shell history.

## Features

- **Menu bar status indicator** — see at a glance whether your tunnels are up (●), starting (◐), or down (○)
- **Live traffic stats** — per-tunnel byte counters (uploaded/downloaded) right in the tray menu
- **Tunnel configuration management** — define and save tunnel profiles with SSH host, key, ports, and auto-reconnect
- **Per-tunnel stats view** — connections open, total handled, uptime, and formatted traffic counters
- **Multiple simultaneous tunnels** — run as many tunnels as you need, each independently managed

## How It Works

Bad Adit spawns `ssh -L` processes and proxies TCP traffic through them. The proxy layer sits between your local port and SSH's ephemeral port, counting every byte that flows through. This gives you accurate per-tunnel traffic stats without touching the SSH protocol.

For unprivileged ports (≥1024):

```
[your app] → [local port: proxy] → [ephemeral port: ssh -L] → [remote host:port]
```

### Privileged Ports (<1024)

Binding ports below 1024 requires root. Bad Adit handles this transparently using a self-exec pattern with macOS's native admin authorization dialog:

```
[your app] → [port 443: root forwarder] → [random port: stats proxy] → [ephemeral port: ssh -L] → [remote host:port]
```

When you configure a tunnel on a privileged port (e.g. 443), Bad Adit:

1. **Binds a stats proxy** on a random unprivileged port that tracks bytes and forwards to the SSH tunnel's ephemeral port
2. **Re-executes itself** as root via `osascript "do shell script ... with administrator privileges"`, passing `--privileged-forwarder <local_port> <stats_port> <parent_pid>`
3. The **root forwarder** binds the privileged port and forwards all TCP connections to the stats proxy

This keeps the root process minimal (a simple TCP forwarder with no Tauri/tokio dependencies) while the unprivileged main process handles stats tracking, SSH management, and UI.

#### Orphan Prevention

The root forwarder includes a **parent-PID watchdog** that polls every 2 seconds using `kill(parent_pid, 0)`. If the parent process exits (e.g. `cargo tauri dev` restarts, crash, or normal shutdown), the forwarder exits immediately. This prevents orphaned root processes that would squat on the privileged port.

## Install

```sh
brew install xrl/bad-adit/bad-adit
```

This taps the repo and installs the latest release. To update later:

```sh
brew upgrade bad-adit
```

## Prerequisites (development)

- macOS (Apple Silicon)
- [Rust toolchain](https://rustup.rs/) (1.75+)
- [Node.js](https://nodejs.org/) (22+)
- SSH client (ships with macOS)

## Getting Started

```sh
# Clone the repo
git clone https://github.com/xrl/bad-adit.git
cd bad-adit

# Install frontend dependencies
npm install

# Run in development mode (launches the app with hot reload)
cargo tauri dev
```

The app starts in the menu bar. Click the tray icon to see your tunnels. Click "Edit Tunnel Configurations..." to open the main window where you can add, edit, and remove tunnels.

## Running Tests

```sh
# Unit tests (no external dependencies needed)
cd src-tauri
cargo test --lib

# Integration tests (require a local SSH server + echo server)
# Set up the test environment first:
ssh-keygen -t ed25519 -N "" -f /tmp/test_key
cat /tmp/test_key.pub >> ~/.ssh/authorized_keys
chmod 600 ~/.ssh/authorized_keys

# Start an echo server (install socat if needed: brew install socat)
socat TCP-LISTEN:9999,fork,reuseaddr EXEC:cat &

# Make sure sshd is running (macOS: System Settings → General → Sharing → Remote Login)
# Then run:
TEST_SSH_PORT=22 TEST_SSH_KEY=/tmp/test_key TEST_ECHO_PORT=9999 cargo test --test integration
```

## Building for Production

```sh
cargo tauri build
```

This produces a `.dmg` in `src-tauri/target/release/bundle/dmg/`.

## Configuration

Tunnel configs are stored in `~/Library/Application Support/bad-adit/tunnels.json`. Each tunnel definition includes:

| Field | Description |
|---|---|
| **Name** | Human-readable label for the tunnel |
| **Host** | Remote SSH server hostname or IP |
| **User** | SSH username |
| **SSH Key** | Path to the private key file |
| **Local Port** | Port on your machine to listen on |
| **Remote Host** | Destination host on the remote network (default: `localhost`) |
| **Remote Port** | Port on the remote host to forward to |
| **Auto-reconnect** | Automatically restart the tunnel if it drops |

## Tech Stack

- **Rust** + **Tauri v2** — backend tunnel management, system tray, IPC
- **TypeScript** + **Vite** — lightweight frontend (no framework)
- **tokio** — async SSH process management and TCP proxy

## License

[MIT](LICENSE)
