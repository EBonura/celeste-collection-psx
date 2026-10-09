//! Headless navigation routes for the Celeste Classic Collection disc.
//!
//! Every way around the launcher and both games' pause menus is one route: a
//! scripted pad tape (one sample per emulator route tick, so it can hold the
//! analog stick as well as buttons) replayed by PSoXide's headless `frontend
//! launch`, with screenshots at the checkpoints. Each checkpoint names the
//! screen it expects (menu, credits, settings, pause or game) and, on the cover
//! menu, which game is highlighted; on the settings screen, which row.
//!
//!     nav-routes --frontend PATH/TO/frontend \
//!         --disc dist/celeste-collection.cue [--out DIR] [--only NAME ...]
//!
//! Exits non-zero if any checkpoint fails. Screens are recognised from fixed UI
//! pixels (the menu's gold hint labels, the credits' navy fill, the settings
//! highlight bar, the pause panel's white border), so a checkpoint must sit
//! where the screen has settled, not mid-fade.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

// ---- routes ----------------------------------------------------------------
// Event: (tick, inputs, hold). inputs is '+'-joined buttons and/or stick
// directions (stick_left/right/up/down push the left stick fully that way).
// Check: (tick, screen, detail) where detail is the highlighted game on the
// menu (0 Celeste, 1 Celeste 2), the highlighted row on the settings screen, or
// None.

type Event = (usize, &'static str, usize);
type Check = (usize, &'static str, Option<i64>);

const LAUNCH_C1: [Event; 2] = [(60, "cross", 8), (150, "cross", 8)];
const LAUNCH_C2: [Event; 3] = [(60, "cross", 8), (150, "right", 8), (200, "cross", 8)];

struct Route {
    name: &'static str,
    events: Vec<Event>,
    checks: Vec<Check>,
}

fn route(name: &'static str, prefix: &[Event], events: &[Event], checks: &[Check]) -> Route {
    Route {
        name,
        events: prefix.iter().chain(events).copied().collect(),
        checks: checks.to_vec(),
    }
}

fn routes() -> Vec<Route> {
    let none: &[Event] = &[];
    vec![
        // boot and the cover menu
        route(
            "intro_skip_cross",
            none,
            &[(60, "cross", 8)],
            &[(150, "menu", Some(0))],
        ),
        route(
            "intro_skip_circle",
            none,
            &[(60, "circle", 8)],
            &[(150, "menu", Some(0))],
        ),
        route(
            "intro_skip_start",
            none,
            &[(60, "start", 8)],
            &[(150, "menu", Some(0)), (250, "menu", Some(0))],
        ),
        route(
            "menu_dpad",
            none,
            &[(60, "cross", 8), (150, "right", 8), (250, "left", 8)],
            &[(200, "menu", Some(1)), (300, "menu", Some(0))],
        ),
        route(
            "menu_stick",
            none,
            &[
                (60, "cross", 8),
                (150, "stick_right", 8),
                (250, "stick_left", 8),
            ],
            &[(200, "menu", Some(1)), (300, "menu", Some(0))],
        ),
        route(
            "menu_circle_does_nothing",
            none,
            &[(60, "cross", 8), (150, "right", 8), (200, "circle", 8)],
            &[(300, "menu", Some(1))],
        ),
        // credits: Select opens, Cross / Circle / Start close, selection kept
        route(
            "credits_cross",
            none,
            &[
                (60, "cross", 8),
                (150, "right", 8),
                (200, "select", 8),
                (400, "cross", 8),
            ],
            &[(300, "credits", None), (500, "menu", Some(1))],
        ),
        route(
            "credits_circle",
            none,
            &[
                (60, "cross", 8),
                (150, "right", 8),
                (200, "select", 8),
                (400, "circle", 8),
            ],
            &[(300, "credits", None), (500, "menu", Some(1))],
        ),
        route(
            "credits_start",
            none,
            &[
                (60, "cross", 8),
                (150, "right", 8),
                (200, "select", 8),
                (400, "start", 8),
            ],
            &[(300, "credits", None), (500, "menu", Some(1))],
        ),
        // settings: Start opens, Circle / Start close, stick moves the row
        route(
            "settings_circle",
            none,
            &[
                (60, "cross", 8),
                (150, "right", 8),
                (200, "start", 8),
                (300, "circle", 8),
            ],
            &[(260, "settings", Some(0)), (400, "menu", Some(1))],
        ),
        route(
            "settings_start",
            none,
            &[
                (60, "cross", 8),
                (150, "right", 8),
                (200, "start", 8),
                (300, "start", 8),
            ],
            &[(260, "settings", Some(0)), (400, "menu", Some(1))],
        ),
        route(
            "settings_stick",
            none,
            &[
                (60, "cross", 8),
                (200, "start", 8),
                (260, "stick_down", 8),
                (300, "stick_down", 8),
                (340, "stick_up", 8),
            ],
            &[
                (250, "settings", Some(0)),
                (290, "settings", Some(1)),
                (330, "settings", Some(2)),
                (370, "settings", Some(1)),
            ],
        ),
        // leaving a game: pause "Quit to Menu", Select+Start, and back in again
        route(
            "c1_pause_quit",
            &LAUNCH_C1,
            &[
                (500, "start", 8),
                (560, "up", 8),
                (620, "cross", 8),
                (800, "cross", 8),
            ],
            &[
                (400, "game", None),
                (540, "pause", None),
                (720, "menu", Some(0)),
                (1000, "game", None),
            ],
        ),
        route(
            "c2_pause_quit",
            &LAUNCH_C2,
            &[
                (550, "start", 8),
                (610, "up", 8),
                (670, "cross", 8),
                (850, "cross", 8),
            ],
            &[
                (450, "game", None),
                (590, "pause", None),
                (770, "menu", Some(1)),
                (1050, "game", None),
            ],
        ),
        route(
            "c2_pause_quit_stick",
            &LAUNCH_C2,
            &[(550, "start", 8), (610, "stick_up", 8), (670, "cross", 8)],
            &[(590, "pause", None), (770, "menu", Some(1))],
        ),
        route(
            "c1_select_start",
            &LAUNCH_C1,
            &[(500, "select", 30), (504, "start", 26)],
            &[(400, "game", None), (640, "menu", Some(0))],
        ),
        route(
            "c2_select_start",
            &LAUNCH_C2,
            &[(550, "select", 30), (554, "start", 26)],
            &[(450, "game", None), (690, "menu", Some(1))],
        ),
        route(
            "c1_start_then_select",
            &LAUNCH_C1,
            &[(500, "start", 30), (504, "select", 26)],
            &[(400, "game", None), (640, "menu", Some(0))],
        ),
        route(
            "c2_pause_select_start",
            &LAUNCH_C2,
            &[(550, "start", 8), (620, "select", 30), (626, "start", 20)],
            &[(600, "pause", None), (760, "menu", Some(1))],
        ),
        route(
            "c1_pause_start_resume",
            &LAUNCH_C1,
            &[(500, "start", 8), (600, "start", 8)],
            &[(560, "pause", None), (680, "game", None)],
        ),
        route(
            "c2_pause_circle_resume",
            &LAUNCH_C2,
            &[(550, "start", 8), (650, "circle", 8)],
            &[(610, "pause", None), (730, "game", None)],
        ),
        // the reported path: back from a game, then the credits and out again
        route(
            "c1_quit_then_credits",
            &LAUNCH_C1,
            &[
                (500, "start", 8),
                (560, "up", 8),
                (620, "cross", 8),
                (800, "select", 8),
                (1000, "cross", 8),
            ],
            &[
                (720, "menu", Some(0)),
                (900, "credits", None),
                (1100, "menu", Some(0)),
            ],
        ),
        route(
            "c2_quit_then_credits",
            &LAUNCH_C2,
            &[
                (550, "select", 30),
                (554, "start", 26),
                (800, "select", 8),
                (1000, "circle", 8),
            ],
            &[
                (720, "menu", Some(1)),
                (900, "credits", None),
                (1100, "menu", Some(1)),
            ],
        ),
    ]
}

// ---- tape ------------------------------------------------------------------
fn button(name: &str) -> Option<u16> {
    Some(match name {
        "select" => 0x0001,
        "l3" => 0x0002,
        "r3" => 0x0004,
        "start" => 0x0008,
        "up" => 0x0010,
        "right" => 0x0020,
        "down" => 0x0040,
        "left" => 0x0080,
        "l2" => 0x0100,
        "r2" => 0x0200,
        "l1" => 0x0400,
        "r1" => 0x0800,
        "triangle" => 0x1000,
        "circle" => 0x2000,
        "cross" => 0x4000,
        "square" => 0x8000,
        _ => return None,
    })
}

/// A stick direction as (left-x override, left-y override).
fn stick(name: &str) -> (Option<u8>, Option<u8>) {
    match name {
        "stick_left" => (Some(0x00), None),
        "stick_right" => (Some(0xFF), None),
        "stick_up" => (None, Some(0x00)),
        "stick_down" => (None, Some(0xFF)),
        other => panic!("unknown input {other:?}"),
    }
}

/// PXITAPE1: magic, u32 count, then (u16 buttons, rx, ry, lx, ly) per tick.
fn tape_bytes(events: &[Event], length: usize) -> Vec<u8> {
    let mut samples = vec![(0u16, 0x80u8, 0x80u8, 0x80u8, 0x80u8); length];
    for &(tick, inputs, hold) in events {
        for name in inputs.split('+') {
            for sample in samples
                .iter_mut()
                .take((tick + hold).min(length))
                .skip(tick)
            {
                if let Some(bits) = button(name) {
                    sample.0 |= bits;
                } else {
                    let (lx, ly) = stick(name);
                    if let Some(lx) = lx {
                        sample.3 = lx;
                    }
                    if let Some(ly) = ly {
                        sample.4 = ly;
                    }
                }
            }
        }
    }
    let mut out = b"PXITAPE1".to_vec();
    out.extend((length as u32).to_le_bytes());
    for (buttons, rx, ry, lx, ly) in samples {
        out.extend(buttons.to_le_bytes());
        out.extend([rx, ry, lx, ly]);
    }
    out
}

// ---- screen recognition ----------------------------------------------------
const MENU_HINT_GOLD: [u8; 3] = [222, 206, 115]; // the "Menu" / "Credits" hint labels
const CREDITS_NAVY: [u8; 3] = [8, 8, 24];
const SETTINGS_BAR: [u8; 3] = [16, 16, 49]; // the highlighted settings row
const PAUSE_WHITE: [u8; 3] = [255, 247, 239]; // the pause panel's border

struct Frame {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Frame {
    /// A binary `P6` PPM with maxval 255, as the frontend writes it.
    fn parse(data: &[u8]) -> Option<Frame> {
        let mut at = 0;
        let mut token = || -> Option<&[u8]> {
            while at < data.len() && data[at].is_ascii_whitespace() {
                at += 1;
            }
            let start = at;
            while at < data.len() && !data[at].is_ascii_whitespace() {
                at += 1;
            }
            (at > start).then(|| &data[start..at])
        };
        if token()? != b"P6" {
            return None;
        }
        let number = |t: &[u8]| std::str::from_utf8(t).ok()?.parse::<usize>().ok();
        let width = number(token()?)?;
        let height = number(token()?)?;
        if number(token()?)? != 255 {
            return None;
        }
        let pixels = data.get(at + 1..)?;
        (pixels.len() == width * height * 3).then(|| Frame {
            width,
            height,
            pixels: pixels.to_vec(),
        })
    }

    fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let at = (y * self.width + x) * 3;
        [self.pixels[at], self.pixels[at + 1], self.pixels[at + 2]]
    }

    fn count(&self, (x0, y0, x1, y1): (usize, usize, usize, usize), colour: [u8; 3]) -> usize {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| self.pixel(x, y) == colour)
            .count()
    }

    fn brightness(&self, (x0, y0, x1, y1): (usize, usize, usize, usize)) -> u64 {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .map(|(x, y)| self.pixel(x, y).iter().map(|&c| u64::from(c)).sum::<u64>())
            .sum()
    }
}

type Screen = (&'static str, Option<i64>);

fn classify(frame: &Frame) -> Screen {
    if frame.count((156, 202, 200, 226), MENU_HINT_GOLD) >= 150 {
        let left = frame.brightness((52, 68, 132, 148));
        let right = frame.brightness((188, 68, 268, 148));
        return ("menu", Some(if left > right { 0 } else { 1 }));
    }
    if frame.count((0, 0, frame.width, frame.height), CREDITS_NAVY) >= 320 * 240 * 3 / 10 {
        return ("credits", None);
    }
    let bar: Vec<i64> = (60..210usize)
        .filter(|&y| {
            (60..260)
                .step_by(4)
                .filter(|&x| frame.pixel(x, y) == SETTINGS_BAR)
                .count()
                > 20
        })
        .map(|y| y as i64)
        .collect();
    if bar.len() >= 8 {
        return ("settings", Some((bar[0] + 3 - 64).div_euclid(22)));
    }
    let border = (60..180)
        .filter(|&y| frame.pixel(56, y) == PAUSE_WHITE && frame.pixel(262, y) == PAUSE_WHITE)
        .count();
    if border >= 100 {
        return ("pause", None);
    }
    ("game", None)
}

fn save_png(frame: &Frame, path: &Path) {
    let Ok(file) = fs::File::create(path) else {
        return;
    };
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        frame.width as u32,
        frame.height as u32,
    );
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    if let Ok(mut writer) = encoder.write_header() {
        let _ = writer.write_image_data(&frame.pixels);
    }
}

// ---- runner ----------------------------------------------------------------
const STEPS_PER_TICK: usize = 300_000; // measured ~240k retired instructions per route tick

struct Outcome {
    name: &'static str,
    rc: i32,
    results: Vec<(
        usize,
        &'static str,
        Option<i64>,
        (String, Option<i64>),
        bool,
    )>,
}

fn run_route(route: &Route, frontend: &Path, disc: &Path, out: &Path) -> Outcome {
    let last = route.checks.iter().map(|check| check.0).max().unwrap_or(0);
    let dir = out.join(route.name);
    let shots = dir.join("shots");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&shots).expect("create route directory");
    let tape = dir.join("route.pxtape");
    fs::write(&tape, tape_bytes(&route.events, last + 10)).expect("write tape");
    let log = fs::File::create(dir.join("run.log")).expect("create run.log");
    let rc = Command::new(frontend)
        .arg("launch")
        .arg("--path")
        .arg(disc)
        .arg("--steps")
        .arg(((last + 30) * STEPS_PER_TICK).to_string())
        .arg("--input-tape")
        .arg(&tape)
        .arg("--route-screenshot-dir")
        .arg(&shots)
        .args(["--route-screenshot-interval", "10"])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().expect("clone log")))
        .stderr(Stdio::from(log))
        .status()
        .map(|status| status.code().unwrap_or(-1))
        .unwrap_or(-1);
    let mut results = Vec::new();
    for &(tick, want, detail) in &route.checks {
        let shot = shots.join(format!("tick-{tick:06}.ppm"));
        let frame = fs::read(&shot).ok().and_then(|data| Frame::parse(&data));
        let got: (String, Option<i64>) = match &frame {
            Some(frame) => {
                let (name, detail) = classify(frame);
                (name.to_string(), detail)
            }
            None => ("missing".to_string(), None),
        };
        let ok = rc == 0 && got.0 == want && (detail.is_none() || got.1 == detail);
        results.push((tick, want, detail, got, ok));
        if let Some(frame) = &frame {
            save_png(frame, &dir.join(format!("check-{tick:06}.png")));
        }
    }
    let _ = fs::remove_dir_all(&shots); // keep only the checkpoint frames
    Outcome {
        name: route.name,
        rc,
        results,
    }
}

struct Options {
    frontend: PathBuf,
    disc: PathBuf,
    out: PathBuf,
    only: Option<Vec<String>>,
    jobs: usize,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut frontend = None;
    let mut disc = "dist/celeste-collection.cue".to_string();
    let mut out = "dist/nav-routes".to_string();
    let mut only: Option<Vec<String>> = None;
    let mut jobs = 4usize;
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = |i: &mut usize| -> Result<String, String> {
            *i += 1;
            args.get(*i).cloned().ok_or(format!("{flag} needs a value"))
        };
        match flag {
            "--frontend" => frontend = Some(value(&mut i)?),
            "--disc" => disc = value(&mut i)?,
            "--out" => out = value(&mut i)?,
            "--jobs" => {
                jobs = value(&mut i)?
                    .parse()
                    .map_err(|_| "--jobs must be an integer".to_string())?;
            }
            "--only" => {
                let names = only.get_or_insert_with(Vec::new);
                while args.get(i + 1).is_some_and(|next| !next.starts_with("--")) {
                    i += 1;
                    names.push(args[i].clone());
                }
            }
            other => return Err(format!("unrecognized argument: {other}")),
        }
        i += 1;
    }
    let frontend = frontend.ok_or("--frontend is required")?;
    Ok(Options {
        frontend: PathBuf::from(frontend),
        disc: PathBuf::from(disc),
        out: PathBuf::from(out),
        only,
        jobs: jobs.max(1),
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse_args(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("nav-routes: {message}");
            std::process::exit(2);
        }
    };
    let all = routes();
    let selected: Vec<&Route> = match &options.only {
        Some(names) if !names.is_empty() => {
            let unknown: Vec<&str> = names
                .iter()
                .map(String::as_str)
                .filter(|name| !all.iter().any(|route| route.name == *name))
                .collect();
            if !unknown.is_empty() {
                eprintln!("unknown route(s): {}", unknown.join(", "));
                std::process::exit(1);
            }
            names
                .iter()
                .map(|name| {
                    all.iter()
                        .find(|route| route.name == name)
                        .expect("checked")
                })
                .collect()
        }
        _ => all.iter().collect(),
    };
    let disc = fs::canonicalize(&options.disc)
        .unwrap_or_else(|_| std::env::current_dir().expect("cwd").join(&options.disc));
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<Outcome>>> =
        Mutex::new((0..selected.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.min(selected.len().max(1)) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::SeqCst);
                let Some(route) = selected.get(index) else {
                    break;
                };
                let outcome = run_route(route, &options.frontend, &disc, &options.out);
                slots.lock().expect("slots")[index] = Some(outcome);
            });
        }
    });
    let mut failed = 0;
    for outcome in slots.into_inner().expect("slots").into_iter().flatten() {
        let bad = outcome.rc != 0 || !outcome.results.iter().all(|r| r.4);
        failed += usize::from(bad);
        println!(
            "{}  {}{}",
            if bad { "FAIL" } else { "pass" },
            outcome.name,
            if outcome.rc != 0 {
                format!("  (frontend exit {})", outcome.rc)
            } else {
                String::new()
            }
        );
        for (tick, want, detail, got, ok) in &outcome.results {
            let expected = match detail {
                None => want.to_string(),
                Some(d) => format!("{want}[{d}]"),
            };
            let seen = match got.1 {
                None => got.0.clone(),
                Some(d) => format!("{}[{d}]", got.0),
            };
            println!(
                "      tick {tick:5}  want {expected:<12} got {seen:<12} {}",
                if *ok { "ok" } else { "MISMATCH" }
            );
        }
    }
    println!(
        "{}/{} routes passed",
        selected.len() - failed,
        selected.len()
    );
    std::process::exit(i32::from(failed != 0));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(fill: impl Fn(usize, usize) -> [u8; 3]) -> Frame {
        let mut pixels = Vec::new();
        for y in 0..240 {
            for x in 0..320 {
                pixels.extend(fill(x, y));
            }
        }
        Frame {
            width: 320,
            height: 240,
            pixels,
        }
    }

    #[test]
    fn tape_holds_buttons_and_sticks_for_their_ticks() {
        let tape = tape_bytes(&[(1, "cross+stick_right", 2), (2, "stick_down", 1)], 4);
        assert_eq!(&tape[..8], b"PXITAPE1");
        assert_eq!(u32::from_le_bytes(tape[8..12].try_into().unwrap()), 4);
        let sample = |tick: usize| &tape[12 + tick * 6..12 + tick * 6 + 6];
        assert_eq!(sample(0), [0, 0, 0x80, 0x80, 0x80, 0x80]);
        // tick 1: cross held (0x4000), stick pushed right
        assert_eq!(sample(1), [0x00, 0x40, 0x80, 0x80, 0xFF, 0x80]);
        // tick 2: cross still held, stick right still held, stick down added
        assert_eq!(sample(2), [0x00, 0x40, 0x80, 0x80, 0xFF, 0xFF]);
        assert_eq!(sample(3), [0, 0, 0x80, 0x80, 0x80, 0x80]);
    }

    #[test]
    fn every_route_has_a_checkpoint_after_its_last_input() {
        let all = routes();
        assert_eq!(all.len(), 23);
        for route in &all {
            let last_input = route.events.iter().map(|e| e.0).max().unwrap();
            let last_check = route.checks.iter().map(|c| c.0).max().unwrap();
            assert!(last_check > last_input, "{}", route.name);
        }
        let mut names: Vec<&str> = all.iter().map(|r| r.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 23);
    }

    #[test]
    fn screens_are_recognised_from_fixed_pixels() {
        let hint = |x: usize, y: usize| (156..200).contains(&x) && (202..226).contains(&y);
        let menu_right = frame(|x, y| {
            if hint(x, y) {
                MENU_HINT_GOLD
            } else if (188..268).contains(&x) && (68..148).contains(&y) {
                [90, 90, 90]
            } else {
                [0, 0, 0]
            }
        });
        assert_eq!(classify(&menu_right), ("menu", Some(1)));
        assert_eq!(classify(&frame(|_, _| CREDITS_NAVY)), ("credits", None));
        // Settings bar on the second row: rows 96..104 painted.
        let settings = frame(|x, y| {
            if (60..260).contains(&x) && (96..106).contains(&y) {
                SETTINGS_BAR
            } else {
                [0, 0, 0]
            }
        });
        assert_eq!(classify(&settings), ("settings", Some(1)));
        let pause = frame(|x, y| {
            if (x == 56 || x == 262) && (60..180).contains(&y) {
                PAUSE_WHITE
            } else {
                [0, 0, 0]
            }
        });
        assert_eq!(classify(&pause), ("pause", None));
        assert_eq!(classify(&frame(|_, _| [0, 0, 0])), ("game", None));
    }

    #[test]
    fn arguments_follow_the_script() {
        let args: Vec<String> = ["--frontend", "f", "--only", "a", "b", "--jobs", "2"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let options = parse_args(&args).unwrap();
        assert_eq!(options.only, Some(vec!["a".to_string(), "b".to_string()]));
        assert_eq!(options.jobs, 2);
        assert_eq!(options.disc, PathBuf::from("dist/celeste-collection.cue"));
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["--bogus".to_string()]).is_err());
    }
}
