use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use image::imageops::FilterType;
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, ScaleMode, Window, WindowOptions};
use font8x8::{UnicodeFonts, BASIC_FONTS};

const VIDEO_ADDR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(192, 168, 5, 1), 58080);
const BATTERY_ADDR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(192, 168, 5, 1), 58090);
const START: [u8; 2] = [0x20, 0x36];
const STOP: [u8; 2] = [0x20, 0x37];
const BATTERY_QUERY: [u8; 2] = [0x66, 0x3a];
const LED_QUERY: [u8; 3] = [0x66, 0x3c, 0xfe]; // APK SetCameraBrightness(-2)
const LED_SET_PREFIX: [u8; 2] = [0x66, 0x3c]; // APK SetCameraBrightness(value 0..100)
const WIDTH: usize = 1000;
const HEIGHT: usize = 720;
const CAMERA_SIZE: usize = 680;
const CAMERA_X: usize = 18;
const CAMERA_Y: usize = 20;
const PANEL_X: usize = 720;
const PANEL_W: usize = 260;
const MAX_FRAME: usize = 524_288;
const MAX_PARTS: usize = 255;

#[derive(Default)]
struct FrameAssembler {
    current_id: Option<u8>,
    parts: BTreeMap<u8, Vec<u8>>,
    max_index: u8,
    started: Option<Instant>,
}
impl FrameAssembler {
    fn reset(&mut self) { self.current_id = None; self.parts.clear(); self.max_index = 0; self.started = None; }
    fn push(&mut self, packet: &[u8]) -> Option<Vec<u8>> {
        if packet.len() <= 4 || packet[2] == 0 { return None; }
        let id = packet[0];
        let index = packet[2];
        let completed = if self.current_id.is_some() && self.current_id != Some(id) {
            self.complete()
        } else { None };
        if self.current_id != Some(id) || self.started.is_some_and(|at| at.elapsed() > Duration::from_secs(2)) {
            self.reset();
            self.current_id = Some(id);
            self.started = Some(Instant::now());
        }
        if self.parts.len() < MAX_PARTS {
            self.parts.entry(index).or_insert_with(|| packet[4..].to_vec());
            self.max_index = self.max_index.max(index);
        }
        completed
    }
    fn complete(&self) -> Option<Vec<u8>> {
        if self.max_index == 0 || !self.parts.contains_key(&1) { return None; }
        let mut bytes = Vec::new();
        for idx in 1..=self.max_index {
            let part = self.parts.get(&idx)?;
            if bytes.len() + part.len() > MAX_FRAME { return None; }
            bytes.extend_from_slice(part);
        }
        if !bytes.starts_with(&[0xff, 0xd8]) { return None; }
        let end = bytes.windows(2).rposition(|s| s == [0xff, 0xd9])? + 2;
        bytes.truncate(end);
        Some(bytes)
    }
}

enum Event { Frame(Vec<u8>), Battery(Option<(u16, u16)>), Status(&'static str) }

fn video_loop(running: Arc<AtomicBool>, tx: mpsc::SyncSender<Event>, local: Ipv4Addr) {
    let socket = match UdpSocket::bind(SocketAddrV4::new(local, 0)) {
        Ok(s) => s,
        Err(e) => { eprintln!("VIDEO: WLAN-Adapter nicht erreichbar: {e}"); return; }
    };
    if let Err(e) = socket.connect(VIDEO_ADDR) { eprintln!("VIDEO: Verbindung: {e}"); return; }
    let _ = socket.set_read_timeout(Some(Duration::from_millis(150)));
    println!("VIDEO: {} -> {VIDEO_ADDR}", socket.local_addr().map(|a| a.to_string()).unwrap_or_default());
    let _ = socket.send(&START);
    let mut last_request = Instant::now();
    let mut last_valid = Instant::now();
    let mut assembler = FrameAssembler::default();
    let mut buf = [0u8; 4096];
    let mut accepted = 0u64;
    while running.load(Ordering::Relaxed) {
        match socket.recv(&mut buf) {
            Ok(len) if len > 4 => {
                if let Some(jpeg) = assembler.push(&buf[..len]) {
                    last_valid = Instant::now();
                    accepted += 1;
                    let _ = tx.try_send(Event::Frame(jpeg));
                    if accepted % 100 == 0 { println!("VIDEO: {accepted} vollstaendige Frames empfangen"); }
                }
            }
            Ok(_) => {},
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {},
            Err(e) => { eprintln!("VIDEO: Empfangsfehler: {e}"); thread::sleep(Duration::from_millis(150)); }
        }
        // No *valid frame* for 3s: restart even if malformed UDP packets keep arriving.
        if last_valid.elapsed() >= Duration::from_millis(1500) && last_request.elapsed() >= Duration::from_millis(1500) {
            println!("VIDEO: 1.5s kein vollstaendiges Bild; erneute Startanforderung");
            assembler.reset();
            let _ = socket.send(&STOP);
            thread::sleep(Duration::from_millis(100));
            let _ = socket.send(&START);
            last_request = Instant::now();
            let _ = tx.try_send(Event::Status("Warte auf Video"));
        }
    }
    let _ = socket.send(&STOP);
    println!("VIDEO: gestoppt");
}

// Original APK: 66 3A sent to UDP 58090, response big-endian i32.
// Upper 16 bits encode device flags; lower 16 bits are interpreted by Android
// as battery percent for some device statuses. Unknown statuses are not converted.
fn battery_loop(running: Arc<AtomicBool>, tx: mpsc::SyncSender<Event>, local: Ipv4Addr) {
    let socket = match UdpSocket::bind(SocketAddrV4::new(local, 0)) {
        Ok(s) => s, Err(e) => { eprintln!("AKKU: Socketfehler: {e}"); return; }
    };
    if let Err(e) = socket.connect(BATTERY_ADDR) { eprintln!("AKKU: Verbindung: {e}"); return; }
    let _ = socket.set_read_timeout(Some(Duration::from_millis(1100)));
    let mut buf = [0u8; 32];
    while running.load(Ordering::Relaxed) {
        let mut value = None;
        if socket.send(&BATTERY_QUERY).is_ok() {
            if let Ok(len) = socket.recv(&mut buf) {
                if len >= 4 {
                    value = Some((u16::from_be_bytes([buf[0], buf[1]]), u16::from_be_bytes([buf[2], buf[3]])));
                }
            }
        }
        let _ = tx.try_send(Event::Battery(value));
        for _ in 0..100 {
            if !running.load(Ordering::Relaxed) { return; }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let local: Ipv4Addr = std::env::args().nth(1).unwrap_or_else(|| "192.168.5.100".to_owned()).parse()?;
    let running = Arc::new(AtomicBool::new(true));
    let (tx, rx) = mpsc::sync_channel::<Event>(12);
    let video_run = Arc::clone(&running);
    let video_tx = tx.clone();
    let video_worker = thread::spawn(move || video_loop(video_run, video_tx, local));
    let battery_run = Arc::clone(&running);
    let battery_worker = thread::spawn(move || battery_loop(battery_run, tx, local));
    let control_socket = UdpSocket::bind(SocketAddrV4::new(local, 0))?;
    control_socket.connect(BATTERY_ADDR)?;
    control_socket.set_read_timeout(Some(Duration::from_millis(450)))?;
    let led_initial = query_led(&control_socket);
    let result = display(rx, control_socket, led_initial);
    running.store(false, Ordering::Relaxed);
    let _ = video_worker.join();
    let _ = battery_worker.join();
    result
}

// Read the current LED value without changing it. A timeout is not fatal.
fn query_led(socket: &UdpSocket) -> Option<u8> {
    socket.send(&LED_QUERY).ok()?;
    let mut reply = [0u8; 32];
    let n = socket.recv(&mut reply).ok()?;
    if n > 0 && reply[0] <= 100 { Some(reply[0]) } else { None }
}

fn rect(buf: &mut [u32], x: usize, y: usize, w: usize, h: usize, color: u32) {
    for yy in y.min(HEIGHT)..y.saturating_add(h).min(HEIGHT) {
        for xx in x.min(WIDTH)..x.saturating_add(w).min(WIDTH) {
            buf[yy * WIDTH + xx] = color;
        }
    }
}

fn label(buf: &mut [u32], x: usize, y: usize, message: &str, color: u32, scale: usize) {
    let mut cursor = x;
    for c in message.chars() {
        if let Some(glyph) = BASIC_FONTS.get(c) {
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if ((*bits >> col) & 1) == 1 {
                        rect(buf, cursor + col * scale, y + row * scale, scale, scale, color);
                    }
                }
            }
        }
        cursor += 8 * scale;
        if cursor + 8 * scale >= WIDTH { break; }
    }
}

fn button(buf: &mut [u32], x: usize, y: usize, w: usize, caption: &str, active: bool) {
    rect(buf, x, y, w, 44, if active { 0x00365770 } else { 0x00293746 });
    label(buf, x + 12, y + 14, caption, 0x00f5f8fb, 2);
}

fn inside(pos: (f32, f32), x: usize, y: usize, w: usize, h: usize) -> bool {
    pos.0 >= x as f32 && pos.0 < (x + w) as f32 && pos.1 >= y as f32 && pos.1 < (y + h) as f32
}

fn apply_led(socket: &UdpSocket, led: &mut Option<u8>, value: u8) {
    let value = value.min(100);
    let cmd = [LED_SET_PREFIX[0], LED_SET_PREFIX[1], value];
    match socket.send(&cmd) {
        Ok(3) => { *led = Some(value); println!("LED Sollwert: {value}%"); },
        Ok(n) => eprintln!("LED: nur {n} Bytes gesendet"),
        Err(e) => eprintln!("LED: Senden fehlgeschlagen: {e}"),
    }
}

fn save_photo(jpeg: &Option<Vec<u8>>) {
    let Some(jpeg) = jpeg else { println!("FOTO: Noch kein Kamerabild"); return; };
    let result = (|| -> Result<String, Box<dyn Error>> {
        fs::create_dir_all("Aufnahmen")?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let path = format!("Aufnahmen/R37R_{now}.jpg");
        fs::write(&path, jpeg)?;
        Ok(path)
    })();
    match result { Ok(path) => println!("Foto gespeichert: {path}"), Err(e) => eprintln!("Foto fehlgeschlagen: {e}") }
}

fn draw_ui(buf: &mut [u32], video: &[u32], battery: Option<(u16, u16)>, led: Option<u8>,
           fresh: bool, connected_ever: bool, fps: f32, shown: u64, invalid: u64, photo_flash: bool) {
    buf.fill(0x00101520);
    rect(buf, CAMERA_X - 2, CAMERA_Y - 2, CAMERA_SIZE + 4, CAMERA_SIZE + 4, 0x0034495c);
    for y in 0..CAMERA_SIZE {
        let dst = (y + CAMERA_Y) * WIDTH + CAMERA_X;
        let src = y * CAMERA_SIZE;
        buf[dst..dst + CAMERA_SIZE].copy_from_slice(&video[src..src + CAMERA_SIZE]);
    }
    rect(buf, PANEL_X, 20, PANEL_W, 680, 0x001d2a38);
    label(buf, PANEL_X + 16, 44, "BEBIRD R37R", 0x00e7f5ff, 2);
    label(buf, PANEL_X + 16, 84, "RUST VIEWER 0.4.1", 0x008aa5ba, 1);
    rect(buf, PANEL_X + 16, 120, 12, 12, if fresh { 0x0030d68b } else { 0x00eda944 });
    let state = if fresh { "LIVE" } else if connected_ever { "VERBINDUNG PAUSIERT" } else { "WARTE AUF VIDEO" };
    label(buf, PANEL_X + 36, 120, state, 0x00e7eff9, 1);
    label(buf, PANEL_X + 16, 153, &format!("FPS: {fps:.1}"), 0x00b7ccda, 2);
    label(buf, PANEL_X + 16, 182, &format!("Frames: {shown}"), 0x009eb3c4, 1);
    label(buf, PANEL_X + 16, 199, &format!("JPEG-Fehler: {invalid}"), 0x009eb3c4, 1);

    label(buf, PANEL_X + 16, 249, "AKKU", 0x00c8dce9, 2);
    let percent = match battery {
        Some((state, percent)) if state <= 3 && percent <= 100 => Some(percent as usize),
        _ => None,
    };
    let batt_text = percent.map(|p| format!("{p}%")).unwrap_or_else(|| "--".to_owned());
    label(buf, PANEL_X + 16, 280, &batt_text, 0x00eef6ff, 2);
    rect(buf, PANEL_X + 92, 278, 140, 22, 0x0068798b);
    rect(buf, PANEL_X + 95, 281, 134, 16, 0x00253240);
    rect(buf, PANEL_X + 232, 284, 4, 10, 0x0068798b);
    if let Some(p) = percent {
        rect(buf, PANEL_X + 95, 281, 134 * p / 100, 16,
             if p <= 20 { 0x00e86c63 } else { 0x0033c78e });
    }
    label(buf, PANEL_X + 16, 343, "LED HELLIGKEIT", 0x00c8dce9, 2);
    let led_text = led.map(|p| format!("{p}%")).unwrap_or_else(|| "UNBEKANNT".to_owned());
    label(buf, PANEL_X + 16, 378, &led_text, 0x00f9d67f, 2);
    rect(buf, PANEL_X + 16, 417, 220, 18, 0x0046596b);
    if let Some(p) = led { rect(buf, PANEL_X + 16, 417, 220 * p as usize / 100, 18, 0x00f0c767); }
    label(buf, PANEL_X + 16, 443, "0                   100", 0x008ea4b6, 1);
    button(buf, PANEL_X + 16, 478, 102, "- 10%", false);
    button(buf, PANEL_X + 134, 478, 102, "+ 10%", false);
    button(buf, PANEL_X + 16, 553, 220, if photo_flash { "FOTO OK" } else { "FOTO SPEICHERN" }, photo_flash);
    label(buf, PANEL_X + 16, 616, "S = FOTO", 0x009eb3c4, 1);
    label(buf, PANEL_X + 16, 634, "PFEILE = LED", 0x009eb3c4, 1);
    label(buf, PANEL_X + 16, 652, "ESC = BEENDEN", 0x009eb3c4, 1);
}

fn display(rx: mpsc::Receiver<Event>, control_socket: UdpSocket, led_initial: Option<u8>) -> Result<(), Box<dyn Error>> {
    let mut window = Window::new("Bebird R37R | Rust Viewer v0.4.1", WIDTH, HEIGHT, WindowOptions {
        resize: true, scale_mode: ScaleMode::AspectRatioStretch, ..WindowOptions::default()
    })?;
    window.set_target_fps(45);
    let mut pixels = vec![0x00101520u32; WIDTH * HEIGHT];
    let mut video = vec![0x001b2530u32; CAMERA_SIZE * CAMERA_SIZE];
    let mut last_jpeg: Option<Vec<u8>> = None;
    let mut shown: u64 = 0;
    let mut invalid: u64 = 0;
    let mut last_frame = Instant::now();
    let mut battery: Option<(u16, u16)> = None;
    let mut led: Option<u8> = led_initial;
    println!("LED Anfangswert: {}", led.map(|x| format!("{x}%")).unwrap_or_else(|| "unbekannt - keine Aenderung vorgenommen".to_owned()));
    let mut fps_start = Instant::now();
    let mut fps_count: u32 = 0;
    let mut fps: f32 = 0.0;
    let mut last_mouse_down = false;
    let mut last_led_action = Instant::now() - Duration::from_secs(1);
    let mut last_photo: Option<Instant> = None;
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let mut newest = None;
        while let Ok(ev) = rx.try_recv() {
            match ev { Event::Frame(jpeg) => newest = Some(jpeg), Event::Battery(v) => battery = v, Event::Status(s) => println!("STATUS: {s}") }
        }
        if let Some(frame) = newest {
            match image::load_from_memory_with_format(&frame, image::ImageFormat::Jpeg) {
                Ok(decoded) => {
                    let rgb = decoded.resize_exact(CAMERA_SIZE as u32, CAMERA_SIZE as u32, FilterType::Triangle).to_rgb8();
                    for (out, p) in video.iter_mut().zip(rgb.pixels()) {
                        *out = ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32;
                    }
                    last_frame = Instant::now();
                    shown += 1; fps_count += 1;
                    last_jpeg = Some(frame);
                }
                Err(e) => { invalid += 1; if invalid % 20 == 1 { eprintln!("JPEG ungueltig ({invalid}): {e}"); } }
            }
        }
        if fps_start.elapsed() >= Duration::from_secs(2) {
            fps = fps_count as f32 / fps_start.elapsed().as_secs_f32();
            fps_start = Instant::now(); fps_count = 0;
        }
        if window.is_key_pressed(Key::S, KeyRepeat::No) {
            save_photo(&last_jpeg); last_photo = Some(Instant::now());
        }
        let delta: i16 = if window.is_key_pressed(Key::Up, KeyRepeat::No) || window.is_key_pressed(Key::Equal, KeyRepeat::No) { 10 }
            else if window.is_key_pressed(Key::Down, KeyRepeat::No) || window.is_key_pressed(Key::Minus, KeyRepeat::No) { -10 }
            else { 0 };
        if delta != 0 {
            let next_led = ((led.unwrap_or(50) as i16 + delta).clamp(0, 100)) as u8;
            apply_led(&control_socket, &mut led, next_led);
            last_led_action = Instant::now();
        }
        let mouse_down = window.get_mouse_down(MouseButton::Left);
        if let Some(pos) = window.get_mouse_pos(MouseMode::Clamp) {
            if mouse_down && !last_mouse_down {
                if inside(pos, PANEL_X + 16, 553, 220, 44) {
                    save_photo(&last_jpeg); last_photo = Some(Instant::now());
                } else if inside(pos, PANEL_X + 16, 478, 102, 44) {
                    let next = led.unwrap_or(50).saturating_sub(10);
                    apply_led(&control_socket, &mut led, next);
                    last_led_action = Instant::now();
                } else if inside(pos, PANEL_X + 134, 478, 102, 44) {
                    let next = led.unwrap_or(50).saturating_add(10).min(100);
                    apply_led(&control_socket, &mut led, next);
                    last_led_action = Instant::now();
                }
            }
            if mouse_down && inside(pos, PANEL_X + 10, 405, 232, 45) && last_led_action.elapsed() >= Duration::from_millis(110) {
                let value = (((pos.0 - (PANEL_X + 16) as f32) / 220.0) * 100.0).round().clamp(0.0, 100.0) as u8;
                if led != Some(value) {
                    apply_led(&control_socket, &mut led, value);
                    last_led_action = Instant::now();
                }
            }
        }
        last_mouse_down = mouse_down;
        let fresh = shown > 0 && last_frame.elapsed() < Duration::from_millis(1500);
        draw_ui(&mut pixels, &video, battery, led, fresh, shown > 0, fps, shown, invalid,
                last_photo.is_some_and(|at| at.elapsed() < Duration::from_millis(750)));
        window.update_with_buffer(&pixels, WIDTH, HEIGHT)?;
    }
    println!("Fenster geschlossen. Angezeigte Bilder: {shown}, JPEG-Fehler: {invalid}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reassembles_valid_jpeg() {
        let mut a = FrameAssembler::default();
        assert!(a.push(&[9, 0, 1, 8, 0xff, 0xd8, 1, 2]).is_none());
        assert!(a.push(&[9, 0, 2, 8, 3, 4, 0xff, 0xd9]).is_none());
        assert_eq!(a.push(&[10, 0, 1, 8, 0xff, 0xd8]).unwrap(), [0xff, 0xd8, 1, 2, 3, 4, 0xff, 0xd9]);
    }
    #[test]
    fn discards_missing_packet() {
        let mut a = FrameAssembler::default();
        a.push(&[9, 0, 1, 8, 0xff, 0xd8]); a.push(&[9, 0, 3, 8, 0xff, 0xd9]);
        assert!(a.push(&[10, 0, 1, 8, 0xff, 0xd8]).is_none());
    }
    #[test]
    fn ignores_duplicated_packet() {
        let mut a = FrameAssembler::default();
        a.push(&[9, 0, 1, 8, 0xff, 0xd8, 1]);
        a.push(&[9, 0, 1, 8, 0xff, 0xd8, 1]);
        a.push(&[9, 0, 2, 8, 0xff, 0xd9]);
        assert_eq!(a.push(&[10, 0, 1, 8, 0xff, 0xd8]), Some(vec![0xff, 0xd8, 1, 0xff, 0xd9]));
    }
    #[test]
    fn discards_bad_jpeg_marker() {
        let mut a = FrameAssembler::default();
        a.push(&[9, 0, 1, 8, 0, 0]); a.push(&[9, 0, 2, 8, 0xff, 0xd9]);
        assert!(a.push(&[10, 0, 1, 8, 0xff, 0xd8]).is_none());
    }
}
