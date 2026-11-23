//! A key test on a connected Stream Deck (deckhand, CuriousTimo's fork). Run: `cargo run --example key_test`.
//!
//! - prints the devices found, and the model, serial and firmware of the first one;
//! - gives every key its own colour with its number on it;
//! - a key turns white while it is held, and each press and release is printed;
//! - key 0 dims, key 1 brightens; the last key (bottom right) clears the deck and ends.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use elgato_streamdeck::{DeviceStateUpdate, StreamDeck, list_devices, new_hidapi};
use image::{DynamicImage, Rgb, RgbImage};

fn main() -> ExitCode {
    let hid = match new_hidapi() {
        Ok(hid) => hid,
        Err(e) => {
            eprintln!("hidapi: {e}");
            return ExitCode::FAILURE;
        }
    };

    let devices = list_devices(&hid);
    if devices.is_empty() {
        eprintln!("No Stream Deck found. Is it plugged in, and is the Elgato app closed?");
        return ExitCode::FAILURE;
    }
    for (kind, serial) in &devices {
        println!("found: {kind:?} (pid {:#06x}), serial {serial}", kind.product_id());
    }

    let (kind, serial) = devices[0].clone();
    let deck = match StreamDeck::connect(&hid, kind, &serial) {
        Ok(deck) => deck,
        Err(e) => {
            eprintln!("connect: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "connected: {kind:?}, {} keys ({} x {}), firmware {}",
        kind.key_count(),
        kind.column_count(),
        kind.row_count(),
        deck.firmware_version().unwrap_or_else(|e| format!("? ({e})"))
    );

    if let Err(e) = run(deck) {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run(deck: StreamDeck) -> Result<(), elgato_streamdeck::StreamDeckError> {
    let kind = deck.kind();
    let keys = kind.key_count();
    let (w, h) = kind.key_image_format().size;
    let last = keys - 1;
    let mut brightness: u8 = 50;

    deck.set_brightness(brightness)?;
    for key in 0..keys {
        deck.set_button_image(key, key_image(key, keys, w as u32, h as u32, false))?;
    }
    deck.flush()?;
    println!("Press keys. Key 0: dimmer, key 1: brighter, key {last}: stop.");

    #[allow(clippy::arc_with_non_send_sync)]
    let deck = Arc::new(deck);
    let reader = deck.get_reader();
    loop {
        for update in reader.read(Some(Duration::from_millis(100)))? {
            match update {
                DeviceStateUpdate::ButtonDown(key) => {
                    println!("key {key} down");
                    deck.set_button_image(key, key_image(key, keys, w as u32, h as u32, true))?;
                }
                DeviceStateUpdate::ButtonUp(key) => {
                    println!("key {key} up");
                    if key == last {
                        deck.clear_all_button_images()?;
                        deck.flush()?;
                        println!("stopped");
                        return Ok(());
                    }
                    if key == 0 {
                        brightness = brightness.saturating_sub(10);
                        deck.set_brightness(brightness)?;
                        println!("brightness {brightness}%");
                    }
                    if key == 1 {
                        brightness = (brightness + 10).min(100);
                        deck.set_brightness(brightness)?;
                        println!("brightness {brightness}%");
                    }
                    deck.set_button_image(key, key_image(key, keys, w as u32, h as u32, false))?;
                }
                other => println!("{other:?}"),
            }
        }
        deck.flush()?;
    }
}

/// A key's picture: its own colour (round the colour wheel), white while held, with its number.
fn key_image(key: u8, keys: u8, w: u32, h: u32, held: bool) -> DynamicImage {
    let bg = if held { Rgb([255, 255, 255]) } else { hue(key as f32 / keys as f32) };
    let fg = if held { Rgb([0, 0, 0]) } else { Rgb([255, 255, 255]) };
    let mut img = RgbImage::from_pixel(w, h, bg);
    draw_number(&mut img, key as u32, fg);
    DynamicImage::ImageRgb8(img)
}

fn hue(t: f32) -> Rgb<u8> {
    let x = t * 6.0;
    let f = |n: f32| {
        let k = (n + x) % 6.0;
        let v = 1.0 - (k.min(4.0 - k).clamp(0.0, 1.0));
        (v * 200.0) as u8
    };
    Rgb([f(5.0), f(3.0), f(1.0)])
}

/// Digits of a 3 x 5 pixel font, one row per entry, bit 2 is the left column.
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

fn draw_number(img: &mut RgbImage, n: u32, colour: Rgb<u8>) {
    let digits: Vec<usize> = n.to_string().bytes().map(|b| (b - b'0') as usize).collect();
    let scale = 6;
    let gap = scale;
    let width = digits.len() as u32 * 3 * scale + (digits.len() as u32 - 1) * gap;
    let x0 = (img.width() - width) / 2;
    let y0 = (img.height() - 5 * scale) / 2;
    for (i, d) in digits.iter().enumerate() {
        let dx = x0 + i as u32 * (3 * scale + gap);
        for (row, bits) in DIGITS[*d].iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) != 0 {
                    for y in 0..scale {
                        for x in 0..scale {
                            img.put_pixel(dx + col * scale + x, y0 + row as u32 * scale + y, colour);
                        }
                    }
                }
            }
        }
    }
}
