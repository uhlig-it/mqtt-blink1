use std::boxed::Box;
use std::error::Error;
use std::{thread, time};

use blinkrs::{Blinkers, Color, Message};

fn main() -> Result<(), Box<dyn Error>> {
    let blinkers: Blinkers = match Blinkers::new() {
        Ok(b) => b,
        Err(_e) => {
            println!("unable to find device");
            return Ok(());
        }
    };

    let interval = time::Duration::from_millis(1);

    for r in (0..255).step_by(10) {
        for g in (0..255).step_by(10) {
            for b in (0..255).step_by(10) {
                blinkers.send(Message::Immediate(Color::Three(r, g, b), None))?;
                thread::sleep(interval);
                // blinkers.send(Message::from("off"))?;
                // thread::sleep(interval);
            }
        }
    }

    Ok(())
}
