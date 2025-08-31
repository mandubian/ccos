use std::time::Duration;
use crossterm::event::{self, Event as CEvent, KeyCode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing crossterm event detection...");
    println!("Press any key (timeout in 5 seconds)...");

    // Enable raw mode
    crossterm::terminal::enable_raw_mode()?;

    let result = event::poll(Duration::from_secs(5));

    match result {
        Ok(true) => {
            println!("Event detected!");
            match event::read()? {
                CEvent::Key(key) => {
                    println!("Key pressed: {:?}", key.code);
                }
                other => {
                    println!("Other event: {:?}", other);
                }
            }
        }
        Ok(false) => {
            println!("No event detected within timeout");
        }
        Err(e) => {
            println!("Error polling for events: {:?}", e);
        }
    }

    // Disable raw mode
    crossterm::terminal::disable_raw_mode()?;
    println!("Test complete");

    Ok(())
}
