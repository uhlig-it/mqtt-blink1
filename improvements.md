# mqtt-blink1 — Future Improvements Plan

Status: analysis complete; **P0 implemented 2026-09-12** (§2 below), **P1 implemented
2026-09-12** (commit `5feddc0`), **P2 implemented 2026-09-12** (§4 below), P3 still open. Scope decision (2026-09-12):
the broker is on the same host (`localhost:1883`), so **TLS enablement is deferred**
(§8); the URI-handling fix is kept as a correctness fix (§3.1); the stability
items were promoted to P0 (§2).
Repo root: `/Users/suhlig/git/github.com/uhlig.it/mqtt-blink1` (branch `main`, was 2 commits ahead of `origin/main`).

---

## 1. Context (read this first)

Small Rust service: subscribes to an MQTT command topic, drives a Blink1 USB LED
(`blinkrs`/libusb), publishes status. Deployed to a Raspberry Pi (`shop`, armv7,
Debian 12 / glibc 2.36) via Ansible + systemd (external role
`uhlig-it.simple_systemd_service`). The MQTT broker runs on the same host
(`localhost:1883`).

### Files map

| File | Role |
|---|---|
| `src/main.rs` | CLI args (clap), MQTT_URL parsing, connect/subscribe/reconnect loop, command dispatch |
| `src/blink1.rs` | `Command`/`Color`/`Blink` serde types + deserialization tests |
| `Cargo.toml` | deps: `blinkrs 2.0.1`, `clap`, `paho-mqtt 0.14` (default-features off, `bundled`), `serde`, `serde_json`, `signal-hook 0.4.0`, `url` |
| `.github/workflows/ci.yml` | fmt + clippy, tests (ubuntu/macos) |
| `.github/workflows/release.yml` | cross-builds 5 targets; armv7 uses glibc + `isoc23_shim.c` |
| `playbook.yml` | Ansible deploy; hardcodes binary URL `.../v1.0.1/mqtt-blink1-linux-armv7.tar.gz` |
| `inventory.yml`, `group_vars/all/secrets.yml` | host + vault-encrypted `mqtt_url` |
| `isoc23_shim.c` | `__isoc23_*` shims so the armv7 binary runs on glibc 2.36 |
| `.cargo/config.toml` | `CMAKE_POLICY_VERSION_MINIMUM=3.5` workaround for bundled paho.mqtt.c + CMake ≥ 4 |

### Baseline checks (all pass at analysis time)

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test                          # 2 tests, both pass
cargo audit                         # no advisories
```

### Verified facts (from the vendored paho.mqtt.c in `~/.cargo/registry/src/.../paho-mqtt-sys-0.11.0/`)

- paho-mqtt's default features are `["bundled", "ssl"]`; this project disables
  defaults and enables only `bundled`, so **OpenSSL/TLS is compiled out**
  (`MQTTCLIENT_SSL_NOT_SUPPORTED` would be returned for `ssl://` URIs). This is
  fine for the localhost broker and **stays that way in this pass** — §3.1 makes
  `mqtts://` fail loudly instead of silently downgrading to cleartext.
- paho parses the server URI in `MQTTClient.c`/`MQTTProtocolOut.c`: a bare
  hostname with no scheme is treated as plain `tcp://` on `MQTT_DEFAULT_PORT`
  (1883). Any port in the URL is therefore silently dropped (see §3.1).
- The bundled C library has **no auto-resubscribe logic** (grep for
  `resubscribe` in `paho.mqtt.c/src` is empty), and the Rust connect options
  default to `cleansession = true` (C initializer). After a manual
  `client.reconnect()`, the broker creates a fresh session → the command-topic
  subscription is gone and the app never re-subscribes: connected but deaf (§2.1).

⚠️ If `paho-mqtt`/`paho-mqtt-sys` is bumped, re-verify items above against the
new vendored source before relying on them.

---

## 2. Priority P0 — Stability

> Status: **implemented 2026-09-12** — all three items below. Unit tests live
> in `src/main.rs` (`ReconnectSchedule`, `subscribe_command_topic`,
> `handle_command`); manual reconnect validation from §7 still recommended.
> Note: a broker-less unit test cannot observe a real publish/subscribe round
> trip, so the re-subscribe-after-reconnect behaviour is pinned by the code
> structure (a single helper called from both connect paths) + the §7 manual
> check, rather than by an embedded-broker integration test (judged brittle).
>
> - [x] 2.1 Re-subscribe after every successful (re)connect
> - [x] 2.2 Reconnect forever with capped exponential backoff
> - [x] 2.3 Don't abort the service on USB/status-publish errors

These three bite hardest on the localhost deployment (broker restarts,
redeploys, USB hiccups) and are the reason to do this pass at all.

### 2.1 Re-subscribe after every successful (re)connect

`src/main.rs:191-202` + the `else if` at `main.rs:169`: with
`clean_session = true` (default), a successful `client.reconnect()` leaves the
service connected but with **no subscription** (see verified facts above). The
`session_present` guard at `main.rs:111` never skips either — a fresh session is
always created, so `session_present` is always false.

Fix: extract a `subscribe_command_topic(cli, topic)` helper and call it
**unconditionally after every successful connect** — initial and reconnect —
dropping the `session_present` guard:

```rust
fn subscribe_command_topic(cli: &mqtt::Client, topic: &str) -> bool {
    match cli.subscribe(topic, QOS_0) {
        Ok(_) => {
            eprintln!("Subscribed to '{topic}'");
            true
        }
        Err(e) => {
            eprintln!("Error subscribing to '{topic}': {e:?}");
            false
        }
    }
}
```

Call it (a) right after the initial `connect` succeeds (`main.rs:103-129`),
and (b) in the reconnect path after `cli.reconnect()` returns `Ok` (§2.2).

Alternative (also valid): switch to `clean_session(false)` + unique client ID
(§3.3) and rely on the broker's persistent session. Then the `session_present`
guard becomes meaningful. Choose the explicit re-subscribe approach unless
persistent-session semantics (queueing of offline messages) are wanted — with
the default clean session there is no queueing anyway.

### 2.2 Reconnect forever with capped exponential backoff

`try_reconnect` (`main.rs:191-202`) tries 12 × 1 s, then returns `false`, the
main loop breaks, and the process exits — even with exit code 0, so
`Restart=on-failure` (if configured) may not even restart it. A network/broker
blip longer than ~12 s kills the headless service.

Replace with an unbounded loop and capped backoff:

```rust
// 1s → 64s cap (adjust to taste)
let mut delay = MQTT_RECONNECT_INTERVAL;
loop {
    thread::sleep(delay);
    if cli.reconnect().is_ok() {
        eprintln!("Successfully reconnected");
        subscribe_command_topic(cli, &topic);   // §2.1
        return true;
    }
    eprintln!("Reconnect failed; retrying in {delay:?}");
    delay = std::cmp::min(delay * 2, MQTT_RECONNECT_MAX_INTERVAL);
}
```

Keep a log line per failed attempt (rate-limited by the backoff). After this
change the `else if ... || !try_reconnect(...) { break; }` exit path at
`main.rs:169` effectively never triggers on transient failures; that is the
intent.

Note: this loop lives in the main thread and a SIGTERM kills the process
anyway (systemd), so no signal handling is required. (The unused `signal-hook`
dep — see §4.1 — should NOT be resurrected for this.)

### 2.3 Don't abort the service on USB/status-publish errors

`main.rs:148-162`: `blink1.send(...)?` and `publish_status(...)?` propagate out
of `main`, skipping the LED-off cleanup and disconnect. One transient USB
hiccup leaves the LED stuck in its last color.

Move per-message handling into a function that returns a non-fatal result and
logs errors (`eprintln!`) instead of `?`-ing to `main`. Keep the final
LED-off/disconnect cleanup reachable only on genuine fatal errors (or on
shutdown). If the USB write itself failed, the LED's actual color is unknown —
log, continue, and let the next command reset it.

---

## 3. Priority P1 — Security & robustness

### 3.1 Preserve scheme + port in `server_uri` (correctness; TLS deliberately out of scope)

`src/main.rs:72-76` — `server_uri(hostname)` strips scheme and port. Two
consequences, both worth fixing even though the broker is on localhost:

1. **Explicit ports are silently dropped**: the client always connects to 1883
   regardless of `MQTT_URL`. Today's URL works by luck (`localhost:1883` is
   paho's default); the moment the port changes, the service silently connects
   to the wrong place.
2. **`mqtts://` silently downgrades to cleartext** `tcp://host:1883`. With the
   `ssl` feature compiled out (`Cargo.toml:17`) TLS can never work — make it
   fail loudly at parse time instead of running without anyone noticing.

Implementation — replace `main.rs:72-76`:

```rust
let scheme = match url.scheme() {
    "mqtt" | "tcp" => "tcp",
    "ws" => "ws",
    "wss" => "wss",
    "mqtts" | "ssl" | "tls" => {
        // TLS is not compiled in and is out of scope for this pass (§8).
        // Fail loudly instead of downgrading to cleartext; revisit this
        // arm when TLS is added.
        eprintln!("Error in $MQTT_URL: scheme '{scheme}' requires TLS, which is not enabled");
        process::exit(1);
    }
    other => {
        eprintln!("Unsupported scheme in $MQTT_URL: {other}");
        process::exit(1);
    }
};
let host = match url.host_str() {
    Some(h) => h,
    None => {
        eprintln!("Error in $MQTT_URL: no host given");
        process::exit(1);
    }
};
let server_uri = match url.port() {
    Some(port) => format!("{scheme}://{host}:{port}"),
    None => format!("{scheme}://{host}"),
};
// ... CreateOptionsBuilder::new().server_uri(server_uri) ...
```

Notes:

- IPv6 literal hosts: `url.host_str()` yields `::1` without brackets — re-bracket
  (`[::1]`) before handing the URI to paho.
- `ws`/`wss` are untested with the bundled build (paho C must be compiled with
  WebSocket support to serve them); today the scheme is stripped anyway, so ws
  URLs are already broken. Verify before relying on them.

Acceptance: `MQTT_URL=mqtt://user:pass@localhost:1883` connects on 1883; an
explicit non-default port is honored; `mqtts://…` aborts with a clear error
(no silent cleartext downgrade).

### 3.2 Cap blink parameters (defense in depth)

`src/blink1.rs:28-34`: `count: u64` unbounded, `interval_ms` may be 0. One
message with `count: u64::MAX` blocks the service indefinitely in a USB-write
busy loop (the blink loop runs synchronously in the consuming thread).

Add constants + validation (e.g. in a new `validate` fn or a
`#[serde(deserialize_with = "...")]` on `Blink`):

```rust
const MAX_BLINK_COUNT: u64 = 100;
const MIN_BLINK_INTERVAL_MS: u64 = 10;
```

Call it when processing `Command::Blink` in `main.rs`; on violation, log and
ignore the message instead of executing. Add unit tests for the validation
(boundary values) in `src/blink1.rs` (existing test module).

Note: `count` defaults to 0 (asserted by the existing `test_deserialize_blink`),
so a blink message without `count` is already a no-op — leave the default alone
while adding the caps.

### 3.3 Unique client ID (+ `--client-id` flag)

`main.rs:73,77`: `client_id(progname)` is the same (`mqtt-blink1`) for every
instance → two instances (overlapping deploy restarts) kick each other off the
broker in a reconnect war. Also closes the README TODO "allow overriding the
client id".

Implement:

- clap arg `#[arg(long, default_value = ...)] client_id: String` (README TODO),
- default derived from hostname: `format!("mqtt-blink1-{}", hostname)` —
  watch the MQTT 23-char client-id limit (truncate/sanitize; `shop` →
  `mqtt-blink1-shop` = 17 chars, fine).

This also removes the need for `progname()`/`ProgError` → see §4.3.

### 3.4 Remove dead disconnect code

`main.rs:174-182`: the second `if client.is_connected()` block is unreachable
(the first block already disconnects), so `client.unsubscribe(...)` never runs.
Also `disconnect(None).unwrap()` can panic.

Collapse into a single block; handle disconnect errors with `if let Err(e) = ...`
and log; unsubscribe first, then disconnect.

Related dead code: the `client.is_connected()` branch at `main.rs:169` never
triggers either — `rx.iter()` yields `None` precisely because the connection
was lost. Same treatment: log and let §2.2's reconnect loop decide.

---

## 4. Priority P2 — Cleanup & simplification

> Status: **implemented 2026-09-12** — all six items below. §4.4 diverges from the
> plan: `MQTT_URL` stays **env-only** (`std::env::var` + friendly error) and is
> deliberately *not* exposed via clap — neither `--mqtt-url` nor a positional —
> because the URL may carry `user:pass` credentials and must not be passable on
> the CLI (process listings). clap keeps only the `derive` feature. Status
> publishes use `retained(true)` (§4.6); mosquitto's default accepts retained
> messages.
>
> - [x] 4.1 Remove unused `signal-hook` dependency
> - [x] 4.2 Import cleanup (`src/main.rs:1-11`)
> - [x] 4.3 Delete `progname()` / `ProgError` machinery
> - [x] 4.4 Read `MQTT_URL` via clap's `env` feature — *reimplemented: env-only, see status above*
> - [x] 4.5 `publish_status` error type — *log-and-ignore, returns `()`*
> - [x] 4.6 Publish status with `retain(true)`

### 4.1 Remove unused `signal-hook` dependency

Declared in `Cargo.toml:20`, never imported in `src/` (confirmed by grep).
Remove it (smaller supply-chain surface, faster builds).

### 4.2 Import cleanup (`src/main.rs:1-11`)

- `use std::boxed::Box;` — in prelude, remove.
- `use std::time;` is redundant with `time::Duration` from the `use std::{...}` line.

### 4.3 Delete `progname()` / `ProgError` machinery

`main.rs:204-235` (~30 lines) exists only to produce the client ID. With §3.3 the
client ID comes from clap/hostname; remove the enum, its `Display`/`From`
impls, and the `progname` fn. Confirmed: `use std::fmt;` and
`use std::io::Error;` (`main.rs:7-8`) are used only by this machinery and become
unused too — remove them.

### 4.4 Read `MQTT_URL` via clap's `env` feature

> Result (2026-09-12): **not implemented as specified** — `MQTT_URL` stays
> env-only via `std::env::var` with a friendly error (see the Status block
> above for the reasoning). The clap snippet below is kept for reference only.

`main.rs:58-70`: the manual `env::var` + `process::exit` dance becomes:

```rust
// clap: features = ["derive", "env"] in Cargo.toml
#[arg(long, env = "MQTT_URL")]
mqtt_url: String,
```

Keep a friendly error message if it is missing (clap handles env-missing via
its own required-arg error; optionally set `required_unless`/help text).

### 4.5 `publish_status` error type

`main.rs:237-253`: returns `Result<(), String>` (stringly-typed). Either return
`paho_mqtt::Error` or log-and-ignore inside; with §2.3 the caller no longer
needs a `Result` at all.

### 4.6 Publish status with `retain(true)`

`main.rs:242`: `MessageBuilder::new().topic(t).payload(m).retained(true)` so
late subscribers / Home Assistant integrations (README TODO) see the current
color immediately. Confirm the broker is configured to accept retained messages
(mosquitto's default allows them). Note: during a blink the loop publishes
every half-cycle, ending with the neutral (off) color — so the retained state
will end up "off", which is the correct state for HA.

---

## 5. Priority P3 — Deployment & CI

### 5.1 Playbook: parameterize version + verify checksum

`playbook.yml:11` hardcodes the `v1.0.1` release URL (already stale relative to
`main`). Move the tag into a variable (`mqtt_blink1_version`) and add a SHA-256
checksum for the tarball if the sysd role supports it.

### 5.2 systemd hardening (if role allows extra unit options)

In `playbook.yml` (role `uhlig-it.simple_systemd_service` — repo is private,
inspect its vars first):

```ini
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
```

Credentials exposure: `MQTT_URL` (with `user:pass`) is set via `environment:`
and is readable by any local user through `systemctl show`/D-Bus. This is the
real credential exposure for the localhost broker (TLS would not fix it — see
§8). Prefer `EnvironmentFile=` (0600, root-owned) if the role supports it.

The runtime user also needs the hidraw udev rule for the Blink1 (README TODO).

### 5.3 CI hardening

- `ci.yml:2-5`: `on: push` + `pull_request` double-runs for same-repo PRs —
  restrict push to `main`/tags or add a `concurrency` group.
- Pin actions to commit SHAs (supply-chain) instead of major tags.
- Add `cargo audit` (or `cargo-deny`) as a CI job; it currently only runs as a
  local pre-commit hook.

### 5.4 (Optional) armv7-musl to delete `isoc23_shim.c`

The armv7 target uses glibc + `isoc23_shim.c` + a glibc-requirement check.
If a musl armv7 cross-toolchain works with the vendored libusb (the amd64/arm64
musl targets already do), switching would remove the shim and the
`Verify glibc requirement` step. Deliberate touch — only do this if the musl
armv7 build is proven.

---

## 6. Suggested commit breakdown

1. **Robustness**: 3.1 (URI preservation) + 4.1 (remove `signal-hook`).
2. **Stability**: 2.1 (re-subscribe), 2.2 (reconnect backoff), 2.3 (non-fatal
   errors), 3.3 (client ID), 3.4 (dead code).
3. **Validation**: 3.2 (blink caps) + unit tests.
4. **Cleanup**: 4.2-4.6.
5. **Deploy/CI**: 5.1-5.3.

Keep commits small and run the full validation checklist (below) per commit.

## 7. Validation checklist

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo audit
```

Manual, per change:

- URI: `MQTT_URL=mqtt://user:pass@localhost:1883` connects on 1883; an explicit
  non-default port is honored; `mqtts://…` fails fast with a clear message (no
  silent cleartext downgrade).
- Reconnect: connect, kill the broker/network for > 30 s, restore; confirm
  reconnection, **re-subscription**, and that a published command is then
  honored (previously the service reconnected but stayed deaf).
- Blink caps: publish `{"blink":{"count": 1000000, "interval_ms": 0, ...}}`;
  expect a logged rejection, not a hang.
- Two instances locally with the same topics → no client-id thrash (distinct
  IDs).

## 8. Out of scope / deferred (do not do in this pass)

- **TLS enablement** — the paho `ssl`/`vendored-ssl` feature (`Cargo.toml:17`),
  `SslOptions`/verification work, and the TLS acceptance tests. Deferred
  because the broker is on the same host (`localhost:1883`): loopback is not
  remotely sniffable, no MITM is possible on `localhost`, and the real
  credential exposure is the systemd environment (§5.2). Revisit if the broker
  ever moves off-box — note the armv7 cross-build cost (OpenSSL cross-compile)
  when doing so. Until then §3.1 guarantees `mqtts://` URLs fail loudly instead
  of downgrading to cleartext.
- Home Assistant MQTT discovery integration (README TODO) — 4.6 is a
  prerequisite.
- General udev rules for the Blink1 (README TODO).
- `edition = "2024"` bump.
- macOS/Linux behavioral parity work beyond what CI already covers.
