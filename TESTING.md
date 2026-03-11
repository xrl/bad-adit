# Testing Strategy

## Unit Tests

These test isolated components with no external dependencies.

### Config Layer
- Parse/serialize tunnel configs to/from disk format
- Validate required fields (name, host, port, key path, local port)
- Reject invalid configs (port out of range, empty name, duplicate local ports)

### Stats Tracker
- Increment byte counters, verify totals
- Track open/closed connections, verify counts
- Reset stats on tunnel stop
- Concurrent access safety (multiple proxy tasks updating stats simultaneously)

### Formatting
- Human-readable byte formatting: `0 → "0 B"`, `1023 → "1023 B"`, `1024 → "1.0 KB"`, `1536 → "1.5 KB"`, etc.
- Uptime formatting: seconds → `"2h 14m"`, `"3d 1h"`, etc.

### Proxy Relay
- Bidirectional copy between two in-memory `AsyncRead + AsyncWrite` streams
- Verify bytes are forwarded correctly in both directions
- Verify byte counts match what was sent
- Verify connection count increments on connect, decrements on disconnect
- Behavior when one side closes (half-close propagation)
- Behavior when one side errors (other side gets cleaned up)

## Integration Tests

These test the full tunnel lifecycle against a real SSH server.

### Test Environment Setup

The CI environment bootstraps a local sshd and a TCP echo server:

1. **sshd** on a non-standard port (e.g., 2222)
   - Configured with `PubkeyAuthentication yes`, `PasswordAuthentication no`
   - Uses ephemeral host keys and authorized keys generated per test run
   - Allows TCP forwarding (`AllowTcpForwarding yes`)

2. **TCP echo server** listening on a port (e.g., 9999) bound to localhost
   - Echoes back whatever it receives — simple validation target
   - Could also use a small HTTP server for richer request/response testing

3. **Ephemeral SSH key pair** generated with `ssh-keygen -t ed25519 -N "" -f /tmp/test_key`

### Test Cases

#### Basic Tunnel Lifecycle
1. Create a tunnel config: `localhost:15432 → localhost:9999` via sshd on port 2222
2. Start the tunnel
3. Verify the local port is listening
4. Connect to `localhost:15432`, send `"hello"`, receive `"hello"` back
5. Stop the tunnel
6. Verify the local port is no longer listening

#### Stats Accuracy
1. Start a tunnel
2. Send a known payload (e.g., 4096 bytes) through the tunnel
3. Wait for the echo response
4. Assert uploaded bytes ≥ 4096 (may include TCP framing overhead from proxy perspective, but the app-level byte count should be exact)
5. Assert downloaded bytes ≥ 4096
6. Assert total connections handled = 1
7. Assert current open connections = 0 (after closing)

#### Multiple Concurrent Connections
1. Start a tunnel
2. Open 5 connections simultaneously
3. Assert current open connections = 5
4. Send/receive data on all 5
5. Close 3 of them
6. Assert current open connections = 2
7. Assert total handled = 5
8. Close remaining 2
9. Assert current open connections = 0

#### Auto-Reconnect
1. Start a tunnel with auto-reconnect enabled
2. Verify tunnel is working (send/receive data)
3. Kill the `ssh` child process (simulate connection drop)
4. Wait for reconnect (poll with timeout)
5. Verify tunnel is working again
6. Stats should have reset (new session) and "last reconnect" should be set

#### SSH Process Failure
1. Start a tunnel pointed at a non-existent SSH server
2. Verify the tunnel enters an error state (not "running")
3. Verify the local proxy port rejects/refuses connections gracefully
4. Verify the tray state reflects the tunnel is down

#### Config Edit While Running
1. Start a tunnel on local port 15432
2. Edit the config to change local port to 15433
3. Trigger restart
4. Verify old port (15432) is no longer listening
5. Verify new port (15433) is working

## GitHub Actions CI

### Feasibility

Fully feasible on **Ubuntu runners**. Everything needed is available in the default image or via `apt`:

- `openssh-server` — for sshd
- `openssh-client` — for the `ssh` binary Bad Adit spawns
- Rust toolchain — via `actions-rust-lang/setup-rust-toolchain`

macOS runners also work but are slower and more expensive. Use Linux for CI, macOS for release builds only.

### Workflow Sketch

```yaml
name: CI
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rust-lang/setup-rust-toolchain@v1

      - name: Set up test SSH environment
        run: |
          # Generate ephemeral keys
          ssh-keygen -t ed25519 -N "" -f /tmp/test_key
          mkdir -p ~/.ssh
          cat /tmp/test_key.pub >> ~/.ssh/authorized_keys
          chmod 600 ~/.ssh/authorized_keys

          # Configure and start sshd on port 2222
          sudo tee /etc/ssh/sshd_test_config <<SSHEOF
          Port 2222
          ListenAddress 127.0.0.1
          HostKey /etc/ssh/ssh_host_ed25519_key
          PubkeyAuthentication yes
          PasswordAuthentication no
          AllowTcpForwarding yes
          SSHEOF
          sudo /usr/sbin/sshd -f /etc/ssh/sshd_test_config

          # Start a TCP echo server (using socat or a small Rust binary)
          socat TCP-LISTEN:9999,fork,reuseaddr EXEC:cat &

      - name: Run unit tests
        run: cargo test --lib

      - name: Run integration tests
        env:
          TEST_SSH_PORT: "2222"
          TEST_SSH_KEY: "/tmp/test_key"
          TEST_ECHO_PORT: "9999"
        run: cargo test --test integration

  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          components: clippy, rustfmt
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
```

### Test Binary vs socat

The echo server can be `socat TCP-LISTEN:9999,fork EXEC:cat` for simplicity. For more control (simulating slow responses, dropped connections, large payloads), write a small Rust echo server in `tests/fixtures/` and build it as part of the test harness.

## What We Don't Test in CI

- **macOS tray icon / system integration** — requires a display server; test manually or with a separate macOS workflow for smoke tests
- **Tauri frontend** — the webview UI should have its own test story (e.g., Playwright or similar); this document focuses on the Rust engine
- **Password/passphrase prompting** — CI uses passwordless keys; passphrase handling is tested manually or with a PTY mock in unit tests
