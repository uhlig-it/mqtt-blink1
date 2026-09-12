use blinkrs::{Blinkers, Message};
use clap::Parser;
use mqtt::QOS_0;
use paho_mqtt as mqtt;
use serde_json::Result as SerdeJsonResult;
use std::boxed::Box;
use std::time;
use std::{env, process, thread, time::Duration};
use url::Url;

pub mod blink1;

// Keep alive interval for the client session
const MQTT_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(20);

// Duration that needs to elapse before an attempt to reconnect will be made
const MQTT_RECONNECT_INTERVAL: Duration = Duration::from_millis(1000);

// Upper bound for the exponential reconnect backoff (§2.2)
const MQTT_RECONNECT_MAX_INTERVAL: Duration = Duration::from_secs(64);

#[derive(Parser, Debug)]
#[clap(
    author,
    version,
    about,
    help_template = "\
{before-help}{name} v.{version}

{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}

Author: {author-with-newline}
"
)]
pub struct Args {
    /// Topic where the device expects commands on
    #[arg(short('t'), long, default_value = "werkstatt/blink1/cmnd")]
    command_topic: String,

    /// Topic where the device publishes status changes on
    #[arg(short('s'), long, default_value = "werkstatt/blink1/status")]
    status_topic: String,

    /// MQTT client ID; defaults to "mqtt-blink1-<broker host>"
    #[arg(long)]
    client_id: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options: Args = Args::parse();

    let blink1: Blinkers = match Blinkers::new() {
        Ok(b) => b,
        Err(_e) => {
            println!("unable to find device");
            return Ok(());
        }
    };

    let urlstr = env::var("MQTT_URL").unwrap_or_else(|e| {
        eprintln!("Error fetching the MQTT_URL: {:?}", e);
        process::exit(1);
    });

    let url = Url::parse(&urlstr).unwrap_or_else(|e| {
        if urlstr.is_empty() {
            eprintln!("Error: $MQTT_URL not set");
        } else {
            eprintln!("Error: unable to parse the $MQTT_URL: {:?}", e);
        }
        process::exit(1);
    });

    // Preserve the scheme and the explicit port from $MQTT_URL (§3.1): the
    // old code silently dropped both and always connected to tcp://1883.
    // TLS schemes fail loudly instead of downgrading to cleartext — the `ssl`
    // feature is not compiled in, so the connection could never work anyway.
    let scheme = url.scheme();
    let transport = match scheme {
        "mqtt" | "tcp" => "tcp",
        "ws" => "ws",
        "wss" => "wss",
        "mqtts" | "ssl" | "tls" => {
            // TLS is not compiled in and is out of scope for this pass;
            // revisit this arm when TLS is added.
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

    // `url.host_str()` returns IPv6 literals without brackets; paho needs
    // them re-bracketed in the URI.
    let host_uri = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let server_uri = match url.port() {
        Some(port) => format!("{transport}://{host_uri}:{port}"),
        None => format!("{transport}://{host_uri}"),
    };

    // A per-broker client ID instead of the constant program name: the old
    // ID made every instance kick the others off the broker (§3.3).
    let client_id = match options.client_id {
        Some(id) => id,
        None => default_client_id(host),
    };

    let create_options = mqtt::CreateOptionsBuilder::new()
        .server_uri(server_uri)
        .client_id(client_id)
        .finalize();

    let mut conn_opts = mqtt::ConnectOptionsBuilder::new();
    conn_opts.keep_alive_interval(MQTT_KEEP_ALIVE_INTERVAL);

    match url.username().len() {
        0 => {}
        _ => {
            eprintln!("Setting username to {}", url.username());
            conn_opts.user_name(url.username());
        }
    }

    if let Some(password) = url.password() {
        eprintln!("Setting password (masked)");
        conn_opts.password(password);
    }

    let client = mqtt::Client::new(create_options).unwrap_or_else(|e| {
        eprintln!("Error creating the client: {:?}", e);
        process::exit(1);
    });

    let rx = client.start_consuming();

    match client.connect(conn_opts.finalize()) {
        Ok(rsp) => {
            if let Some(conn_rsp) = rsp.connect_response() {
                eprintln!(
                    "Connected to: '{}' with MQTT version {}",
                    conn_rsp.server_uri, conn_rsp.mqtt_version
                );
            }

            // Subscribe unconditionally: every connect creates a fresh
            // session (clean_session defaults to true), so the previous
            // subscription is gone — even on reconnect (§2.1).
            if !subscribe_command_topic(&client, &options.command_topic) {
                client.disconnect(None).unwrap();
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error connecting to the broker: {:?}", e);
            process::exit(1);
        }
    }

    for msg in rx.iter() {
        if let Some(msg) = msg {
            let payload_str = msg.payload_str();

            let result: SerdeJsonResult<blink1::Command> = serde_json::from_str(&payload_str);

            match result {
                Ok(cmd) => {
                    // Errors from the USB device or the status publish are
                    // logged and the loop continues instead of aborting the
                    // service (§2.3).
                    if let Err(e) = handle_command(
                        &|m| blink1.send(m).map(|_| ()).map_err(|e| e.to_string()),
                        &|topic, color| publish_status(&client, topic.to_string(), color),
                        &options.status_topic,
                        &cmd,
                    ) {
                        eprintln!("{e}");
                    }
                }
                Err(e) => {
                    eprintln!("Unable to parse message '{}': {}", payload_str, e);
                }
            }
        } else {
            reconnect_forever(&client, &options.command_topic);
        }
    }

    // The message stream ends when the connection is lost — at which point
    // `rx.iter()` has already yielded `None` and §2.2's reconnect loop takes
    // over — so this section is mostly defensive (§3.4). Collapsed from two
    // dead blocks; log failed calls instead of panicking.
    if client.is_connected() {
        println!("\nDisconnecting...");
        if let Err(e) = client.unsubscribe(&options.command_topic) {
            eprintln!(
                "Error unsubscribing from '{}': {e:?}",
                options.command_topic
            );
        }
        if let Err(e) = client.disconnect(None) {
            eprintln!("Error disconnecting: {e:?}");
        }
    }

    println!("Cleaning up...");
    blink1.send(Message::Immediate(blinkrs::Color::Three(0, 0, 0), None))?;
    client.stop_consuming();

    Ok(())
}

/// Reconnects forever with capped exponential backoff (§2.2). On success,
/// re-subscribes — the broker drops the subscription when the connection
/// ends (§2.1) — and returns so the message loop can resume.
fn reconnect_forever(cli: &mqtt::Client, topic: &str) {
    for delay in ReconnectSchedule::new(MQTT_RECONNECT_INTERVAL, MQTT_RECONNECT_MAX_INTERVAL) {
        thread::sleep(delay);
        if cli.reconnect().is_ok() {
            eprintln!("Successfully reconnected");
            subscribe_command_topic(cli, topic);
            return;
        }
        eprintln!("Reconnect failed; retrying in {delay:?}");
    }
}

/// Subscribes to `topic` with QOS_0. Returns true on success. Called after
/// every successful (re)connect: with `clean_session` (the default) the
/// broker drops the subscription when the connection ends, so a reconnect
/// would otherwise leave the service connected but deaf (§2.1).
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

/// Infinite iterator of reconnect delays: starts at `initial` and doubles on
/// every step up to `max`, then stays at `max` forever. The service must
/// never stop retrying — the old `try_reconnect` gave up after 12 tries
/// (§2.2).
struct ReconnectSchedule {
    delay: Duration,
    max: Duration,
}

impl ReconnectSchedule {
    fn new(initial: Duration, max: Duration) -> Self {
        Self {
            delay: std::cmp::min(initial, max),
            max,
        }
    }
}

impl Iterator for ReconnectSchedule {
    type Item = Duration;

    fn next(&mut self) -> Option<Duration> {
        let current = self.delay;
        self.delay = std::cmp::min(self.delay * 2, self.max);
        Some(current)
    }
}

/// Executes a parsed `Command`. USB (`send`) and status-publish errors are
/// returned to the caller, which logs them and continues — they must never
/// abort the service or leave the blink loop running blindly (§2.3). If the
/// USB write itself failed, the LED's actual color is unknown; the next
/// command will reset it.
fn handle_command(
    send: &dyn Fn(blinkrs::Message) -> Result<(), String>,
    publish: &dyn Fn(&str, &blink1::Color) -> Result<(), String>,
    status_topic: &str,
    cmd: &blink1::Command,
) -> Result<(), String> {
    let neutral = blinkrs::Color::Three(0, 0, 0);
    let neutral_c = blink1::Color { r: 0, g: 0, b: 0 };

    match cmd {
        blink1::Command::Blink { blink } => {
            // Reject out-of-range parameters instead of executing them
            // (§3.2): the caller logs the error and ignores the message.
            blink.validate()?;

            let interval = time::Duration::from_millis(blink.interval_ms);
            let color = blinkrs::Color::Three(blink.color.r, blink.color.g, blink.color.b);

            for _ in 0..blink.count {
                send(Message::Immediate(color, None))
                    .map_err(|e| format!("Error sending blink color: {e}"))?;
                publish(status_topic, &blink.color)
                    .map_err(|e| format!("Error publishing status: {e}"))?;
                thread::sleep(interval);
                send(Message::Immediate(neutral, None))
                    .map_err(|e| format!("Error sending neutral color: {e}"))?;
                publish(status_topic, &neutral_c)
                    .map_err(|e| format!("Error publishing status: {e}"))?;
                thread::sleep(interval);
            }
            Ok(())
        }
        blink1::Command::Color { color } => {
            send(Message::Immediate(
                blinkrs::Color::Three(color.r, color.g, color.b),
                None,
            ))
            .map_err(|e| format!("Error sending color: {e}"))?;

            publish(status_topic, color).map_err(|e| format!("Error publishing status: {e}"))?;
            Ok(())
        }
    }
}

/// Derives the default MQTT client ID from the broker host (§3.3), truncated
/// to the MQTT 3.1.1 23-char client-ID limit.
fn default_client_id(host: &str) -> String {
    const PREFIX: &str = "mqtt-blink1-";
    const MAX_LEN: usize = 23;

    let mut id = String::with_capacity(PREFIX.len() + host.len().min(MAX_LEN - PREFIX.len()));
    id.push_str(PREFIX);
    for ch in host.chars() {
        if id.len() + ch.len_utf8() > MAX_LEN {
            break;
        }
        id.push(ch);
    }
    id
}

fn publish_status(client: &mqtt::Client, t: String, color: &blink1::Color) -> Result<(), String> {
    let result = serde_json::to_string(&color);

    match result {
        Ok(m) => {
            let msg = mqtt::MessageBuilder::new().topic(t).payload(m).finalize();

            let result = client.publish(msg);

            match result {
                Ok(o) => Ok(o),
                Err(e) => Err(e.to_string()),
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // --- §2.2: reconnect forever with capped exponential backoff ---

    #[test]
    fn reconnect_schedule_starts_at_initial_delay() {
        let mut schedule = ReconnectSchedule::new(Duration::from_secs(1), Duration::from_secs(64));

        assert_eq!(schedule.next(), Some(Duration::from_secs(1)));
    }

    #[test]
    fn reconnect_schedule_doubles_until_cap() {
        let mut schedule =
            ReconnectSchedule::new(Duration::from_millis(500), Duration::from_secs(4));

        assert_eq!(schedule.next(), Some(Duration::from_millis(500)));
        assert_eq!(schedule.next(), Some(Duration::from_secs(1)));
        assert_eq!(schedule.next(), Some(Duration::from_secs(2)));
        assert_eq!(schedule.next(), Some(Duration::from_secs(4)));
        // Capped: never exceeds the maximum, no matter how long it runs. The
        // old `try_reconnect` gave up after 12 attempts instead.
        assert_eq!(schedule.next(), Some(Duration::from_secs(4)));
        assert_eq!(schedule.next(), Some(Duration::from_secs(4)));
    }

    #[test]
    fn reconnect_schedule_never_exceeds_cap() {
        let schedule = ReconnectSchedule::new(MQTT_RECONNECT_INTERVAL, MQTT_RECONNECT_MAX_INTERVAL);

        let delays: Vec<Duration> = schedule.take(100).collect();

        assert_eq!(delays[0], MQTT_RECONNECT_INTERVAL);
        assert_eq!(delays[6], MQTT_RECONNECT_MAX_INTERVAL);
        assert!(delays.iter().all(|d| *d <= MQTT_RECONNECT_MAX_INTERVAL));
        // It is infinite: the service must never stop retrying.
        assert_eq!(delays.len(), 100);
    }

    // --- §2.1: re-subscribe after every successful (re)connect ---

    #[test]
    fn subscribe_command_topic_fails_without_connection() {
        let client = mqtt::Client::new(
            mqtt::CreateOptionsBuilder::new()
                .server_uri("tcp://127.0.0.1:1")
                .client_id("unittest")
                .finalize(),
        )
        .unwrap();

        // An unconnected client cannot subscribe: the helper must report the
        // failure instead of panicking or exiting the process.
        assert!(!subscribe_command_topic(&client, "unittest/topic"));
    }

    // --- §3.2: blink parameter caps ---

    #[test]
    fn handle_command_blink_rejects_out_of_range_parameters() {
        let calls = RefCell::new(0u32);
        let send = |_: blinkrs::Message| -> Result<(), String> {
            *calls.borrow_mut() += 1;
            Ok(())
        };
        let publish = |_: &str, _: &blink1::Color| -> Result<(), String> { Ok(()) };

        let cmd = blink1::Command::Blink {
            blink: blink1::Blink {
                interval_ms: 0,   // below MIN_BLINK_INTERVAL_MS
                count: 1_000_000, // above MAX_BLINK_COUNT
                color: blink1::Color { r: 1, g: 1, b: 1 },
            },
        };

        let result = handle_command(&send, &publish, "t", &cmd);
        // A rejected blink must not execute at all (§3.2): the error is
        // logged by the caller and the message is ignored.
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("maximum"));
        assert_eq!(*calls.borrow(), 0);
    }

    // --- §3.3: unique client ID ---

    #[test]
    fn default_client_id_uses_broker_host() {
        assert_eq!(default_client_id("shop"), "mqtt-blink1-shop");
    }

    #[test]
    fn default_client_id_truncates_to_mqtt_limit() {
        // MQTT 3.1.1 caps client IDs at 23 chars; the prefix takes 12.
        let id = default_client_id("a-very-long-host-name.example.org");

        assert_eq!(id.len(), 23);
        assert!(id.starts_with("mqtt-blink1-"));
    }

    #[test]
    fn default_client_id_handles_ipv6_hosts() {
        assert_eq!(default_client_id("::1"), "mqtt-blink1-::1");
    }

    // --- §2.3: USB/status-publish errors must not abort the service ---

    #[test]
    fn handle_command_color_sends_and_publishes() {
        let sent: RefCell<Vec<blinkrs::Color>> = RefCell::new(Vec::new());
        let send = |msg: blinkrs::Message| -> Result<(), String> {
            match msg {
                blinkrs::Message::Immediate(color, None) => {
                    sent.borrow_mut().push(color);
                    Ok(())
                }
                other => Err(format!("unexpected message: {other:?}")),
            }
        };
        let published: RefCell<Vec<(String, u8)>> = RefCell::new(Vec::new());
        let publish = |topic: &str, color: &blink1::Color| -> Result<(), String> {
            published.borrow_mut().push((topic.to_string(), color.b));
            Ok(())
        };

        let cmd = blink1::Command::Color {
            color: blink1::Color {
                r: 10,
                g: 20,
                b: 30,
            },
        };

        assert!(handle_command(&send, &publish, "werkstatt/blink1/status", &cmd).is_ok());
        assert_eq!(
            sent.borrow().as_slice(),
            &[blinkrs::Color::Three(10, 20, 30)]
        );
        assert_eq!(
            published.borrow().as_slice(),
            &[("werkstatt/blink1/status".to_string(), 30u8)]
        );
    }

    #[test]
    fn handle_command_color_returns_err_on_send_failure() {
        let send = |_: blinkrs::Message| -> Result<(), String> { Err("usb gone".to_string()) };
        let publish = |_: &str, _: &blink1::Color| -> Result<(), String> { Ok(()) };

        let cmd = blink1::Command::Color {
            color: blink1::Color {
                r: 10,
                g: 20,
                b: 30,
            },
        };

        let result = handle_command(&send, &publish, "t", &cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("usb gone"));
    }

    #[test]
    fn handle_command_color_returns_err_on_publish_failure() {
        let send = |msg: blinkrs::Message| -> Result<(), String> {
            match msg {
                blinkrs::Message::Immediate(_, None) => Ok(()),
                other => Err(format!("unexpected message: {other:?}")),
            }
        };
        let publish =
            |_: &str, _: &blink1::Color| -> Result<(), String> { Err("broker down".to_string()) };

        let cmd = blink1::Command::Color {
            color: blink1::Color {
                r: 10,
                g: 20,
                b: 30,
            },
        };

        let result = handle_command(&send, &publish, "t", &cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("broker down"));
    }

    #[test]
    fn handle_command_blink_publishes_per_half_cycle() {
        let sent: RefCell<Vec<blinkrs::Color>> = RefCell::new(Vec::new());
        let send = |msg: blinkrs::Message| -> Result<(), String> {
            match msg {
                blinkrs::Message::Immediate(color, None) => {
                    sent.borrow_mut().push(color);
                    Ok(())
                }
                other => Err(format!("unexpected message: {other:?}")),
            }
        };
        let published: RefCell<Vec<(String, u8)>> = RefCell::new(Vec::new());
        let publish = |topic: &str, color: &blink1::Color| -> Result<(), String> {
            published.borrow_mut().push((topic.to_string(), color.b));
            Ok(())
        };

        let cmd = blink1::Command::Blink {
            blink: blink1::Blink {
                interval_ms: 10, // at MIN_BLINK_INTERVAL_MS (§3.2)
                count: 2,
                color: blink1::Color { r: 1, g: 2, b: 3 },
            },
        };

        assert!(handle_command(&send, &publish, "werkstatt/blink1/status", &cmd).is_ok());
        // count=2 blinks: color, neutral, color, neutral
        assert_eq!(
            sent.borrow().as_slice(),
            &[
                blinkrs::Color::Three(1, 2, 3),
                blinkrs::Color::Three(0, 0, 0),
                blinkrs::Color::Three(1, 2, 3),
                blinkrs::Color::Three(0, 0, 0),
            ]
        );
        assert_eq!(
            published.borrow().as_slice(),
            &[
                ("werkstatt/blink1/status".to_string(), 3u8),
                ("werkstatt/blink1/status".to_string(), 0u8),
                ("werkstatt/blink1/status".to_string(), 3u8),
                ("werkstatt/blink1/status".to_string(), 0u8),
            ]
        );
    }

    #[test]
    fn handle_command_blink_stops_on_send_failure() {
        let calls = RefCell::new(0u32);
        let send = |_: blinkrs::Message| -> Result<(), String> {
            *calls.borrow_mut() += 1;
            Err("usb gone".to_string())
        };
        let publish = |_: &str, _: &blink1::Color| -> Result<(), String> { Ok(()) };

        let cmd = blink1::Command::Blink {
            blink: blink1::Blink {
                interval_ms: 10, // at MIN_BLINK_INTERVAL_MS (§3.2)
                count: 100,      // at MAX_BLINK_COUNT (§3.2)
                color: blink1::Color { r: 1, g: 1, b: 1 },
            },
        };

        let result = handle_command(&send, &publish, "t", &cmd);
        // A dead USB device must stop this command, not the whole service:
        // the error is returned to the caller instead of being propagated.
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("usb gone"));
        assert_eq!(*calls.borrow(), 1);
    }
}
