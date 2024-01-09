# MQTT Blink1 Service

I could not get cross-compilation on the M2 to work yet. For now, compile and run on a Raspberry Pi:

```command
$ cargo build --release && rsync --progress -r -e ssh src/ kiosk:workspace/mqtt-blink1/src && ssh kiosk "cd workspace/mqtt-blink1 && cargo run"
```

# TODO

- implement
- find a simpler way to create the udev rules (see `go` branch)

# FAQ

## Why Rust?

I used to run this in Ruby, but the library was never updated for Ruby 3.x. I could not fix it myself because I do not know enough about handling the native code parts.

I tried go, but the cross-compilation inc. CGO gave me headaches, too.

Finally, the Rust library worked without an issue.
