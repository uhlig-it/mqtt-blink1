use blinkrs::{Blinkers, Message};
use clap::Parser;
use mqtt::QOS_0;
use paho_mqtt as mqtt;
use serde_json::Result as SerdeJsonResult;
use std::boxed::Box;
use std::io::Error;
use std::time;
use std::{env, process, thread, time::Duration};
use url::Url;

pub mod blink1;

// Keep alive interval for the client session
const MQTT_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(20);

// Duration that needs to elapse before an attempt to reconnect will be made
const MQTT_RECONNECT_INTERVAL: Duration = Duration::from_millis(1000);

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

    let hostname = url.host_str().expect("Error extracting the host");
    let progname = progname().expect("Error determining the program name");

    let create_options = mqtt::CreateOptionsBuilder::new()
        .server_uri(hostname)
        .client_id(progname)
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

                if conn_rsp.session_present {
                    eprintln!("Client session already present on broker.");
                } else {
                    match client.subscribe(&options.command_topic, QOS_0) {
                        Ok(_) => eprintln!("Subscribed"),
                        Err(e) => {
                            eprintln!("Error subscribing: {:?}", e);
                            client.disconnect(None).unwrap();
                            process::exit(1);
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error connecting to the broker: {:?}", e);
            process::exit(1);
        }
    }

    let neutral = blinkrs::Color::Three(0, 0, 0);
    let neutral_c = blink1::Color{r: 0, g: 0, b: 0};

    for msg in rx.iter() {
        if let Some(msg) = msg {
            let payload_str = msg.payload_str();

            let result: SerdeJsonResult<blink1::Command> = serde_json::from_str(&payload_str);

            match result {
                Ok(cmd) => match cmd {
                    blink1::Command::Blink { blink } => {
                        let interval = time::Duration::from_millis(blink.interval_ms);
                        let col =
                            blinkrs::Color::Three(blink.color.r, blink.color.g, blink.color.b);

                        for _ in 0..blink.count {
                            blink1.send(Message::Immediate(col, None))?;
                            publish_status(&client, options.status_topic.clone(), &blink.color)?;
                            thread::sleep(interval);
                            blink1.send(Message::Immediate(neutral, None))?;
                            publish_status(&client, options.status_topic.clone(), &neutral_c)?;
                            thread::sleep(interval);
                        }
                    }
                    blink1::Command::Color { color } => {
                        blink1.send(Message::Immediate(
                            blinkrs::Color::Three(color.r, color.g, color.b),
                            None,
                        ))?;

                        publish_status(&client, options.status_topic.clone(), &color)?
                    }
                },
                Err(e) => {
                    eprintln!("Unable to parse message '{}': {}", payload_str, e);
                }
            }
        } else if client.is_connected() || !try_reconnect(&client, MQTT_RECONNECT_INTERVAL) {
            break;
        }
    }

    if client.is_connected() {
        println!("\nDisconnecting...");
        client.disconnect(None).unwrap();
    }

    if client.is_connected() {
        client.unsubscribe(&options.command_topic).unwrap();
        client.disconnect(None).unwrap();
    }

    println!("Cleaning up...");
    blink1.send(Message::Immediate(blinkrs::Color::Three(0, 0, 0), None))?;
    client.stop_consuming();

    Ok(())
}

fn try_reconnect(cli: &mqtt::Client, reconnect_interval: Duration) -> bool {
    eprintln!("Connection lost. Waiting to retry connection");
    for _ in 0..12 {
        thread::sleep(reconnect_interval);
        if cli.reconnect().is_ok() {
            eprintln!("Successfully reconnected");
            return true;
        }
    }
    eprintln!("Unable to reconnect after several attempts.");
    false
}

#[derive(Debug)]
enum ProgError {
    NoFile,
    NotUtf8,
    Io(Error),
}

impl From<Error> for ProgError {
    fn from(err: Error) -> ProgError {
        ProgError::Io(err)
    }
}

// https://stackoverflow.com/a/36859137/3212907
fn progname() -> Result<String, ProgError> {
    Ok(env::current_exe()?
        .file_name()
        .ok_or(ProgError::NoFile)?
        .to_str()
        .ok_or(ProgError::NotUtf8)?
        .to_owned())
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
