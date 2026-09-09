# MQTT Blink1 Service

Controls a Blink1 via MQTT

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

# Development

For fast iteration, rsync the source code to a Raspberry Pi (where this is to be running eventually) and execute there:

1. Once:

    ```command
    $ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    ```

1. Iterate

    ```command
    $ cargo watch -- zsh -c 'rsync --progress --exclude-from=.rsyncignore -r -e ssh ./ pi5:workspace/mqtt-blink1/ \
      && ssh pi5 "cd workspace/mqtt-blink1; killall mqtt-blink1; cargo run"'
    ```

# Build

Tagged releases are built by GitHub Actions: push a `v*` tag and the [Release workflow](.github/workflows/release.yml) cross-compiles the binary for the Raspberry Pi ('shop': 32-bit armv7) plus the other targets and attaches them to a GitHub release.

To build manually on a Pi (32-bit armv7):

```command
$ rsync --progress --exclude-from=.rsyncignore -r -e ssh ./ <host>:workspace/mqtt-blink1/ \
    && ssh <host> "cd workspace/mqtt-blink1; cargo build --release" \
    && scp <host>:workspace/mqtt-blink1/target/release/mqtt-blink1 .
```

The Ansible playbook will then copy the binary to the target machine.

# TODO

- allow overriding the client id
- find a simpler way to create the udev rules (see `go` branch)
- Integrate with Home Assistant as light (using [auto discovery](https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery))

# FAQ

## Why Rust?

I used to run this in Ruby, but the library was never updated for Ruby 3.x. I could not fix it myself because I do not know enough about handling the native code parts. I tried go, but the cross-compilation incl. CGO gave me headaches, too. Finally, the Rust library worked totally fine.
