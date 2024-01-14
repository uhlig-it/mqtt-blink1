# MQTT Blink1 Service

Controls a Blink1 via MQTT

# Iterate

Rust doesn't really cross-compile, so I rsync the source code to a Raspberry Pi and execute there:

```command
$ cargo watch -- zsh -c 'rsync --progress --exclude-from=.rsyncignore -r -e ssh ./ kiosk:workspace/mqtt-blink1/ && ssh kiosk "cd workspace/mqtt-blink1; killall mqtt-blink1; cargo run"'
```

The compiling machine must have OpenSSL dev headers. Check the [paho-mqtt](https://github.com/eclipse/paho.mqtt.rust) project for details.

# TODO

- implement how to blink from message
- store and refer to patterns
- allow overriding the client id
- find a simpler way to create the udev rules (see `go` branch)

# FAQ

## Why Rust?

I used to run this in Ruby, but the library was never updated for Ruby 3.x. I could not fix it myself because I do not know enough about handling the native code parts. I tried go, but the cross-compilation inc. CGO gave me headaches, too. Finally, the Rust library worked totally fine.
