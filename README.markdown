# MQTT Blink1 Service

Controls a Blink1 via MQTT

# TODO

- Integrate with Home Assistant as light (using [auto discovery](https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery))

# Configuration

There is no config file. Everything is taken from the environment or the command line:

| Setting | Source | Default |
| --- | --- | --- |
| Broker URL, including credentials | `MQTT_URL` (environment) | required |
| Command topic | `--command-topic` / `-t` | `werkstatt/blink1/cmnd` |
| Status topic | `--status-topic` / `-s` | `werkstatt/blink1/status` |
| MQTT client ID | `--client-id` | `mqtt-blink1-<broker host>` |

`MQTT_URL` is deliberately environment-only: a URL may carry `user:password`, which should not show up in `ps`. Supported schemes are `mqtt://`, `tcp://`, `ws://` and `wss://`; `mqtts://`, `ssl://` and `tls://` are rejected with an error because the binary is built without TLS. See `./mqtt-blink1 --help` for the flags.

# Commands

The examples below publish to the broker over TLS; the service itself has no TLS support yet (see [Configuration](#configuration)) and must reach the broker via `mqtt://`, `tcp://`, `ws://` or `wss://`.

## color

```command
$ echo '{
  "color": {
    "r": 0,
    "g": 0,
    "b": 255
  }
}' | mosquitto_pub \
  --url 'mqtts://user:pass@mqtt.example.com:8883/werkstatt/blink1/cmnd' \
  --stdin-file
```

## blink

```command
$ echo '{
  "blink": {
    "interval_ms": 80,
    "count": 3,
    "color": {
      "r": 0,
      "g": 0,
      "b": 255
    }
  }
}' | mosquitto_pub \
  --url 'mqtts://user:pass@mqtt.example.com:8883/werkstatt/blink1/cmnd' \
  --stdin-file
```

# Build

For local use, `cargo build --release` (or `cargo run`) builds on Linux and macOS. libusb is taken from the system when `pkg-config` finds it and built from the vendored sources otherwise (as on macOS), so no extra package is needed.

Tagged releases are built by GitHub Actions: push a `v*` tag and the [Release workflow](.github/workflows/release.yml) cross-compiles the binary for the Raspberry Pi ('shop': 32-bit armv7) plus the other targets and attaches them to a GitHub release.

# Running

The service needs a Blink1 on a USB port, `$MQTT_URL` set, and permission to talk to the USB device. Only the last part differs between platforms. Since v1.0.4 the service probes the device at startup (a no-op write, which also resets the LED to off) and exits with a clear error if it is unreachable, so a missing permission surfaces immediately instead of on the first command.

## Linux

Linux is the deployment target: the service runs as the unprivileged `mqtt-blink1` user, so it needs a udev rule that grants access to the Blink1's USB IDs (`27b8:01ed`). Without it, startup fails with `Error: unable to access the Blink1 device: DeviceListError(Access)`.

The deployment — systemd unit, udev rule and version pin — lives in [uhlig-it/infrastructure](https://github.com/uhlig-it/infrastructure), in `playbooks/services/mqtt-blink1.yml`. The former `playbook.yml` and `files/51-blink1.rules` in this repo moved there.

To check a host by hand:

```sh
lsusb -d 27b8:01ed                       # is the Blink1 enumerated?
cat /etc/udev/rules.d/51-blink1.rules    # is the rule installed?
sudo udevadm control --reload-rules
sudo udevadm trigger --action=add --subsystem-match=usb
```

udev only (re)applies permissions when a device is enumerated, so unplug and replug the Blink1 after installing or changing the rule, then restart the service (`sudo systemctl restart mqtt-blink1`). The rule makes the Blink1's `/dev/bus/usb/*/*` node world-writable (`0666`); if it is not, the service user cannot open it. On 2026-09-24 the rule was found missing on `shop`, and the failure surfaced only after a reboot: the already-created device node kept its permissive mode until the device was enumerated again.

The unit's `RestrictAddressFamilies` must include `AF_NETLINK`: libusb opens a netlink socket for its udev-based hotplug monitor and fails to initialise without it, which the service reports as `unable to find device`.

## macOS

macOS has no udev, so there are no udev rules to install. The Blink1 is a HID device, and libusb's macOS backend can only capture it (by detaching macOS's HID kernel driver) as root or with the `com.apple.vm.device-access` entitlement, which is meant for VM host applications; without either, the first USB access fails with `LIBUSB_ERROR_ACCESS`, which the service prints as `DeviceListError(Access)`. Everything else — building, the binary, `$MQTT_URL` — works as on Linux.

Check that macOS enumerates the Blink1:

```sh
system_profiler SPUSBDataType | grep -A 8 -i blink
```

Then run the binary as root:

```sh
sudo MQTT_URL='mqtt://user:pass@mqtt.example.com:1883' ./target/release/mqtt-blink1
```

For a persistent setup, run it as a root LaunchDaemon, e.g. `/Library/LaunchDaemons/net.uhlig-it.mqtt-blink1.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>net.uhlig-it.mqtt-blink1</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/sh</string>
    <string>-c</string>
    <string>MQTT_URL=$(cat /etc/mqtt-blink1.url) exec /usr/local/bin/mqtt-blink1</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
</dict>
</plist>
```

The plist is world-readable, so keep the URL and its credentials in a root-only file such as `/etc/mqtt-blink1.url` (`chmod 600`, owned by `root:wheel`) instead of in `EnvironmentVariables`. Load the daemon with:

```sh
sudo launchctl bootstrap system /Library/LaunchDaemons/net.uhlig-it.mqtt-blink1.plist
```

# Tests

Unit tests cover the MQTT command parsing (`src/blink1.rs`) and the stability logic in `src/main.rs` (reconnect backoff, the re-subscribe helper, and non-fatal message handling). They need neither a broker nor a Blink1 device; run them with:

```sh
cargo test
```

The full validation suite — also run by [CI](.github/workflows/ci.yml) on every push and pull request to `main`:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

# FAQ

## Why Rust?

I used to run this in Ruby, but the library was never updated for Ruby 3.x. I could not fix it myself because I do not know enough about handling the native code parts. I tried go, but the cross-compilation incl. CGO gave me headaches, too. Finally, the Rust library worked totally fine.
