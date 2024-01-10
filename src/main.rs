use std::boxed::Box;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::{thread, time};

use blinkrs::{Blinkers, Color, Message};

fn main() -> Result<(), Box<dyn Error>> {
    let blink1: Blinkers = match Blinkers::new() {
        Ok(b) => b,
        Err(_e) => {
            println!("unable to find device");
            return Ok(());
        }
    };

    let short_interval = time::Duration::from_millis(80);
    let long_interval = time::Duration::from_millis(500);

    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;

    while !term.load(Ordering::Relaxed) {
        for _ in 0..4 {
            blink1.send(Message::Immediate(Color::Three(0, 0, 255), None))?;
            thread::sleep(short_interval);
            blink1.send(Message::Immediate(Color::Three(0, 0, 0), None))?;
            thread::sleep(short_interval);
            blink1.send(Message::Immediate(Color::Three(255, 0, 0), None))?;
            thread::sleep(short_interval);
            blink1.send(Message::Immediate(Color::Three(0, 0, 0), None))?;
            thread::sleep(short_interval);
            blink1.send(Message::Immediate(Color::Three(255, 255, 0), None))?;
            thread::sleep(short_interval);
            blink1.send(Message::Immediate(Color::Three(0, 0, 0), None))?;
            thread::sleep(short_interval);
        }
        thread::sleep(long_interval);
    }

    println!("Cleaning up...");
    blink1.send(Message::Immediate(Color::Three(0, 0, 0), None))?;

    Ok(())
}
