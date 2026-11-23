//! **Idea, work in progress**: a Stream Deck mockup for Maker Connect
//! ([CuriousTimo/Maker-Connect#190](https://github.com/CuriousTimo/Maker-Connect/issues/190)): pages of
//! machines with made-up data, in Maker Connect's house style. It talks to no hub and drives no machine;
//! everything on it is made up. Run: `cargo run --release --example maker_connect_demo -- --deck`
//! (the Elgato app closed); `cargo run --release --example maker_connect_demo -- <dir>` writes the
//! views as PNG pictures, no deck needed. macOS only for now: it draws with the system font
//! (`/System/Library/Fonts/SFNS.ttf`).
//!
//! The bottom row is always navigation: 10 back, 11 ◀ page, 12 where you are (press: home), 13 page ▶,
//! 14 free. The top two rows are the content: on the overview the page's machines and, last, the
//! lockers; a machine: its state, temperatures, time left, Pause/Resume and Cancel (press twice); the
//! lockers: four locks as Maker Connect's door controller (one pulse, then 10 s of rest). A machine in
//! error blinks. Page 3, the weigh station: a scale (weight, steady or not), the one spool on it
//! (picked by a scan of its QR or tag; here Scan stands for one, out of a stock of a hundred and more),
//! a badge (an RFID card at the desk, made up), Save (the gross weight into Materials; the grams used
//! since the last weighing booked on the badge's person) and a QR label (on a DYMO LabelWriter 450).
//! New spool: a new roll on the scale, Save adds it with its weight only and prints its QR label at
//! once; type and brand are filled in later.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, VariableFont};
use elgato_streamdeck::{DeviceStateUpdate, StreamDeck, list_devices, new_hidapi};
use image::{DynamicImage, Rgb, RgbImage};

// The house style, dark (the deck itself is black): mc.css `:root` dark tokens.
const BLACK: Rgb<u8> = Rgb([0, 0, 0]);
const SURFACE: Rgb<u8> = Rgb([0x1B, 0x1C, 0x1F]);
const INK: Rgb<u8> = Rgb([0xF2, 0xF1, 0xEE]);
const MUTED: Rgb<u8> = Rgb([0xA6, 0xA8, 0xA1]);
const LINE: Rgb<u8> = Rgb([0x2E, 0x30, 0x33]);
const ACCENT: Rgb<u8> = Rgb([0x93, 0xA6, 0x82]);
const ON_ACCENT: Rgb<u8> = Rgb([0x1D, 0x1D, 0x1B]);
const ST_BUSY: Rgb<u8> = Rgb([0x93, 0xA6, 0x82]);
const ST_WAIT: Rgb<u8> = Rgb([0x62, 0xC7, 0x7E]);
const ST_WARN: Rgb<u8> = Rgb([0xE3, 0xA9, 0x48]);
const ST_BAD: Rgb<u8> = Rgb([0xF2, 0x85, 0x7A]);
const ST_OFF: Rgb<u8> = Rgb([0x9A, 0x9C, 0x95]);
/// The cut corner of a card (house style: cards 12 px).
const CUT: u32 = 12;

#[derive(Clone, Copy, PartialEq)]
enum St {
    Printing,
    Idle,
    Paused,
    Error,
    Off,
}

impl St {
    fn colour(self) -> Rgb<u8> {
        match self {
            St::Printing => ST_BUSY,
            St::Idle => ST_WAIT,
            St::Paused => ST_WARN,
            St::Error => ST_BAD,
            St::Off => ST_OFF,
        }
    }
    fn word(self) -> &'static str {
        match self {
            St::Printing => "Printing",
            St::Idle => "Idle",
            St::Paused => "Paused",
            St::Error => "Error",
            St::Off => "Offline",
        }
    }
}

struct Machine {
    name: &'static str,
    what: &'static str,
    st: St,
    progress: f32,
    left_s: u32,
    nozzle: (u32, u32),
    bed: (u32, u32),
}

fn m(name: &'static str, what: &'static str, st: St, progress: f32, left_min: u32, n: (u32, u32), b: (u32, u32)) -> Machine {
    Machine { name, what, st, progress, left_s: left_min * 60, nozzle: n, bed: b }
}

enum View {
    Overview(usize),
    Machine { id: usize, from: usize, confirm: Option<Instant> },
    Lockers { from: usize },
}

/// Keys of the overview that hold machines (9 is the lockers).
const SLOTS: [u8; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 8];
/// The weigh station's page.
const WEIGH: usize = 2;
/// The weigh station's keys.
const SCALE_KEY: u8 = 0;
/// The spool on the scale, over keys 1-4.
const SPOOL_KEYS: [u8; 4] = [1, 2, 3, 4];
const NEW_KEY: u8 = 5;
const SCAN_KEY: u8 = 6;
const BADGE_KEY: u8 = 7;
const SAVE_KEY: u8 = 8;
const LABEL_KEY: u8 = 9;

/// A spool in Materials (made up): what it is, its colour, its empty spool's weight, the gross weight
/// last saved, and what it really weighs now (the scale shows that, with a little noise).
struct Spool {
    material: &'static str,
    brand: &'static str,
    name: String,
    /// its id in Materials
    id: u32,
    /// added from the scale, type and brand not filled in yet
    fresh: bool,
    colour: Rgb<u8>,
    empty_g: u32,
    saved_g: u32,
    real_g: u32,
}

/// The weigh station's state.
struct Weigh {
    spools: Vec<Spool>,
    /// the spool picked: the one on the scale
    on_scale: Option<usize>,
    /// when it was put on: the scale settles for a moment
    since: Instant,
    /// the person whose badge was shown last (made up: a press stands for a card)
    badge: Option<&'static str>,
    /// what Save did, shown for a moment: grams used and who
    saved: Option<(i64, Option<&'static str>, Instant)>,
    /// the label printing, then printed
    label: Option<Instant>,
    /// the scale's last digit moves a little
    noise: i32,
    /// the grams used booked per person (made up)
    booked: Vec<(&'static str, i64)>,
    /// the next id Materials gives (made up)
    next_id: u32,
    /// how many scans so far: the demo's scan walks through the stock
    scans: usize,
}

/// The navigation row.
const BACK: u8 = 10;
const PREV: u8 = 11;
const HOME: u8 = 12;
const NEXT: u8 = 13;

/// One lock of the door controller (part 7 of the SDK extension): a pulse of at most 0.5 s opens it,
/// then it rests at least 10 s (`LOCK_RESTING`); the door's switch says open or closed.
struct Locker {
    name: &'static str,
    door_open: bool,
    pulse_until: Option<Instant>,
    rest_until: Option<Instant>,
    /// made up for the demo: someone closes the door again
    close_at: Option<Instant>,
    /// pressed while resting: says so for a moment
    refused_until: Option<Instant>,
}

impl Locker {
    fn new(name: &'static str) -> Locker {
        Locker { name, door_open: false, pulse_until: None, rest_until: None, close_at: None, refused_until: None }
    }
    fn resting_s(&self) -> u64 {
        self.rest_until.map_or(0, |t| t.saturating_duration_since(Instant::now()).as_secs_f32().ceil() as u64).min(10)
    }
}

struct Demo {
    /// The system font at each weight used (a variable font).
    fonts: Vec<(u32, FontVec)>,
    machines: Vec<Machine>,
    pages: Vec<(&'static str, Vec<usize>)>,
    view: View,
    held: Option<u8>,
    lockers: Vec<Locker>,
    weigh: Weigh,
    /// the blink of a machine in error: on half of each second
    blink: bool,
    w: u32,
    h: u32,
}

fn main() {
    if let Some(dir) = std::env::args().nth(1).filter(|a| a != "--deck") {
        return pictures(&dir);
    }
    let hid = new_hidapi().expect("hidapi");
    let Some((kind, serial)) = list_devices(&hid).into_iter().next() else {
        eprintln!("No Stream Deck found. Is it plugged in, and is the Elgato app closed?");
        std::process::exit(1);
    };
    let deck = StreamDeck::connect(&hid, kind, &serial).expect("connect");
    let (w, h) = kind.key_image_format().size;
    println!("{kind:?}, {} keys, firmware {}", kind.key_count(), deck.firmware_version().unwrap_or_default());

    let mut demo = Demo::new(w as u32, h as u32);

    deck.set_brightness(60).ok();
    #[allow(clippy::arc_with_non_send_sync)]
    let deck = Arc::new(deck);
    let reader = deck.get_reader();
    let keys = kind.key_count();
    let mut tick = Instant::now();
    demo.draw_all(&deck, keys);
    loop {
        let updates = reader.read(Some(Duration::from_millis(100))).unwrap_or_default();
        let mut dirty = false;
        for u in updates {
            match u {
                DeviceStateUpdate::ButtonDown(k) => {
                    demo.held = Some(k);
                    dirty = true;
                }
                DeviceStateUpdate::ButtonUp(k) => {
                    demo.held = None;
                    if demo.press(k) {
                        deck.clear_all_button_images().ok();
                        deck.flush().ok();
                        return;
                    }
                    dirty = true;
                }
                _ => {}
            }
        }
        if tick.elapsed() >= Duration::from_millis(500) {
            tick = Instant::now();
            demo.blink = !demo.blink;
            if !demo.blink {
                demo.step();
            }
            demo.locks();
            demo.weigh_tick();
            dirty = true;
        }
        if dirty {
            demo.draw_all(&deck, keys);
        }
    }
}

impl Demo {
    fn new(w: u32, h: u32) -> Demo {
    let data = std::fs::read("/System/Library/Fonts/SFNS.ttf").expect("SF font");
    let fonts = [500, 600, 650, 700, 760]
        .map(|w| {
            let mut f = FontVec::try_from_vec(data.clone()).expect("font");
            f.set_variation(b"wght", w as f32);
            (w, f)
        })
        .into();
    let machines = vec![
        m("MK3S", "Prusa MK3S", St::Printing, 0.42, 72, (215, 215), (60, 60)),
        m("MINI", "Prusa MINI+", St::Idle, 0.0, 0, (24, 0), (23, 0)),
        m("AtomX", "AtomX MK3", St::Error, 0.18, 0, (24, 0), (23, 0)),
        m("XL", "Prusa XL", St::Printing, 0.87, 9, (240, 240), (85, 85)),
        m("Bench", "Bench Mega", St::Paused, 0.55, 31, (170, 200), (60, 60)),
        m("Ender", "Ender 3", St::Off, 0.0, 0, (0, 0), (0, 0)),
        m("Ortur", "Ortur LM2", St::Idle, 0.0, 0, (0, 0), (0, 0)),
        m("LW450", "LabelWriter 450", St::Idle, 0.0, 0, (0, 0), (0, 0)),
        m("PnP", "LabelManager PnP", St::Printing, 0.6, 1, (0, 0), (0, 0)),
        m("Scale", "DYMO M10", St::Off, 0.0, 0, (0, 0), (0, 0)),
    ];
    let mut demo = Demo {
        fonts,
        machines,
        pages: vec![
            ("Printers", vec![0, 1, 2, 3, 4, 5]),
            ("Laser & labels", vec![6, 7, 8, 9]),
            ("Weigh station", vec![]),
        ],
        view: View::Overview(0),
        held: None,
        lockers: ["Locker 1", "Locker 2", "Locker 3", "Locker 4"].map(Locker::new).into(),
        weigh: Weigh {
            spools: vec![
                Spool { material: "PLA", brand: "Prusament", name: "Galaxy Black".into(), id: 31, fresh: false, colour: Rgb([0x2B, 0x2B, 0x33]), empty_g: 250, saved_g: 1062, real_g: 831 },
                Spool { material: "PETG", brand: "Prusament", name: "Ocean Blue".into(), id: 32, fresh: false, colour: Rgb([0x2F, 0x6F, 0xB8]), empty_g: 220, saved_g: 640, real_g: 598 },
                Spool { material: "PLA", brand: "Prusament", name: "Signal White".into(), id: 33, fresh: false, colour: Rgb([0xEE, 0xEE, 0xEA]), empty_g: 250, saved_g: 1180, real_g: 1180 },
                Spool { material: "ASA", brand: "Prusament", name: "Jet Black".into(), id: 34, fresh: false, colour: Rgb([0x15, 0x15, 0x15]), empty_g: 230, saved_g: 905, real_g: 712 },
                Spool { material: "TPU", brand: "Prusament", name: "Fire Red".into(), id: 35, fresh: false, colour: Rgb([0xC8, 0x32, 0x2B]), empty_g: 200, saved_g: 455, real_g: 441 },
                Spool { material: "PLA", brand: "Prusament", name: "Sage".into(), id: 36, fresh: false, colour: Rgb([0x93, 0xA6, 0x82]), empty_g: 250, saved_g: 1250, real_g: 1250 },
            ],
            on_scale: None,
            since: Instant::now(),
            scans: 0,
            badge: None,
            saved: None,
            label: None,
            noise: 0,
            booked: Vec::new(),
            next_id: 125,
        },
        blink: false,
        w,
        h,
    };
    // the rest of the stock, made up: a hundred and more spools
    const MATS: [(&str, &str, [u8; 3]); 6] = [
        ("PLA", "Prusament", [0xD9, 0x6B, 0x2B]),
        ("PETG", "Prusament", [0x3A, 0x7D, 0x44]),
        ("PLA", "Bambu", [0x8A, 0x8C, 0x8E]),
        ("ASA", "Fillamentum", [0xE8, 0xE4, 0xD8]),
        ("PETG", "Polymaker", [0x6E, 0x3B, 0x8E]),
        ("PLA", "eSun", [0xF2, 0xC9, 0x3A]),
    ];
    let first = demo.weigh.spools.len() as u32;
    for n in first..124 {
        let (material, brand, c) = MATS[n as usize % MATS.len()];
        let full = 1000 + 250;
        let left = 120 + (n * 137) % 880;
        demo.weigh.spools.push(Spool {
            material,
            brand,
            name: format!("{brand} {material}"),
            id: n + 1,
            fresh: false,
            colour: Rgb(c),
            empty_g: 250,
            saved_g: (250 + left).min(full),
            real_g: (250 + left).min(full) - (n * 13) % 60,
        });
    }
    demo

    }

    /// A key released; true: stop the demo (never, for now).
    fn press(&mut self, k: u8) -> bool {
        let n = self.pages.len();
        // the navigation row, the same everywhere
        match (k, &self.view) {
            (HOME, _) => {
                self.view = View::Overview(0);
                return false;
            }
            (BACK, View::Machine { from, .. }) | (BACK, View::Lockers { from }) => {
                self.view = View::Overview(*from);
                return false;
            }
            (PREV, View::Overview(p)) if n > 1 => {
                self.view = View::Overview((p + n - 1) % n);
                return false;
            }
            (NEXT, View::Overview(p)) if n > 1 => {
                self.view = View::Overview((p + 1) % n);
                return false;
            }
            _ => {}
        }
        match self.view {
            View::Overview(WEIGH) => self.weigh_press(k),
            View::Overview(p) => {
                if k == 9 {
                    self.view = View::Lockers { from: p };
                } else if let Some(i) = SLOTS.iter().position(|s| *s == k)
                    && let Some(&id) = self.pages[p].1.get(i)
                {
                    self.view = View::Machine { id, from: p, confirm: None };
                }
            }
            View::Machine { id, from, confirm } => match k {
                8 => {
                    let m = &mut self.machines[id];
                    m.st = match m.st {
                        St::Printing => St::Paused,
                        St::Paused => St::Printing,
                        s => s,
                    };
                }
                9 => {
                    let st = self.machines[id].st;
                    if matches!(st, St::Printing | St::Paused) {
                        if confirm.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
                            let m = &mut self.machines[id];
                            m.st = St::Idle;
                            m.progress = 0.0;
                            m.left_s = 0;
                            self.view = View::Machine { id, from, confirm: None };
                        } else {
                            self.view = View::Machine { id, from, confirm: Some(Instant::now()) };
                        }
                    }
                }
                _ => {}
            },
            View::Lockers { .. } => {
                if k < 4 {
                    self.open_lock(k as usize);
                }
            }
        }
        false
    }

    fn weigh_press(&mut self, k: u8) {
        let w = &mut self.weigh;
        if k == NEW_KEY {
            // a new roll put on the scale: one entry, nothing known but what the scale will say
            let id = w.next_id;
            w.next_id += 1;
            w.spools.push(Spool {
                material: "?",
                brand: "",
                name: format!("Spool #{id}"),
                id,
                fresh: true,
                colour: Rgb([0x58, 0x5C, 0x54]),
                empty_g: 0,
                saved_g: 0,
                real_g: 1247,
            });
            w.on_scale = Some(w.spools.len() - 1);
            w.since = Instant::now();
            w.saved = None;
            w.label = None;
        } else if k == SCAN_KEY {
            // a scan (QR or tag; made up): the next spool of the stock is on the scale now
            const SHOWN: [usize; 5] = [0, 3, 1, 57, 4];
            let i = SHOWN[w.scans % SHOWN.len()];
            w.scans += 1;
            w.on_scale = Some(i);
            w.since = Instant::now();
            w.saved = None;
            w.label = None;
        } else if k == BADGE_KEY {
            const PEOPLE: [&str; 3] = ["Timo", "Sanne", "Guest 17"];
            w.badge = match w.badge {
                None => Some(PEOPLE[0]),
                Some(p) => PEOPLE.iter().position(|x| *x == p).and_then(|i| PEOPLE.get(i + 1)).copied(),
            };
        } else if k == SAVE_KEY
            && let Some(i) = w.on_scale
            && w.since.elapsed() >= Duration::from_millis(1500)
        {
            let s = &mut w.spools[i];
            if s.fresh && s.saved_g == 0 {
                // added: its first weight, and its QR label at once
                s.saved_g = s.real_g;
                w.saved = Some((-(s.real_g as i64), None, Instant::now()));
                w.label = Some(Instant::now());
                println!("added: spool #{} with {} g gross, QR label to the LW450; fill in type and brand in Materials", s.id, s.real_g);
                return;
            }
            let used = s.saved_g as i64 - s.real_g as i64;
            s.saved_g = s.real_g;
            if let Some(who) = w.badge
                && used > 0
            {
                match w.booked.iter_mut().find(|(p, _)| *p == who) {
                    Some((_, g)) => *g += used,
                    None => w.booked.push((who, used)),
                }
            }
            w.saved = Some((used, w.badge, Instant::now()));
            println!("saved: {} {} {} g gross, used {used} g, booked on {:?} (total {:?})", s.material, s.name, s.real_g, w.badge, w.booked);
        } else if k == LABEL_KEY && w.on_scale.is_some() && w.label.is_none() {
            w.label = Some(Instant::now());
        }
    }

    /// `OPEN_LOCK` as part 7 has it: one pulse, then rest; refused while resting.
    fn open_lock(&mut self, i: usize) {
        let now = Instant::now();
        let l = &mut self.lockers[i];
        if l.resting_s() > 0 {
            l.refused_until = Some(now + Duration::from_millis(1500));
            return;
        }
        l.pulse_until = Some(now + Duration::from_millis(500));
        l.rest_until = Some(now + Duration::from_millis(10_500));
        l.door_open = true;
        l.close_at = Some(now + Duration::from_secs(6));
    }

    /// The scale's last digit (made up noise, ±1 g) and how long messages stay.
    fn weigh_tick(&mut self) {
        let w = &mut self.weigh;
        w.noise = match w.noise {
            0 => 1,
            1 => -1,
            _ => 0,
        };
        if w.saved.is_some_and(|(_, _, t)| t.elapsed() >= Duration::from_secs(4)) {
            w.saved = None;
        }
    }

    /// The locks' time: pulses end, rests run out, doors are closed again (made up).
    fn locks(&mut self) {
        let now = Instant::now();
        for l in &mut self.lockers {
            if l.pulse_until.is_some_and(|t| now >= t) {
                l.pulse_until = None;
            }
            if l.rest_until.is_some_and(|t| now >= t) {
                l.rest_until = None;
            }
            if l.close_at.is_some_and(|t| now >= t) {
                l.close_at = None;
                l.door_open = false;
            }
            if l.refused_until.is_some_and(|t| now >= t) {
                l.refused_until = None;
            }
        }
    }

    /// One second of made-up time: prints go on, heaters reach their target.
    fn step(&mut self) {
        for m in &mut self.machines {
            if m.st == St::Printing && m.left_s > 0 {
                m.left_s -= 1;
                let total = (m.left_s as f32 / (1.0 - m.progress).max(0.01)).max(1.0);
                m.progress = (m.progress + 1.0 / total).min(1.0);
                if m.left_s == 0 {
                    m.st = St::Idle;
                    m.progress = 0.0;
                }
            }
        }
        if let View::Machine { id, from, confirm: Some(t) } = self.view
            && t.elapsed() >= Duration::from_secs(3)
        {
            self.view = View::Machine { id, from, confirm: None };
        }
    }

    fn draw_all(&self, deck: &StreamDeck, keys: u8) {
        for k in 0..keys {
            let img = self.key(k);
            deck.set_button_image(k, DynamicImage::ImageRgb8(img)).ok();
        }
        deck.flush().ok();
    }

    fn key(&self, k: u8) -> RgbImage {
        let held = self.held == Some(k);
        let empty = || RgbImage::from_pixel(self.w, self.h, BLACK);
        let pages = matches!(self.view, View::Overview(_)) && self.pages.len() > 1;
        // the navigation row, the same everywhere
        match k {
            BACK => return if matches!(self.view, View::Overview(_)) { empty() } else { self.nav("←", "Back", held) },
            PREV => return if pages { self.nav("◀", "Page", held) } else { empty() },
            NEXT => return if pages { self.nav("▶", "Page", held) } else { empty() },
            HOME => {
                return match self.view {
                    View::Overview(p) => self.logo(self.pages[p].0, &format!("{}/{}", p + 1, self.pages.len())),
                    View::Machine { id, .. } => self.logo(self.machines[id].name, "machine"),
                    View::Lockers { .. } => self.logo("Lockers", "4 locks"),
                };
            }
            14 => return empty(),
            _ => {}
        }
        match self.view {
            View::Overview(WEIGH) => self.weigh_key(k, held),
            View::Overview(p) => {
                if k == 9 {
                    return self.lockers_key(held);
                }
                match SLOTS.iter().position(|s| *s == k).and_then(|i| self.pages[p].1.get(i)) {
                    Some(&id) => self.machine_key(&self.machines[id], held),
                    None => empty(),
                }
            }
            View::Machine { id, confirm, .. } => {
                let m = &self.machines[id];
                let printer = m.nozzle.1 > 0 || m.nozzle.0 > 0;
                let job = matches!(m.st, St::Printing | St::Paused);
                match k {
                    0 => self.title_key(m),
                    1 if m.st == St::Error && self.blink => {
                        let mut img = self.tile(ST_BAD);
                        self.text(&mut img, "State", 7, 20, 10.0, 500.0, ON_ACCENT);
                        self.text_fit(&mut img, "Error", 7, 46, 17.0, 650.0, ON_ACCENT);
                        img
                    }
                    1 => self.value_key("State", m.st.word(), m.st.colour(), held),
                    2 if job => self.value_key("Done", &format!("{}%", (m.progress * 100.0).round()), INK, held),
                    3 if job => self.value_key("Left", &left(m.left_s), INK, held),
                    5 if printer => self.value_key("Nozzle", &temp(m.nozzle), INK, held),
                    6 if printer => self.value_key("Bed", &temp(m.bed), INK, held),
                    8 if job => self.action(if m.st == St::Paused { "Resume" } else { "Pause" }, None, held),
                    9 if job => {
                        if confirm.is_some() {
                            self.action("Cancel?", Some("press again"), true)
                        } else {
                            self.action("Cancel", None, held)
                        }
                    }
                    _ => empty(),
                }
            }
            View::Lockers { .. } => match k {
                0..=3 => self.locker_key(&self.lockers[k as usize], held),
                _ => empty(),
            },
        }
    }

    // ---- the keys ----

    /// Key 0: the logo on the house colour, dark text on it (never white), the page under it.
    fn logo(&self, page: &str, n: &str) -> RgbImage {
        let mut img = self.tile(ACCENT);
        self.text(&mut img, "MC", 8, 24, 22.0, 760.0, ON_ACCENT);
        self.text_fit(&mut img, page, 8, 44, 11.0, 600.0, ON_ACCENT);
        self.text_fit(&mut img, n, 8, 60, 10.0, 500.0, ON_ACCENT);
        img
    }

    /// A machine on the overview: its name, its value, its state in its own colour with a bar.
    fn machine_key(&self, m: &Machine, held: bool) -> RgbImage {
        if m.st == St::Error && self.blink {
            // In error: the whole key in the fault colour, opaque, dark text on it; every half second.
            let mut img = self.tile(ST_BAD);
            self.text_fit(&mut img, m.name, 7, 17, 13.0, 650.0, ON_ACCENT);
            self.text_fit(&mut img, "Error", 7, 41, 18.0, 650.0, ON_ACCENT);
            self.text_fit(&mut img, "needs a look", 7, 56, 10.0, 500.0, ON_ACCENT);
            return img;
        }
        let mut img = self.tile(self.fill(held));
        self.text_fit(&mut img, m.name, 7, 17, 13.0, 650.0, INK);
        let (value, colour) = match m.st {
            St::Printing | St::Paused => (format!("{}%", (m.progress * 100.0).round()), INK),
            s => (s.word().to_owned(), s.colour()),
        };
        self.text_fit(&mut img, &value, 7, 41, 18.0, 650.0, colour);
        let sub = match m.st {
            St::Printing => left(m.left_s),
            St::Paused => "Paused".into(),
            St::Error => "needs a look".into(),
            St::Idle => "free".into(),
            St::Off => "not seen".into(),
        };
        self.text_fit(&mut img, &sub, 7, 56, 10.0, 500.0, MUTED);
        // The state's bar: opaque status colour (house style: never translucent), the progress in it.
        let (y0, y1) = (self.h - 8, self.h - 4);
        let frac = if matches!(m.st, St::Printing | St::Paused) { m.progress } else { 1.0 };
        let x1 = 7 + ((self.w - 14) as f32 * frac) as u32;
        self.rect(&mut img, 7, y0, self.w - 7, y1, LINE);
        self.rect(&mut img, 7, y0, x1, y1, m.st.colour());
        img
    }

    fn weigh_key(&self, k: u8, held: bool) -> RgbImage {
        let w = &self.weigh;
        let steady = w.since.elapsed() >= Duration::from_millis(1500);
        match k {
            SCALE_KEY => {
                let mut img = self.tile(self.fill(held));
                self.text(&mut img, "Scale", 7, 17, 11.0, 600.0, MUTED);
                match w.on_scale {
                    None => {
                        self.text_fit(&mut img, "0 g", 7, 43, 20.0, 700.0, INK);
                        self.text_fit(&mut img, "put a spool on", 7, 58, 10.0, 500.0, MUTED);
                    }
                    Some(i) => {
                        let s = &w.spools[i];
                        let shown = if steady {
                            s.real_g as i32 + w.noise
                        } else {
                            // settling: the reading still moves
                            s.real_g as i32 + ((w.since.elapsed().as_millis() as i32 / 97) % 37) - 18
                        };
                        self.text_fit(&mut img, &grams(shown as i64), 7, 43, 20.0, 700.0, INK);
                        let (t, c) = if steady { ("steady", ST_WAIT) } else { ("settling", ST_WARN) };
                        self.text_fit(&mut img, t, 7, 58, 10.0, 500.0, c);
                    }
                }
                img
            }
            BADGE_KEY => {
                let mut img = self.tile(self.fill(held));
                self.text(&mut img, "Badge", 7, 17, 11.0, 600.0, MUTED);
                match w.badge {
                    Some(who) => {
                        self.text_fit(&mut img, who, 7, 41, 16.0, 650.0, INK);
                        let total = w.booked.iter().find(|(p, _)| *p == who).map_or(0, |(_, g)| *g);
                        self.text_fit(&mut img, &format!("{} used", grams(total)), 7, 57, 10.0, 500.0, MUTED);
                    }
                    None => {
                        self.text_fit(&mut img, "—", 7, 41, 16.0, 650.0, MUTED);
                        self.text_fit(&mut img, "tap a card", 7, 57, 10.0, 500.0, MUTED);
                    }
                }
                img
            }
            SAVE_KEY => {
                if let Some((used, _, _)) = w.saved
                    && used < 0
                {
                    let id = w.on_scale.map_or(0, |i| w.spools[i].id);
                    let mut img = self.tile(ST_WAIT);
                    self.text_centre(&mut img, &format!("Added #{id}"), 24, 13.0, 650.0, ON_ACCENT);
                    self.text_centre(&mut img, &grams(-used), 44, 15.0, 700.0, ON_ACCENT);
                    self.text_centre(&mut img, "+ QR label", 59, 10.0, 500.0, ON_ACCENT);
                    return img;
                }
                if let Some((used, who, _)) = w.saved {
                    let mut img = self.tile(ST_WAIT);
                    self.text_centre(&mut img, "Saved", 24, 13.0, 650.0, ON_ACCENT);
                    self.text_centre(&mut img, &format!("−{}", grams(used.max(0))), 44, 15.0, 700.0, ON_ACCENT);
                    self.text_centre(&mut img, who.unwrap_or("no badge"), 59, 10.0, 500.0, ON_ACCENT);
                    return img;
                }
                let ready = w.on_scale.is_some() && steady;
                let mut img = self.tile(self.fill(held));
                self.text_centre(&mut img, "Save", 38, 15.0, 650.0, if ready { INK } else { MUTED });
                let sub = match (w.on_scale, steady) {
                    (None, _) => "pick a spool",
                    (Some(_), false) => "wait: settling",
                    (Some(_), true) => "into Materials",
                };
                self.text_centre(&mut img, sub, 55, 10.0, 500.0, MUTED);
                img
            }
            LABEL_KEY => {
                let mut img = self.tile(self.fill(held));
                match (w.on_scale, w.label) {
                    (Some(_), Some(t)) if t.elapsed() < Duration::from_secs(2) => {
                        self.text_centre(&mut img, "Printing QR", 38, 13.0, 650.0, ST_BUSY);
                        self.text_centre(&mut img, "LW450", 55, 10.0, 500.0, MUTED);
                    }
                    (Some(_), Some(_)) => {
                        self.text_centre(&mut img, "QR printed", 38, 13.0, 650.0, ST_WAIT);
                        self.text_centre(&mut img, "LW450", 55, 10.0, 500.0, MUTED);
                    }
                    (on, _) => {
                        self.text_centre(&mut img, "QR label", 38, 15.0, 650.0, if on.is_some() { INK } else { MUTED });
                        self.text_centre(&mut img, "LW450", 55, 10.0, 500.0, MUTED);
                    }
                }
                img
            }
            NEW_KEY => {
                let mut img = self.tile(self.fill(held));
                self.text_centre(&mut img, "+", 34, 26.0, 600.0, INK);
                self.text_centre(&mut img, "New spool", 55, 11.0, 600.0, INK);
                img
            }
            SCAN_KEY => {
                let mut img = self.tile(self.fill(held));
                self.text_centre(&mut img, "Scan", 36, 15.0, 650.0, INK);
                self.text_centre(&mut img, "QR or tag", 52, 10.0, 500.0, MUTED);
                self.text_centre(&mut img, &format!("{} in stock", w.spools.len()), 64, 9.0, 500.0, MUTED);
                img
            }
            _ => match SPOOL_KEYS.iter().position(|s| *s == k) {
                Some(part) => self.spool_part(part, held),
                None => RgbImage::from_pixel(self.w, self.h, BLACK),
            },
        }
    }

    /// The spool on the scale, over four keys: what it is, its name, as last saved, and the grams
    /// used since then (live, once steady). No spool: "scan a spool".
    fn spool_part(&self, part: usize, held: bool) -> RgbImage {
        let w = &self.weigh;
        let mut img = self.tile(self.fill(held));
        let Some(i) = w.on_scale else {
            if part == 0 {
                self.text(&mut img, "Spool", 7, 17, 11.0, 600.0, MUTED);
                self.text_fit(&mut img, "none", 7, 41, 16.0, 650.0, MUTED);
                self.text_fit(&mut img, "scan its QR", 7, 57, 10.0, 500.0, MUTED);
            }
            return img;
        };
        let s = &w.spools[i];
        let steady = w.since.elapsed() >= Duration::from_millis(1500);
        match part {
            0 => {
                // the filament's colour, a swatch with a hairline so black and white show
                self.rect(&mut img, 7, 8, 23, 24, LINE);
                self.rect(&mut img, 8, 9, 22, 23, s.colour);
                self.text_fit(&mut img, s.material, 7, 46, 18.0, 700.0, if s.fresh { ST_WARN } else { INK });
                self.text_fit(&mut img, &format!("#{}", s.id), 7, 60, 10.0, 500.0, MUTED);
            }
            1 => {
                self.text(&mut img, "Spool", 7, 17, 11.0, 600.0, MUTED);
                if s.fresh {
                    self.text_fit(&mut img, "new", 7, 40, 15.0, 650.0, ST_WARN);
                    self.text_fit(&mut img, "fill in Materials", 7, 57, 10.0, 500.0, ST_WARN);
                } else {
                    self.text_fit(&mut img, &s.name, 7, 40, 13.0, 650.0, INK);
                    self.text_fit(&mut img, s.brand, 7, 57, 10.0, 500.0, MUTED);
                }
            }
            2 => {
                self.text(&mut img, "Last saved", 7, 17, 11.0, 600.0, MUTED);
                if s.saved_g == 0 {
                    self.text_fit(&mut img, "—", 7, 40, 15.0, 650.0, MUTED);
                    self.text_fit(&mut img, "never weighed", 7, 57, 10.0, 500.0, MUTED);
                } else {
                    self.text_fit(&mut img, &grams(s.saved_g as i64), 7, 40, 15.0, 650.0, INK);
                    let left = if s.empty_g > 0 { format!("{} left", grams(s.saved_g as i64 - s.empty_g as i64)) } else { "empty: ?".into() };
                    self.text_fit(&mut img, &left, 7, 57, 10.0, 500.0, MUTED);
                }
            }
            _ => {
                self.text(&mut img, "Used", 7, 17, 11.0, 600.0, MUTED);
                if s.saved_g == 0 || s.fresh || !steady {
                    let why = if !steady { "settling" } else if s.fresh && s.saved_g > 0 { "just added" } else { "first weighing" };
                    self.text_fit(&mut img, "—", 7, 40, 15.0, 650.0, MUTED);
                    self.text_fit(&mut img, why, 7, 57, 10.0, 500.0, MUTED);
                } else {
                    let used = s.saved_g as i64 - s.real_g as i64;
                    self.text_fit(&mut img, &format!("−{}", grams(used.max(0))), 7, 40, 15.0, 650.0, INK);
                    self.text_fit(&mut img, "since last saved", 7, 57, 10.0, 500.0, MUTED);
                }
            }
        }
        img
    }

    /// The lockers on the overview: how many doors are open.
    fn lockers_key(&self, held: bool) -> RgbImage {
        let open = self.lockers.iter().filter(|l| l.door_open).count();
        let mut img = self.tile(self.fill(held));
        self.text_fit(&mut img, "Lockers", 7, 17, 13.0, 650.0, INK);
        let (v, c) = if open == 0 { ("Closed".to_owned(), INK) } else { (format!("{open} open"), ST_WARN) };
        self.text_fit(&mut img, &v, 7, 41, 18.0, 650.0, c);
        self.text_fit(&mut img, &format!("{} locks ›", self.lockers.len()), 7, 56, 10.0, 500.0, MUTED);
        img
    }

    /// A lock: its door, its pulse, its rest (a bar running down), "resting" when pressed too soon.
    fn locker_key(&self, l: &Locker, held: bool) -> RgbImage {
        let mut img = self.tile(self.fill(held));
        self.text_fit(&mut img, l.name, 7, 17, 13.0, 650.0, INK);
        let (v, c) = if l.pulse_until.is_some() {
            ("Opening", ST_BUSY)
        } else if l.door_open {
            ("Open", ST_WARN)
        } else {
            ("Closed", INK)
        };
        self.text_fit(&mut img, v, 7, 41, 18.0, 650.0, c);
        let rest = l.resting_s();
        let (sub, sc) = if l.refused_until.is_some() {
            ("resting".to_owned(), ST_WARN)
        } else if rest > 0 {
            (format!("rest {rest} s"), MUTED)
        } else {
            ("press to open".to_owned(), MUTED)
        };
        self.text_fit(&mut img, &sub, 7, 56, 10.0, 500.0, sc);
        let (y0, y1) = (self.h - 8, self.h - 4);
        self.rect(&mut img, 7, y0, self.w - 7, y1, LINE);
        if rest > 0 {
            let x1 = 7 + ((self.w - 14) as f32 * (rest as f32 / 10.0).min(1.0)) as u32;
            self.rect(&mut img, 7, y0, x1, y1, ST_OFF);
        }
        img
    }

    #[allow(dead_code)]
    fn title_text(&self, title: &str, sub: &str) -> RgbImage {
        let mut img = self.tile(SURFACE);
        self.text_fit(&mut img, title, 7, 28, 15.0, 700.0, INK);
        self.text_fit(&mut img, sub, 7, 46, 10.0, 500.0, MUTED);
        img
    }

    fn title_key(&self, m: &Machine) -> RgbImage {
        let mut img = self.tile(SURFACE);
        self.text_fit(&mut img, m.name, 7, 28, 16.0, 700.0, INK);
        self.text_fit(&mut img, m.what, 7, 46, 10.0, 500.0, MUTED);
        img
    }

    fn value_key(&self, label: &str, value: &str, colour: Rgb<u8>, held: bool) -> RgbImage {
        let mut img = self.tile(self.fill(held));
        self.text(&mut img, label, 7, 20, 10.0, 500.0, MUTED);
        self.text_fit(&mut img, value, 7, 46, 17.0, 650.0, colour);
        img
    }

    fn nav(&self, arrow: &str, label: &str, held: bool) -> RgbImage {
        let mut img = self.tile(self.fill(held));
        self.text_centre(&mut img, arrow, 40, 24.0, 600.0, INK);
        self.text_centre(&mut img, label, 58, 10.0, 500.0, MUTED);
        img
    }

    /// An action; `warn`: the second press is asked for (Cancel), in the warning colour, opaque.
    fn action(&self, word: &str, sub: Option<&str>, warn: bool) -> RgbImage {
        let (fill, ink) = if warn && sub.is_some() { (ST_WARN, ON_ACCENT) } else { (self.fill(warn), INK) };
        let mut img = self.tile(fill);
        self.text_centre(&mut img, word, 40, 15.0, 650.0, ink);
        if let Some(sub) = sub {
            self.text_centre(&mut img, sub, 56, 10.0, 500.0, ink);
        }
        img
    }

    // ---- drawing ----

    /// Held: the selected tint (house colour 18 %) over the surface.
    fn fill(&self, held: bool) -> Rgb<u8> {
        if held { mix(SURFACE, ACCENT, 0.18) } else { SURFACE }
    }

    /// A card: square corners, one 45° cut top right where the deck's black shows.
    fn tile(&self, fill: Rgb<u8>) -> RgbImage {
        let mut img = RgbImage::from_pixel(self.w, self.h, fill);
        for y in 0..CUT {
            for x in (self.w - CUT + y)..self.w {
                img.put_pixel(x, y, BLACK);
            }
        }
        img
    }

    fn rect(&self, img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, c: Rgb<u8>) {
        for y in y0..y1.min(self.h) {
            for x in x0..x1.min(self.w) {
                img.put_pixel(x, y, c);
            }
        }
    }

    fn width(&self, s: &str, size: f32, weight: f32) -> f32 {
        let font = self.weighted(weight);
        let sf = font.as_scaled(PxScale::from(size));
        s.chars().map(|c| sf.h_advance(sf.glyph_id(c))).sum()
    }

    fn weighted(&self, weight: f32) -> &FontVec {
        &self.fonts.iter().find(|(w, _)| *w as f32 == weight).unwrap_or(&self.fonts[0]).1
    }

    /// Text with its baseline at `y`.
    #[allow(clippy::too_many_arguments)]
    fn text(&self, img: &mut RgbImage, s: &str, x: i32, y: i32, size: f32, weight: f32, c: Rgb<u8>) {
        let font = self.weighted(weight);
        let sf = font.as_scaled(PxScale::from(size));
        let mut pen = x as f32;
        for ch in s.chars() {
            let id = sf.glyph_id(ch);
            let g = id.with_scale_and_position(PxScale::from(size), ab_glyph::point(pen, y as f32));
            pen += sf.h_advance(id);
            if let Some(o) = font.outline_glyph(g) {
                let b = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && (px as u32) < self.w && (py as u32) < self.h {
                        let bg = *img.get_pixel(px as u32, py as u32);
                        img.put_pixel(px as u32, py as u32, mix(bg, c, cov));
                    }
                });
            }
        }
    }

    /// Text that shrinks to fit the key's width.
    #[allow(clippy::too_many_arguments)]
    fn text_fit(&self, img: &mut RgbImage, s: &str, x: i32, y: i32, size: f32, weight: f32, c: Rgb<u8>) {
        let room = self.w as f32 - 2.0 * x as f32;
        let mut size = size;
        while size > 7.0 && self.width(s, size, weight) > room {
            size -= 0.5;
        }
        self.text(img, s, x, y, size, weight, c);
    }

    fn text_centre(&self, img: &mut RgbImage, s: &str, y: i32, size: f32, weight: f32, c: Rgb<u8>) {
        let x = ((self.w as f32 - self.width(s, size, weight)) / 2.0).max(2.0) as i32;
        self.text(img, s, x, y, size, weight, c);
    }
}

fn mix(a: Rgb<u8>, b: Rgb<u8>, t: f32) -> Rgb<u8> {
    let f = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
    Rgb([f(0), f(1), f(2)])
}

fn left(s: u32) -> String {
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 { format!("{h} h {m:02} min") } else if s >= 60 { format!("{m} min") } else { format!("{s} s") }
}

/// Grams with a thin space between thousands: "1 043 g".
fn grams(g: i64) -> String {
    let n = g.unsigned_abs();
    let s = if n >= 1000 { format!("{} {:03}", n / 1000, n % 1000) } else { n.to_string() };
    format!("{}{s} g", if g < 0 { "−" } else { "" })
}

fn temp((now, target): (u32, u32)) -> String {
    if target > 0 { format!("{now}/{target}°") } else { format!("{now}°") }
}

/// `deck-demo <dir>`: the views as pictures of the 5 x 3 deck, no deck needed.
fn pictures(dir: &str) {
    let (w, h, gap) = (72u32, 72u32, 12u32);
    let mut demo = Demo::new(w, h);
    let shot = |demo: &Demo, name: &str| {
        let mut img = RgbImage::from_pixel(5 * w + 6 * gap, 3 * h + 4 * gap, Rgb([0x0A, 0x0A, 0x0A]));
        for k in 0..15u8 {
            let key = demo.key(k);
            let (cx, cy) = (gap + (k as u32 % 5) * (w + gap), gap + (k as u32 / 5) * (h + gap));
            image::imageops::overlay(&mut img, &key, cx as i64, cy as i64);
        }
        let big = image::imageops::resize(&img, img.width() * 2, img.height() * 2, image::imageops::FilterType::Nearest);
        big.save(format!("{dir}/{name}.png")).unwrap();
    };
    shot(&demo, "1-printers");
    demo.view = View::Overview(1);
    shot(&demo, "2-laser-labels");
    demo.view = View::Machine { id: 0, from: 0, confirm: None };
    shot(&demo, "3-mk3s");
    demo.view = View::Machine { id: 0, from: 0, confirm: Some(Instant::now()) };
    demo.held = Some(9);
    shot(&demo, "4-mk3s-cancel");
    demo.held = None;
    demo.view = View::Overview(0);
    demo.blink = true;
    shot(&demo, "5-printers-blink");
    demo.view = View::Lockers { from: 0 };
    demo.open_lock(1);
    demo.lockers[1].pulse_until = None;
    demo.open_lock(2);
    demo.open_lock(2);
    shot(&demo, "6-lockers");
    demo.view = View::Overview(WEIGH);
    demo.weigh.badge = Some("Timo");
    demo.weigh_press(SCAN_KEY);
    demo.weigh.since = Instant::now() - Duration::from_secs(3);
    shot(&demo, "7-weigh");
    demo.weigh_press(SAVE_KEY);
    shot(&demo, "8-weigh-saved");
    demo.weigh.saved = None;
    demo.weigh_press(NEW_KEY);
    demo.weigh.since = Instant::now() - Duration::from_secs(3);
    demo.weigh_press(SAVE_KEY);
    shot(&demo, "9-new-spool");
}
