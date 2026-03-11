# Bad Adit

> An **adit** is a horizontal tunnel dug into a hillside. A *bad* one? That's the SSH tunnel you forgot about three terminals ago.

Bad Adit is a macOS menu bar app for managing SSH tunnels. No more lost terminal tabs, forgotten port numbers, or mysterious `ssh -L` invocations buried in your shell history.

## Features

- **Menu bar status indicator** — green/red traffic light shows at a glance whether your tunnels are up or down
- **Tunnel configuration management** — define and save tunnel profiles with:
  - Remote host and user
  - SSH key path
  - Local (incoming) port
  - Remote (outgoing) port and host
  - Auto-reconnect on disconnect
- **Password caching** — enter your passphrase once at startup; Bad Adit remembers it for the session
- **Per-tunnel console** — view live stdout/stderr output and connection history for each tunnel process
- **Multiple simultaneous tunnels** — run as many tunnels as you need, each independently managed

## Tech Stack

- **Rust** — backend tunnel management and SSH process orchestration
- **Tauri** — lightweight native app shell with system tray integration
- **macOS** — primary target platform (system tray APIs)

## How It Works

Bad Adit spawns and supervises `ssh` processes under the hood. Each tunnel configuration maps to a managed child process with:

1. Lifecycle management (start / stop / restart)
2. Health monitoring with optional auto-reconnect
3. Output capture for the built-in console view
4. Passphrase forwarding via SSH_ASKPASS or equivalent mechanism

## Getting Started

### Prerequisites

- macOS
- Rust toolchain (`rustup`)
- Node.js (for Tauri frontend)
- An SSH client installed (ships with macOS)

### Build & Run

```sh
# Install dependencies
npm install

# Run in development mode
cargo tauri dev

# Build for production
cargo tauri build
```

## Configuration

Tunnel configs are stored locally on disk. Each tunnel definition includes:

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

## License

[MIT](LICENSE)
