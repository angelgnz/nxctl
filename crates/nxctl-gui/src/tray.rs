//! X11 XEmbed system tray integration, popup context menu, and embedded PNG icon loaders.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use eframe::egui;
use nxctl_core::Language;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, CreateGCAux,
    CreateWindowAux, EventMask, GrabMode, GrabStatus, ImageFormat, ImageOrder, NotifyMode,
    PropMode, StackMode, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");
const TRAY_ICON_PNG: &[u8] = include_bytes!("../assets/tray.png");

const DEFAULT_TRAY_BG: (u8, u8, u8) = (0x19, 0x19, 0x1B);
const SYSTEM_TRAY_REQUEST_DOCK: u32 = 0;

/// Popup context menu dimensions in pixels.
pub const MENU_WIDTH: u16 = 136;
pub const MENU_HEIGHT: u16 = 69;

const MENU_BG: (u8, u8, u8) = (0x19, 0x19, 0x1B); // BG_CARD
const MENU_HOVER_BG: (u8, u8, u8) = (0x23, 0x23, 0x25); // BG_CONTROL
const MENU_BORDER: (u8, u8, u8) = (0x2E, 0x2E, 0x32);
const MENU_DIVIDER: (u8, u8, u8) = (0x25, 0x25, 0x28);
const MENU_TEXT: (u8, u8, u8) = (0xA1, 0xA1, 0xA6); // TEXT_SECONDARY
const MENU_TEXT_HOVER: (u8, u8, u8) = (0xED, 0xED, 0xED); // TEXT_PRIMARY
const MENU_ACCENT: (u8, u8, u8) = (0x4E, 0xA8, 0xDE); // ACCENT
const MENU_ERR: (u8, u8, u8) = (0xE5, 0x48, 0x4D); // SEMANTIC_ERR

/// User actions triggered from the system tray icon or its context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    /// Left click: show or hide the main window.
    ToggleWindow,
    /// Context menu "Open"/"Abrir": show and focus the main window.
    OpenWindow,
    /// Scroll up: switch to the next profile.
    NextProfile,
    /// Scroll down: switch to the previous profile.
    PrevProfile,
    /// Middle click or context menu "Quit"/"Cerrar": close the application.
    Quit,
}

/// Handle to the background X11 system tray worker.
pub struct TrayHandle {
    pub rx: Receiver<TrayEvent>,
    stop: Arc<AtomicBool>,
    lang: Arc<AtomicU8>,
}

impl TrayHandle {
    /// Spawns the X11 XEmbed tray thread if a display and tray manager are available.
    pub fn spawn(ctx: egui::Context, initial_lang: Language) -> Self {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let lang = Arc::new(AtomicU8::new(encode_lang(initial_lang)));
        let stop_flag = Arc::clone(&stop);
        let lang_flag = Arc::clone(&lang);

        thread::spawn(move || {
            if let Err(err) = run_x11_tray(&tx, &ctx, &stop_flag, &lang_flag) {
                tracing::debug!("system tray disabled: {err}");
            }
        });

        Self { rx, stop, lang }
    }

    /// Updates the language used by the tray right-click context menu.
    pub fn set_language(&self, lang: Language) {
        self.lang.store(encode_lang(lang), Ordering::Relaxed);
    }
}

impl Drop for TrayHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

const fn encode_lang(lang: Language) -> u8 {
    match lang {
        Language::Es => 0,
        Language::En => 1,
    }
}

const fn decode_lang(code: u8) -> Language {
    match code {
        0 => Language::Es,
        _ => Language::En,
    }
}

/// Returns `(open_label, quit_label)` localized for the popup context menu.
pub const fn menu_labels(lang: Language) -> (&'static str, &'static str) {
    match lang {
        Language::Es => ("Abrir", "Cerrar"),
        Language::En => ("Open", "Quit"),
    }
}

/// Loads the embedded 64x64 application window icon.
pub fn load_window_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(WINDOW_ICON_PNG).ok()
}

/// Loads the embedded 64x64 transparent system tray icon.
pub fn load_tray_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(TRAY_ICON_PNG).ok()
}

/// Scales `icon` with premultiplied bilinear filtering and composites over `bg_rgb`, returning BGRA bytes.
pub fn render_bgra_zpixmap(
    icon: &egui::IconData,
    dst_w: u16,
    dst_h: u16,
    bg_rgb: (u8, u8, u8),
) -> Vec<u8> {
    if dst_w == 0 || dst_h == 0 || icon.width == 0 || icon.height == 0 {
        return Vec::new();
    }
    let expected_len = icon.width as usize * icon.height as usize * 4;
    if icon.rgba.len() < expected_len {
        return Vec::new();
    }

    let src_w = icon.width;
    let src_h = icon.height;
    let bg_r = f32::from(bg_rgb.0) / 255.0;
    let bg_g = f32::from(bg_rgb.1) / 255.0;
    let bg_b = f32::from(bg_rgb.2) / 255.0;

    let mut out = Vec::with_capacity(usize::from(dst_w) * usize::from(dst_h) * 4);

    let sample = |x: u32, y: u32| -> (f32, f32, f32, f32) {
        let idx = (y as usize * src_w as usize + x as usize) * 4;
        let a = f32::from(icon.rgba[idx + 3]) / 255.0;
        let r = (f32::from(icon.rgba[idx]) / 255.0) * a;
        let g = (f32::from(icon.rgba[idx + 1]) / 255.0) * a;
        let b = (f32::from(icon.rgba[idx + 2]) / 255.0) * a;
        (r, g, b, a)
    };

    for dy in 0..dst_h {
        let sy = (((f32::from(dy) + 0.5) * src_h as f32 / f32::from(dst_h)) - 0.5)
            .clamp(0.0, (src_h - 1) as f32);
        let y0 = sy.floor() as u32;
        let y1 = (y0 + 1).min(src_h - 1);
        let fy = sy - y0 as f32;

        for dx in 0..dst_w {
            let sx = (((f32::from(dx) + 0.5) * src_w as f32 / f32::from(dst_w)) - 0.5)
                .clamp(0.0, (src_w - 1) as f32);
            let x0 = sx.floor() as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let fx = sx - x0 as f32;

            let p00 = sample(x0, y0);
            let p10 = sample(x1, y0);
            let p01 = sample(x0, y1);
            let p11 = sample(x1, y1);

            let w00 = (1.0 - fx) * (1.0 - fy);
            let w10 = fx * (1.0 - fy);
            let w01 = (1.0 - fx) * fy;
            let w11 = fx * fy;

            let pm_r = p00.0 * w00 + p10.0 * w10 + p01.0 * w01 + p11.0 * w11;
            let pm_g = p00.1 * w00 + p10.1 * w10 + p01.1 * w01 + p11.1 * w11;
            let pm_b = p00.2 * w00 + p10.2 * w10 + p01.2 * w01 + p11.2 * w11;
            let alpha = (p00.3 * w00 + p10.3 * w10 + p01.3 * w01 + p11.3 * w11).clamp(0.0, 1.0);

            let inv_a = 1.0 - alpha;
            let r = ((pm_r + inv_a * bg_r) * 255.0).round().clamp(0.0, 255.0) as u8;
            let g = ((pm_g + inv_a * bg_g) * 255.0).round().clamp(0.0, 255.0) as u8;
            let b = ((pm_b + inv_a * bg_b) * 255.0).round().clamp(0.0, 255.0) as u8;

            out.extend_from_slice(&[b, g, r, 0xFF]);
        }
    }

    out
}

/// Returns which context-menu row (`Some(0)` for Open/Abrir, `Some(1)` for Quit/Cerrar)
/// contains `(x, y)` inside a `width x height` menu, or `None` if on the border or outside.
pub fn menu_row_at(x: i16, y: i16, width: u16, height: u16) -> Option<usize> {
    if width < 4 || height < 4 {
        return None;
    }
    let max_x = i16::try_from(width).unwrap_or(i16::MAX) - 1;
    let max_y = i16::try_from(height).unwrap_or(i16::MAX) - 1;
    if x <= 0 || x >= max_x || y <= 0 || y >= max_y {
        return None;
    }
    let mid_y = i16::try_from(height / 2).unwrap_or(i16::MAX);
    if y < mid_y { Some(0) } else { Some(1) }
}

/// Renders the two-item X11 popup context menu ("Open/Abrir" and "Quit/Cerrar") as BGRA bytes.
pub fn render_menu_bgra_zpixmap(
    width: u16,
    height: u16,
    hovered_row: Option<usize>,
    lang: Language,
) -> Vec<u8> {
    if width < 4 || height < 8 {
        return Vec::new();
    }

    let w = usize::from(width);
    let h = usize::from(height);
    let mut out = vec![0u8; w * h * 4];
    let mid_y = h / 2;

    let set_px = |buf: &mut [u8], px: usize, py: usize, rgb: (u8, u8, u8)| {
        if px < w && py < h {
            let idx = (py * w + px) * 4;
            buf[idx] = rgb.2;
            buf[idx + 1] = rgb.1;
            buf[idx + 2] = rgb.0;
            buf[idx + 3] = 0xFF;
        }
    };

    // Base fill, hover row highlight, 1px border, and central divider.
    for py in 0..h {
        for px in 0..w {
            let rgb = if px == 0 || px == w - 1 || py == 0 || py == h - 1 {
                MENU_BORDER
            } else if py == mid_y && px >= 8 && px < w.saturating_sub(8) {
                MENU_DIVIDER
            } else {
                let row_idx = if py < mid_y { 0 } else { 1 };
                if hovered_row == Some(row_idx) {
                    MENU_HOVER_BG
                } else {
                    MENU_BG
                }
            };
            set_px(&mut out, px, py, rgb);
        }
    }

    let (open_label, quit_label) = menu_labels(lang);
    let rows = [
        (0usize, 1usize, mid_y, open_label, MENU_ACCENT),
        (1usize, mid_y + 1, h - 1, quit_label, MENU_ERR),
    ];

    for (idx, row_top, row_bottom, label, accent) in rows {
        let row_h = row_bottom.saturating_sub(row_top);
        // 3px-wide indicator bar centered vertically in the row.
        let bar_top = row_top + row_h.saturating_sub(15) / 2;
        let bar_bottom = (bar_top + 15).min(row_bottom);
        for py in bar_top..bar_bottom {
            for px in 10..13 {
                set_px(&mut out, px, py, accent);
            }
        }

        let text_rgb = if hovered_row == Some(idx) {
            MENU_TEXT_HOVER
        } else {
            MENU_TEXT
        };
        let text_top = row_top + row_h.saturating_sub(14) / 2;
        draw_text_2x(&mut out, w, h, 22, text_top, label, text_rgb);
    }

    out
}

fn draw_text_2x(
    buf: &mut [u8],
    buf_w: usize,
    buf_h: usize,
    start_x: usize,
    start_y: usize,
    text: &str,
    rgb: (u8, u8, u8),
) {
    let scale = 2usize;
    let advance = 6 * scale;

    for (char_idx, ch) in text.chars().enumerate() {
        let glyph = glyph_5x7(ch);
        let gx0 = start_x + char_idx * advance;

        for (row_idx, &row_bits) in glyph.iter().enumerate() {
            for col_idx in 0..5usize {
                if (row_bits >> (4 - col_idx)) & 1 == 0 {
                    continue;
                }
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = gx0 + col_idx * scale + sx;
                        let py = start_y + row_idx * scale + sy;
                        if px < buf_w && py < buf_h {
                            let idx = (py * buf_w + px) * 4;
                            buf[idx] = rgb.2;
                            buf[idx + 1] = rgb.1;
                            buf[idx + 2] = rgb.0;
                            buf[idx + 3] = 0xFF;
                        }
                    }
                }
            }
        }
    }
}

const fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'a' => [
            0b00000, 0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111,
        ],
        'b' => [
            0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b11110,
        ],
        'e' => [
            0b00000, 0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110,
        ],
        'i' => [
            0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'n' => [
            0b00000, 0b00000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001,
        ],
        'p' => [
            0b00000, 0b00000, 0b11110, 0b10001, 0b11110, 0b10000, 0b10000,
        ],
        'r' => [
            0b00000, 0b00000, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000,
        ],
        't' => [
            0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01001, 0b00110,
        ],
        'u' => [
            0b00000, 0b00000, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101,
        ],
        _ => [0; 7],
    }
}

const fn rgb_pixel(rgb: (u8, u8, u8)) -> u32 {
    ((rgb.0 as u32) << 16) | ((rgb.1 as u32) << 8) | (rgb.2 as u32)
}

fn clamp_popup_pos(
    root_x: i16,
    root_y: i16,
    menu_w: u16,
    menu_h: u16,
    screen_w: u16,
    screen_h: u16,
) -> (i16, i16) {
    let max_x = i32::from(screen_w).saturating_sub(i32::from(menu_w)).max(0);
    let max_y = i32::from(screen_h).saturating_sub(i32::from(menu_h)).max(0);
    let desired_y = if i32::from(root_y) + 4 + i32::from(menu_h) > i32::from(screen_h) {
        i32::from(root_y) - i32::from(menu_h) - 4
    } else {
        i32::from(root_y) + 4
    };
    let x = i32::from(root_x).clamp(0, max_x) as i16;
    let y = desired_y.clamp(0, max_y) as i16;
    (x, y)
}

fn bgra_to_msb(pixels: &mut [u8]) {
    for chunk in pixels.as_chunks_mut::<4>().0 {
        chunk.reverse();
    }
}

fn run_x11_tray(
    tx: &Sender<TrayEvent>,
    ctx: &egui::Context,
    stop: &AtomicBool,
    lang: &AtomicU8,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(tray_icon) = load_tray_icon() else {
        return Ok(());
    };

    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let screen_w = screen.width_in_pixels;
    let screen_h = screen.height_in_pixels;
    let msb_first = conn.setup().image_byte_order == ImageOrder::MSB_FIRST;

    let tray_selection_name = format!("_NET_SYSTEM_TRAY_S{screen_num}");
    let atom_tray_sel = conn
        .intern_atom(false, tray_selection_name.as_bytes())?
        .reply()?
        .atom;
    let atom_opcode = conn
        .intern_atom(false, b"_NET_SYSTEM_TRAY_OPCODE")?
        .reply()?
        .atom;
    let atom_xembed_info = conn.intern_atom(false, b"_XEMBED_INFO")?.reply()?.atom;
    let atom_net_wm_name = conn.intern_atom(false, b"_NET_WM_NAME")?.reply()?.atom;
    let atom_utf8_string = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;

    let mut tray_owner = x11rb::NONE;
    for attempt in 0..4 {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        tray_owner = conn.get_selection_owner(atom_tray_sel)?.reply()?.owner;
        if tray_owner != x11rb::NONE {
            break;
        }
        if attempt < 3 {
            thread::sleep(Duration::from_millis(300));
        }
    }
    if tray_owner == x11rb::NONE {
        return Ok(());
    }

    let win = conn.generate_id()?;
    let gc = conn.generate_id()?;
    let mut width: u16 = 22;
    let mut height: u16 = 22;
    let mut depth: u8 = screen.root_depth;

    let win_aux = CreateWindowAux::new()
        .background_pixmap(1) // BackPixmap::PARENT_RELATIVE
        .event_mask(EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY | EventMask::BUTTON_PRESS);

    conn.create_window(
        x11rb::COPY_FROM_PARENT as u8,
        win,
        root,
        0,
        0,
        width,
        height,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &win_aux,
    )?;

    conn.change_property32(
        PropMode::REPLACE,
        win,
        atom_xembed_info,
        atom_xembed_info,
        &[0, 1],
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"NXCTL",
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        win,
        atom_net_wm_name,
        atom_utf8_string,
        b"NXCTL",
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        b"nxctl-gui\0Nxctl-gui\0",
    )?;

    conn.create_gc(gc, win, &CreateGCAux::new())?;

    // Override-redirect popup window for right-click context menu.
    let menu_win = conn.generate_id()?;
    let menu_gc = conn.generate_id()?;
    let menu_aux = CreateWindowAux::new()
        .background_pixel(rgb_pixel(MENU_BG))
        .border_pixel(rgb_pixel(MENU_BORDER))
        .override_redirect(1u32)
        .save_under(1u32)
        .event_mask(
            EventMask::EXPOSURE
                | EventMask::BUTTON_PRESS
                | EventMask::POINTER_MOTION
                | EventMask::LEAVE_WINDOW
                | EventMask::STRUCTURE_NOTIFY,
        );
    conn.create_window(
        x11rb::COPY_FROM_PARENT as u8,
        menu_win,
        root,
        0,
        0,
        MENU_WIDTH,
        MENU_HEIGHT,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &menu_aux,
    )?;
    let menu_depth = conn
        .get_geometry(menu_win)
        .ok()
        .and_then(|c| c.reply().ok())
        .map_or(screen.root_depth, |g| g.depth);
    conn.create_gc(menu_gc, menu_win, &CreateGCAux::new())?;

    let dock_msg = ClientMessageEvent::new(
        32,
        tray_owner,
        atom_opcode,
        [x11rb::CURRENT_TIME, SYSTEM_TRAY_REQUEST_DOCK, win, 0, 0],
    );
    conn.send_event(false, tray_owner, EventMask::NO_EVENT, dock_msg)?;
    conn.flush()?;

    let redraw = |w: u16, h: u16, d: u8| {
        if w == 0 || h == 0 {
            return;
        }
        let _ = conn.clear_area(false, win, 0, 0, w, h);
        let bg_rgb = conn
            .get_image(ImageFormat::Z_PIXMAP, win, 0, 0, 1, 1, !0)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|img| {
                if img.data.len() >= 4 {
                    if msb_first {
                        Some((img.data[1], img.data[2], img.data[3]))
                    } else {
                        Some((img.data[2], img.data[1], img.data[0]))
                    }
                } else {
                    None
                }
            })
            .unwrap_or(DEFAULT_TRAY_BG);

        let mut pixels = render_bgra_zpixmap(&tray_icon, w, h, bg_rgb);
        if pixels.is_empty() {
            return;
        }
        if msb_first {
            bgra_to_msb(&mut pixels);
        }
        let _ = conn.put_image(ImageFormat::Z_PIXMAP, win, gc, w, h, 0, 0, 0, d, &pixels);
        let _ = conn.flush();
    };

    let redraw_menu = |hovered: Option<usize>| {
        let current_lang = decode_lang(lang.load(Ordering::Relaxed));
        let mut pixels = render_menu_bgra_zpixmap(MENU_WIDTH, MENU_HEIGHT, hovered, current_lang);
        if pixels.is_empty() {
            return;
        }
        if msb_first {
            bgra_to_msb(&mut pixels);
        }
        let _ = conn.put_image(
            ImageFormat::Z_PIXMAP,
            menu_win,
            menu_gc,
            MENU_WIDTH,
            MENU_HEIGHT,
            0,
            0,
            0,
            menu_depth,
            &pixels,
        );
        let _ = conn.flush();
    };

    let mut menu_open = false;
    let mut menu_grabbed = false;
    let mut hovered_row: Option<usize> = None;

    let close_menu = |open: &mut bool, grabbed: &mut bool, hovered: &mut Option<usize>| {
        if *grabbed {
            let _ = conn.ungrab_pointer(x11rb::CURRENT_TIME);
            *grabbed = false;
        }
        if *open {
            let _ = conn.unmap_window(menu_win);
            let _ = conn.flush();
            *open = false;
        }
        *hovered = None;
    };

    while !stop.load(Ordering::Relaxed) {
        let mut had_event = false;
        while let Some(ev) = conn.poll_for_event()? {
            had_event = true;
            match ev {
                Event::ConfigureNotify(cfg) if cfg.window == win => {
                    width = cfg.width.max(1);
                    height = cfg.height.max(1);
                    if let Ok(cookie) = conn.get_geometry(win)
                        && let Ok(geom) = cookie.reply()
                    {
                        depth = geom.depth;
                    }
                    redraw(width, height, depth);
                }
                Event::Expose(exp) if exp.window == win && exp.count == 0 => {
                    redraw(width, height, depth);
                }
                Event::Expose(exp) if exp.window == menu_win && exp.count == 0 => {
                    if menu_open {
                        redraw_menu(hovered_row);
                    }
                }
                Event::MotionNotify(m) if menu_open && m.event == menu_win => {
                    let next_hover = menu_row_at(m.event_x, m.event_y, MENU_WIDTH, MENU_HEIGHT);
                    if next_hover != hovered_row {
                        hovered_row = next_hover;
                        redraw_menu(hovered_row);
                    }
                }
                Event::LeaveNotify(leave)
                    if menu_open
                        && !menu_grabbed
                        && leave.event == menu_win
                        && leave.mode == NotifyMode::NORMAL =>
                {
                    close_menu(&mut menu_open, &mut menu_grabbed, &mut hovered_row);
                }
                Event::ButtonPress(btn) if menu_open => {
                    let row = if btn.event == menu_win {
                        menu_row_at(btn.event_x, btn.event_y, MENU_WIDTH, MENU_HEIGHT)
                    } else {
                        None
                    };
                    close_menu(&mut menu_open, &mut menu_grabbed, &mut hovered_row);
                    if matches!(btn.detail, 1 | 3)
                        && let Some(row_idx) = row
                    {
                        let tray_ev = if row_idx == 0 {
                            TrayEvent::OpenWindow
                        } else {
                            TrayEvent::Quit
                        };
                        if tx.send(tray_ev).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                }
                Event::ButtonPress(btn) if btn.event == win => match btn.detail {
                    3 => {
                        let (px, py) = clamp_popup_pos(
                            btn.root_x,
                            btn.root_y,
                            MENU_WIDTH,
                            MENU_HEIGHT,
                            screen_w,
                            screen_h,
                        );
                        hovered_row = None;
                        menu_open = true;
                        let _ = conn.configure_window(
                            menu_win,
                            &ConfigureWindowAux::new()
                                .x(i32::from(px))
                                .y(i32::from(py))
                                .stack_mode(StackMode::ABOVE),
                        );
                        let _ = conn.map_window(menu_win);
                        redraw_menu(hovered_row);
                        menu_grabbed = conn
                            .grab_pointer(
                                false,
                                menu_win,
                                EventMask::BUTTON_PRESS | EventMask::POINTER_MOTION,
                                GrabMode::ASYNC,
                                GrabMode::ASYNC,
                                x11rb::NONE,
                                x11rb::NONE,
                                x11rb::CURRENT_TIME,
                            )
                            .ok()
                            .and_then(|c| c.reply().ok())
                            .is_some_and(|r| r.status == GrabStatus::SUCCESS);
                    }
                    1 | 2 | 4 | 5 => {
                        let tray_ev = match btn.detail {
                            1 => TrayEvent::ToggleWindow,
                            4 => TrayEvent::NextProfile,
                            5 => TrayEvent::PrevProfile,
                            _ => TrayEvent::Quit,
                        };
                        if tx.send(tray_ev).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                    _ => {}
                },
                Event::DestroyNotify(d) if d.window == win => {
                    close_menu(&mut menu_open, &mut menu_grabbed, &mut hovered_row);
                    return Ok(());
                }
                _ => {}
            }
        }
        if !had_event {
            thread::sleep(Duration::from_millis(50));
        }
    }

    close_menu(&mut menu_open, &mut menu_grabbed, &mut hovered_row);
    let _ = conn.free_gc(menu_gc);
    let _ = conn.destroy_window(menu_win);
    let _ = conn.free_gc(gc);
    let _ = conn.destroy_window(win);
    let _ = conn.flush();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icons_decode_with_expected_dimensions_and_alpha() {
        let app_icon = load_window_icon().expect("icon.png should decode");
        assert_eq!(app_icon.width, 64);
        assert_eq!(app_icon.height, 64);
        assert_eq!(app_icon.rgba.len(), 64 * 64 * 4);

        let tray_icon = load_tray_icon().expect("tray.png should decode");
        assert_eq!(tray_icon.width, 64);
        assert_eq!(tray_icon.height, 64);
        assert_eq!(tray_icon.rgba.len(), 64 * 64 * 4);

        // Transparent corners on tray.png so it blends into any panel background.
        assert_eq!(tray_icon.rgba[3], 0);
        let last_corner_alpha = tray_icon.rgba[(64 * 64 - 1) * 4 + 3];
        assert_eq!(last_corner_alpha, 0);

        // White opaque strokes on tray.png (near-white RGB with high alpha).
        assert!(
            tray_icon
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|px| px[0] >= 250 && px[1] >= 250 && px[2] >= 250 && px[3] > 200)
        );
    }

    #[test]
    fn render_bgra_zpixmap_scales_and_blends_cleanly() {
        let tray_icon = load_tray_icon().expect("tray.png should decode");
        let bg = (0x79, 0x86, 0xCB);

        let buf16 = render_bgra_zpixmap(&tray_icon, 16, 16, bg);
        assert_eq!(buf16.len(), 16 * 16 * 4);

        let buf21 = render_bgra_zpixmap(&tray_icon, 21, 21, bg);
        assert_eq!(buf21.len(), 21 * 21 * 4);

        // Corner pixel (0, 0) is transparent in tray.png, so BGRA must equal background (B, G, R, 0xFF).
        assert_eq!(&buf21[0..4], &[bg.2, bg.1, bg.0, 0xFF]);

        // White pixels blended over panel background produce brighter pixels than `bg`.
        assert!(
            buf21
                .as_chunks::<4>()
                .0
                .iter()
                .any(|px| px[2] > 200 && px[1] > 200 && px[0] > 220)
        );
    }

    #[test]
    fn render_bgra_zpixmap_handles_zero_dimensions() {
        let tray_icon = load_tray_icon().expect("tray.png should decode");
        assert!(render_bgra_zpixmap(&tray_icon, 0, 21, DEFAULT_TRAY_BG).is_empty());
        assert!(render_bgra_zpixmap(&tray_icon, 21, 0, DEFAULT_TRAY_BG).is_empty());
    }

    #[test]
    fn menu_row_at_distinguishes_rows_and_borders() {
        assert_eq!(menu_row_at(-1, 10, MENU_WIDTH, MENU_HEIGHT), None);
        assert_eq!(menu_row_at(0, 10, MENU_WIDTH, MENU_HEIGHT), None);
        assert_eq!(
            menu_row_at(MENU_WIDTH as i16 - 1, 10, MENU_WIDTH, MENU_HEIGHT),
            None
        );
        assert_eq!(menu_row_at(20, 0, MENU_WIDTH, MENU_HEIGHT), None);
        assert_eq!(
            menu_row_at(20, MENU_HEIGHT as i16 - 1, MENU_WIDTH, MENU_HEIGHT),
            None
        );
        assert_eq!(menu_row_at(20, 15, MENU_WIDTH, MENU_HEIGHT), Some(0));
        assert_eq!(menu_row_at(20, 50, MENU_WIDTH, MENU_HEIGHT), Some(1));
    }

    #[test]
    fn render_menu_bgra_zpixmap_renders_languages_and_hover_states() {
        for lang in [Language::Es, Language::En] {
            let base = render_menu_bgra_zpixmap(MENU_WIDTH, MENU_HEIGHT, None, lang);
            assert_eq!(
                base.len(),
                usize::from(MENU_WIDTH) * usize::from(MENU_HEIGHT) * 4
            );

            // Outer border pixel at (0, 0) equals MENU_BORDER in BGRA.
            assert_eq!(
                &base[0..4],
                &[MENU_BORDER.2, MENU_BORDER.1, MENU_BORDER.0, 0xFF]
            );

            let hover0 = render_menu_bgra_zpixmap(MENU_WIDTH, MENU_HEIGHT, Some(0), lang);
            let hover1 = render_menu_bgra_zpixmap(MENU_WIDTH, MENU_HEIGHT, Some(1), lang);
            assert_ne!(base, hover0);
            assert_ne!(base, hover1);
            assert_ne!(hover0, hover1);

            // Check accent indicator bar on row 0 and error indicator bar on row 1.
            let accent_idx = (17 * usize::from(MENU_WIDTH) + 11) * 4;
            assert_eq!(
                &base[accent_idx..accent_idx + 4],
                &[MENU_ACCENT.2, MENU_ACCENT.1, MENU_ACCENT.0, 0xFF]
            );
            let err_idx = (51 * usize::from(MENU_WIDTH) + 11) * 4;
            assert_eq!(
                &base[err_idx..err_idx + 4],
                &[MENU_ERR.2, MENU_ERR.1, MENU_ERR.0, 0xFF]
            );
        }

        let es_buf = render_menu_bgra_zpixmap(MENU_WIDTH, MENU_HEIGHT, None, Language::Es);
        let en_buf = render_menu_bgra_zpixmap(MENU_WIDTH, MENU_HEIGHT, None, Language::En);
        assert_ne!(es_buf, en_buf);
    }

    #[test]
    fn clamp_popup_pos_keeps_menu_inside_screen_bounds() {
        assert_eq!(
            clamp_popup_pos(2950, 12, MENU_WIDTH, MENU_HEIGHT, 3440, 1440),
            (2950, 16)
        );
        assert_eq!(
            clamp_popup_pos(3400, 1420, MENU_WIDTH, MENU_HEIGHT, 3440, 1440),
            (3440 - MENU_WIDTH as i16, 1420 - MENU_HEIGHT as i16 - 4)
        );
    }
}
