use std::boxed::Box;
use std::error::Error;

use blinkrs::{Blinkers, Message};

fn main() -> Result<(), Box<dyn Error>> {
    let blinkers: Blinkers = match Blinkers::new() {
        Ok(b) => b,
        Err(_e) => {
            println!("unable to find device");
            return Ok(())
        },
    };
    blinkers.send(Message::from("red"))?;
    blinkers.send(Message::from("off"))?;
    Ok(())
}
