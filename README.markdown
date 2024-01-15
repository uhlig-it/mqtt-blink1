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

# Patterns

Currently the `blinkrs` Rust API does not expose the internal pattern store of the blink1 device, so we re-implemented it in this program.

## `define`

```json
{
  "pattern": {
    "define": {
      "name": "American Police Car",
      "lines": [
        { "fade_ms": 100,  "color": { "r": 0, "g": 0, "b": 255 } },
        { "fade_ms": 100,  "color": { "r": 255, "g": 0, "b": 0 } },
      ]
    }
  }
}
```

There is no protection against overwriting an existing pattern.

## `play`

```json
{
  "pattern": {
    "play": {
      "patterns": [
        "American Police Car",
        "Glowing Rainbow",
      ],
      "repeat": 7 // if zero, play forever (until stop or any other command except `define`)
    }
  }
}
```

## `stop`

```json
{
  "pattern": {
    "stop": true
  }
}
```

# Iterate

Rust doesn't really cross-compile, so I rsync the source code to a Raspberry Pi (where this is to be running eventually) and execute there:

```command
$ cargo watch -- zsh -c 'rsync --progress --exclude-from=.rsyncignore -r -e ssh ./ kiosk:workspace/mqtt-blink1/ && ssh kiosk "cd workspace/mqtt-blink1; killall mqtt-blink1; cargo run"'
```

The compiling machine must have OpenSSL dev headers. Check the [paho-mqtt](https://github.com/eclipse/paho.mqtt.rust) project for details.

# TODO

- finish pattern implementation
- persist patterns across restarts
- allow overriding the client id
- find a simpler way to create the udev rules (see `go` branch)
- CI/CD pipeline
- Integrate with Home Assistant as light (using [auto discovery](https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery))

# FAQ

## Why Rust?

I used to run this in Ruby, but the library was never updated for Ruby 3.x. I could not fix it myself because I do not know enough about handling the native code parts. I tried go, but the cross-compilation incl. CGO gave me headaches, too. Finally, the Rust library worked totally fine.
