//! Captura de tela e injeção de mouse/teclado por sistema.
//!
//! - Linux (X11): protocolo X puro (x11rb): `GetImage` para a tela e XTest
//!   para a entrada. No Wayland, abra a sessão em "GNOME/KDE no Xorg".
//! - Windows e macOS: `xcap` (captura) e `enigo` (entrada). No macOS é preciso
//!   autorizar "Gravação de Tela" e "Acessibilidade" em Ajustes do Sistema.

use elo_proto::msg::{Input, Monitor};

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub trait Screen: Send {
    fn monitors(&mut self) -> Vec<Monitor>;
    fn capture(&mut self, monitor: u8) -> Result<Frame, String>;
    /// Cursor relativo ao monitor.
    fn cursor(&mut self, monitor: u8) -> Option<(i32, i32)>;
}

pub trait Control: Send {
    /// `origin` = canto do monitor no desktop virtual.
    fn apply(&mut self, input: &Input, monitor: u8);
}

pub fn os_name() -> String {
    match std::env::consts::OS {
        "linux" => "Linux".into(),
        "windows" => "Windows".into(),
        "macos" => "macOS".into(),
        o => o.into(),
    }
}

pub fn computer_name() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok().filter(|s| !s.is_empty()).or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string())).unwrap_or_else(|| "Computador".into())
}

#[cfg(target_os = "linux")]
pub use x11::{open_control, open_screen};

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use native::{open_control, open_screen};

#[cfg(target_os = "linux")]
mod x11 {
    use super::*;
    use elo_proto::msg::{Button, Key};
    use x11rb::connection::Connection;
    use x11rb::wrapper::ConnectionExt as _;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat};
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::rust_connection::RustConnection;

    struct Mon {
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        name: String,
        primary: bool,
    }

    pub struct X11 {
        conn: RustConnection,
        root: u32,
        mons: Vec<Mon>,
        min_kc: u8,
        max_kc: u8,
        keysyms: Vec<u32>,
        per: u8,
    }

    fn connect() -> Result<X11, String> {
        let (conn, screen) = RustConnection::connect(None).map_err(|e| format!("sem acesso ao X11 ({e}). No Wayland, entre numa sessão Xorg."))?;
        let s = &conn.setup().roots[screen];
        let (root, sw, sh) = (s.root, s.width_in_pixels, s.height_in_pixels);
        let (min_kc, max_kc) = (conn.setup().min_keycode, conn.setup().max_keycode);
        let mut x = X11 { conn, root, mons: Vec::new(), min_kc, max_kc, keysyms: Vec::new(), per: 0 };
        x.refresh_monitors(sw, sh);
        x.load_keymap();
        Ok(x)
    }

    impl X11 {
        fn refresh_monitors(&mut self, sw: u16, sh: u16) {
            let mut mons = Vec::new();
            if let Ok(c) = self.conn.randr_get_monitors(self.root, true)
                && let Ok(r) = c.reply()
            {
                for m in r.monitors {
                    let name = self.conn.get_atom_name(m.name).ok().and_then(|c| c.reply().ok()).map(|n| String::from_utf8_lossy(&n.name).into_owned()).unwrap_or_else(|| "Monitor".into());
                    mons.push(Mon { x: m.x, y: m.y, w: m.width, h: m.height, name, primary: m.primary });
                }
            }
            if mons.is_empty() {
                mons.push(Mon { x: 0, y: 0, w: sw, h: sh, name: "Tela".into(), primary: true });
            }
            self.mons = mons;
        }

        fn load_keymap(&mut self) {
            let count = self.max_kc - self.min_kc + 1;
            if let Ok(c) = self.conn.get_keyboard_mapping(self.min_kc, count)
                && let Ok(r) = c.reply()
            {
                self.per = r.keysyms_per_keycode;
                self.keysyms = r.keysyms;
            }
        }

        /// (keycode, precisa de Shift?)
        fn keycode_for(&self, keysym: u32) -> Option<(u8, bool)> {
            let per = self.per as usize;
            if per == 0 {
                return None;
            }
            for (i, chunk) in self.keysyms.chunks(per).enumerate() {
                if chunk.first() == Some(&keysym) {
                    return Some((self.min_kc + i as u8, false));
                }
                if chunk.get(1) == Some(&keysym) {
                    return Some((self.min_kc + i as u8, true));
                }
            }
            None
        }

        fn spare_keycode(&self) -> Option<u8> {
            let per = self.per as usize;
            self.keysyms.chunks(per.max(1)).enumerate().rev().find(|(_, c)| c.iter().all(|k| *k == 0)).map(|(i, _)| self.min_kc + i as u8)
        }

        fn fake(&self, kind: u8, detail: u8, x: i16, y: i16) {
            let _ = self.conn.xtest_fake_input(kind, detail, 0, self.root, x, y, 0);
        }

        fn key(&self, kc: u8, down: bool) {
            self.fake(if down { 2 } else { 3 }, kc, 0, 0);
        }

        fn tap_keysym(&mut self, ks: u32) {
            const SHIFT_L: u32 = 0xffe1;
            if let Some((kc, shift)) = self.keycode_for(ks) {
                let sk = self.keycode_for(SHIFT_L).map(|x| x.0);
                if shift && let Some(s) = sk {
                    self.key(s, true);
                }
                self.key(kc, true);
                self.key(kc, false);
                if shift && let Some(s) = sk {
                    self.key(s, false);
                }
            } else if let Some(kc) = self.spare_keycode() {
                // Caractere sem tecla (acentos de outro layout): mapeia temporariamente.
                let per = self.per as usize;
                let mut syms = vec![ks; per.max(1)];
                let _ = self.conn.change_keyboard_mapping(1, kc, per as u8, &syms);
                let _ = self.conn.sync();
                self.key(kc, true);
                self.key(kc, false);
                let _ = self.conn.sync();
                syms.iter_mut().for_each(|s| *s = 0);
                let _ = self.conn.change_keyboard_mapping(1, kc, per as u8, &syms);
                self.load_keymap();
            }
            let _ = self.conn.flush();
        }
    }

    fn keysym_of(k: Key) -> u32 {
        match k {
            Key::Enter => 0xff0d,
            Key::Tab => 0xff09,
            Key::Backspace => 0xff08,
            Key::Delete => 0xffff,
            Key::Escape => 0xff1b,
            Key::Space => 0x20,
            Key::Up => 0xff52,
            Key::Down => 0xff54,
            Key::Left => 0xff51,
            Key::Right => 0xff53,
            Key::Home => 0xff50,
            Key::End => 0xff57,
            Key::PageUp => 0xff55,
            Key::PageDown => 0xff56,
            Key::Insert => 0xff63,
            Key::F(n) => 0xffbe + (n.clamp(1, 12) as u32 - 1),
            Key::Shift => 0xffe1,
            Key::Control => 0xffe3,
            Key::Alt => 0xffe9,
            Key::Meta => 0xffeb,
            Key::Char(c) => {
                let c = c.to_ascii_lowercase();
                if (c as u32) < 0x100 { c as u32 } else { 0x0100_0000 | c as u32 }
            }
        }
    }

    pub fn char_keysym(c: char) -> u32 {
        match c {
            '\n' => 0xff0d,
            '\t' => 0xff09,
            c if (c as u32) < 0x100 => c as u32,
            c => 0x0100_0000 | c as u32,
        }
    }

    impl Screen for X11 {
        fn monitors(&mut self) -> Vec<Monitor> {
            let (w, h) = { let s = &self.conn.setup().roots[0]; (s.width_in_pixels, s.height_in_pixels) };
            self.refresh_monitors(w, h);
            self.mons.iter().enumerate().map(|(i, m)| Monitor { index: i as u8, name: m.name.clone(), width: m.w as u32, height: m.h as u32, primary: m.primary }).collect()
        }

        fn capture(&mut self, monitor: u8) -> Result<Frame, String> {
            let m = self.mons.get(monitor as usize).or(self.mons.first()).ok_or("sem monitor")?;
            let r = self.conn.get_image(ImageFormat::Z_PIXMAP, self.root, m.x, m.y, m.w, m.h, !0).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
            let (w, h) = (m.w as u32, m.h as u32);
            let px = r.data;
            if px.len() < (w * h * 4) as usize {
                return Err(format!("formato de tela não suportado (profundidade {})", r.depth));
            }
            // BGRX → RGBA.
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for (d, s) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(px.as_chunks::<4>().0) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = 255;
            }
            Ok(Frame { width: w, height: h, rgba })
        }

        fn cursor(&mut self, monitor: u8) -> Option<(i32, i32)> {
            let m = self.mons.get(monitor as usize)?;
            let r = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
            Some((r.root_x as i32 - m.x as i32, r.root_y as i32 - m.y as i32))
        }
    }

    impl Control for X11 {
        fn apply(&mut self, input: &Input, monitor: u8) {
            let (ox, oy) = self.mons.get(monitor as usize).map(|m| (m.x as i32, m.y as i32)).unwrap_or((0, 0));
            match input {
                Input::MouseMove { x, y } => self.fake(6, 0, (ox + x) as i16, (oy + y) as i16),
                Input::MouseButton { button, down } => {
                    let b = match button {
                        Button::Left => 1,
                        Button::Middle => 2,
                        Button::Right => 3,
                    };
                    self.fake(if *down { 4 } else { 5 }, b, 0, 0);
                }
                Input::Scroll { dx, dy } => {
                    let clicks = |v: i32, neg: u8, pos: u8| (0..v.unsigned_abs().min(20)).map(move |_| if v < 0 { neg } else { pos });
                    for b in clicks(*dy, 5, 4).chain(clicks(*dx, 7, 6)) {
                        self.fake(4, b, 0, 0);
                        self.fake(5, b, 0, 0);
                    }
                }
                Input::Key { key, down } => {
                    if let Some((kc, _)) = self.keycode_for(keysym_of(*key)) {
                        self.key(kc, *down);
                    }
                }
                Input::Text(t) => {
                    for c in t.chars() {
                        self.tap_keysym(char_keysym(c));
                    }
                }
                Input::SecureAttention => {
                    // Ctrl+Alt+Del não tem efeito padrão no X11; envia Ctrl+Alt+Delete mesmo assim.
                    for k in [Key::Control, Key::Alt, Key::Delete] {
                        self.apply(&Input::Key { key: k, down: true }, monitor);
                    }
                    for k in [Key::Delete, Key::Alt, Key::Control] {
                        self.apply(&Input::Key { key: k, down: false }, monitor);
                    }
                }
            }
            let _ = self.conn.flush();
        }
    }

    pub fn open_screen() -> Result<Box<dyn Screen>, String> {
        Ok(Box::new(connect()?))
    }

    pub fn open_control() -> Result<Box<dyn Control>, String> {
        Ok(Box::new(connect()?))
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod native {
    use super::*;
    use elo_proto::msg::{Button, Key};
    use enigo::{Axis, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};

    pub struct Cap {
        mons: Vec<xcap::Monitor>,
    }

    impl Screen for Cap {
        fn monitors(&mut self) -> Vec<Monitor> {
            self.mons = xcap::Monitor::all().unwrap_or_default();
            self.mons
                .iter()
                .enumerate()
                .map(|(i, m)| Monitor { index: i as u8, name: m.name().unwrap_or_default(), width: m.width().unwrap_or(0), height: m.height().unwrap_or(0), primary: m.is_primary().unwrap_or(false) })
                .collect()
        }

        fn capture(&mut self, monitor: u8) -> Result<Frame, String> {
            if self.mons.is_empty() {
                self.monitors();
            }
            let m = self.mons.get(monitor as usize).or(self.mons.first()).ok_or("sem monitor")?;
            let img = m.capture_image().map_err(|e| e.to_string())?;
            Ok(Frame { width: img.width(), height: img.height(), rgba: img.into_raw() })
        }

        fn cursor(&mut self, monitor: u8) -> Option<(i32, i32)> {
            let m = self.mons.get(monitor as usize)?;
            let e = Enigo::new(&Settings::default()).ok()?;
            let (x, y) = e.location().ok()?;
            Some((x - m.x().unwrap_or(0), y - m.y().unwrap_or(0)))
        }
    }

    pub struct Ctl {
        e: Enigo,
        origins: Vec<(i32, i32)>,
    }

    fn key_of(k: Key) -> enigo::Key {
        use enigo::Key as K;
        match k {
            Key::Enter => K::Return,
            Key::Tab => K::Tab,
            Key::Backspace => K::Backspace,
            Key::Delete => K::Delete,
            Key::Escape => K::Escape,
            Key::Space => K::Space,
            Key::Up => K::UpArrow,
            Key::Down => K::DownArrow,
            Key::Left => K::LeftArrow,
            Key::Right => K::RightArrow,
            Key::Home => K::Home,
            Key::End => K::End,
            Key::PageUp => K::PageUp,
            Key::PageDown => K::PageDown,
            #[cfg(target_os = "windows")]
            Key::Insert => K::Insert,
            #[cfg(not(target_os = "windows"))]
            Key::Insert => K::Help,
            Key::F(n) => match n {
                1 => K::F1,
                2 => K::F2,
                3 => K::F3,
                4 => K::F4,
                5 => K::F5,
                6 => K::F6,
                7 => K::F7,
                8 => K::F8,
                9 => K::F9,
                10 => K::F10,
                11 => K::F11,
                _ => K::F12,
            },
            Key::Shift => K::Shift,
            Key::Control => K::Control,
            Key::Alt => K::Alt,
            Key::Meta => K::Meta,
            Key::Char(c) => K::Unicode(c.to_ascii_lowercase()),
        }
    }

    impl Control for Ctl {
        fn apply(&mut self, input: &Input, monitor: u8) {
            let (ox, oy) = self.origins.get(monitor as usize).copied().unwrap_or((0, 0));
            let _ = match input {
                Input::MouseMove { x, y } => self.e.move_mouse(ox + x, oy + y, Coordinate::Abs),
                Input::MouseButton { button, down } => {
                    let b = match button {
                        Button::Left => enigo::Button::Left,
                        Button::Right => enigo::Button::Right,
                        Button::Middle => enigo::Button::Middle,
                    };
                    self.e.button(b, if *down { Direction::Press } else { Direction::Release })
                }
                Input::Scroll { dx, dy } => {
                    let _ = self.e.scroll(-*dy, Axis::Vertical);
                    self.e.scroll(*dx, Axis::Horizontal)
                }
                Input::Key { key, down } => self.e.key(key_of(*key), if *down { Direction::Press } else { Direction::Release }),
                Input::Text(t) => self.e.text(t),
                Input::SecureAttention => {
                    // No Windows, Ctrl+Alt+Del verdadeiro exige serviço do sistema (SAS); aqui só os atalhos.
                    let _ = self.e.key(enigo::Key::Control, Direction::Press);
                    let _ = self.e.key(enigo::Key::Alt, Direction::Press);
                    let _ = self.e.key(enigo::Key::Delete, Direction::Click);
                    let _ = self.e.key(enigo::Key::Alt, Direction::Release);
                    self.e.key(enigo::Key::Control, Direction::Release)
                }
            };
        }
    }

    pub fn open_screen() -> Result<Box<dyn Screen>, String> {
        let mons = xcap::Monitor::all().map_err(|e| e.to_string())?;
        Ok(Box::new(Cap { mons }))
    }

    pub fn open_control() -> Result<Box<dyn Control>, String> {
        let origins = xcap::Monitor::all().unwrap_or_default().iter().map(|m| (m.x().unwrap_or(0), m.y().unwrap_or(0))).collect();
        Ok(Box::new(Ctl { e: Enigo::new(&Settings::default()).map_err(|e| e.to_string())?, origins }))
    }
}

/// Área de transferência (texto).
pub struct Clip {
    cb: Option<arboard::Clipboard>,
    last: String,
}

impl Default for Clip {
    fn default() -> Self {
        Self::new()
    }
}

impl Clip {
    pub fn new() -> Self {
        let mut c = Self { cb: arboard::Clipboard::new().ok(), last: String::new() };
        c.last = c.get().unwrap_or_default();
        c
    }

    pub fn get(&mut self) -> Option<String> {
        self.cb.as_mut()?.get_text().ok()
    }

    /// Texto novo copiado localmente desde a última verificação.
    pub fn changed(&mut self) -> Option<String> {
        let t = self.get()?;
        if t != self.last && !t.is_empty() {
            self.last = t.clone();
            return Some(t);
        }
        None
    }

    pub fn set(&mut self, t: &str) {
        self.last = t.to_string();
        if let Some(c) = self.cb.as_mut() {
            let _ = c.set_text(t.to_string());
        }
    }
}
