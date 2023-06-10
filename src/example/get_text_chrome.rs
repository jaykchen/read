use anyhow::{anyhow, Result};
use headless_chrome::{types::PrintToPdfOptions, Browser, LaunchOptions};
use html2text::from_read;
use pdfium_render::prelude::*;
use readah::readability::Readability;
use serde_json;
use std::time::Duration;
use tiktoken_rs::cl100k_base;
use url::Url;
use std::env;
use std::process::Command;
use clipboard::ClipboardContext;
use clipboard::ClipboardProvider;
use std::{thread, time};

// not completed
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let applescript = r#"
    tell application "System Events"
        keystroke "u" using command down
        delay 1
        keystroke "a" using command down
        delay 1
        keystroke "c" using command down
        delay 1
    end tell
    "#;

    let _ = Command::new("osascript")
        .arg("-e")
        .arg(applescript)
        .output()?;

    // Add an additional delay in the Rust code to give the system time to copy to clipboard
    thread::sleep(time::Duration::from_millis(300));

    // Create a new clipboard context
    let mut ctx: ClipboardContext = ClipboardProvider::new()?;

    // Get the content from the clipboard
    let clipboard_contents = ctx.get_contents()?;

    // Print the contents
    println!("{}", clipboard_contents);

    Ok(())
}

