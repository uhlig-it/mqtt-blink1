# MQTT Blink1 Service

Controls a Blink1 via MQTT

# TODO

- Integrate with Home Assistant as light (using [auto discovery](https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery))

# Commands

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

Tagged releases are built by GitHub Actions: push a `v*` tag and the [Release workflow](.github/workflows/release.yml) cross-compiles the binary for the Raspberry Pi ('shop': 32-bit armv7) plus the other targets and attaches them to a GitHub release.

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

# Device access

The service runs as the unprivileged `mqtt-blink1` user, so it needs the udev rule in `files/51-blink1.rules` (installed to `/etc/udev/rules.d/51-blink1.rules` by the playbook) to open the Blink1's USB device.

On 2026-09-24 that rule was found missing on `shop`, and the service failed with `Error: DeviceListError(Access)` on the first command after a reboot. udev only (re)applies permissions when a device is enumerated, so the already-created device node kept its permissive mode until the next reboot — the failure surfaced long after the rule disappeared. Redeploying the rule and re-triggering the device fixed it. If the service logs `DeviceListError(Access)`, check that the rule file exists and that the Blink1's `/dev/bus/usb/*/*` node is world-writable.

Since v1.0.4 the service probes the device at startup (a no-op write, which also resets the LED to off) and exits with a clear error if it is unreachable, so a missing rule surfaces immediately instead of on the first command.

The unit's `RestrictAddressFamilies` must include `AF_NETLINK`: libusb opens a netlink socket for its udev-based hotplug monitor and fails to initialise without it, which the service reports as `unable to find device`.

# FAQ

## Why Rust?

I used to run this in Ruby, but the library was never updated for Ruby 3.x. I could not fix it myself because I do not know enough about handling the native code parts. I tried go, but the cross-compilation incl. CGO gave me headaches, too. Finally, the Rust library worked totally fine.
