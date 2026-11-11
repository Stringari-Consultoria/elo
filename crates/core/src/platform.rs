//! Captura de tela e injeção de mouse/teclado por sistema.
//!
//! - Linux (X11): protocolo X puro (x11rb): `GetImage` para a tela e XTest
//!   para a entrada.
//! - Linux (Wayland): portal do sistema (RemoteDesktop + ScreenCast) e
//!   PipeWire; ver `wayland.rs`.
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

    /// Descarta o que ainda está na fila para ser aplicado (fim da sessão).
    fn discard_pending(&mut self) {}

    /// Espera o sistema aplicar o que já foi enviado (a conexão vai fechar).
    fn settle(&mut self) {}
}

/// Este computador, como anfitrião, sabe rolar suave (1/120 de clique)?
pub fn fine_scroll() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Wayland: rolagem suave pelo portal; X11: pelo mouse virtual (uinput).
        crate::wayland::is_wayland() || crate::uinput::available()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

pub fn os_name() -> String {
    match std::env::consts::OS {
        "linux" => "Linux".into(),
        "windows" => "Windows".into(),
        "macos" => "macOS".into(),
        o => o.into(),
    }
}

/// Endereço da placa de rede principal ("aa:bb:cc:dd:ee:ff"), para o Wake-on-LAN.
pub fn mac_address() -> String {
    mac_address::get_mac_address().ok().flatten().map(|m| m.to_string().to_lowercase()).unwrap_or_default()
}

/// Reinicia o computador (pedido de quem acessa, com permissão de controle).
pub fn restart_computer() -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("shutdown");
        c.args(["/r", "/t", "5", "/c", "Reiniciado pelo Elo"]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("osascript");
        c.args(["-e", "tell application \"System Events\" to restart"]);
        c
    } else {
        let mut c = std::process::Command::new("systemctl");
        c.arg("reboot");
        c
    };
    let st = cmd.status().map_err(|e| format!("não foi possível reiniciar: {e}"))?;
    if st.success() { Ok(()) } else { Err(format!("não foi possível reiniciar ({st})")) }
}

/// Abre o terminal na tela deste computador, na pasta pessoal (pedido de quem
/// acessa, com permissão de controle): Prompt de Comando no Windows,
/// Terminal no macOS e o terminal da área de trabalho no Linux.
pub fn open_terminal() -> Result<(), String> {
    let home = std::env::var_os(if cfg!(target_os = "windows") { "USERPROFILE" } else { "HOME" }).map(std::path::PathBuf::from).filter(|h| h.is_dir());
    let mut cmd = terminal_command()?;
    if let Some(h) = &home {
        cmd.current_dir(h);
    }
    let mut child = cmd.spawn().map_err(|e| format!("não foi possível abrir o terminal: {e}"))?;
    // Recolhe o processo quando fechar (sem deixar processo zumbi).
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(target_os = "windows")]
fn terminal_command() -> Result<std::process::Command, String> {
    use std::os::windows::process::CommandExt;
    // Janela própria: o Elo não tem console.
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let mut c = std::process::Command::new("cmd.exe");
    c.creation_flags(CREATE_NEW_CONSOLE);
    Ok(c)
}

#[cfg(target_os = "macos")]
fn terminal_command() -> Result<std::process::Command, String> {
    // Janela nova do Terminal na pasta pessoal.
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let mut c = std::process::Command::new("open");
    c.args(["-a", "Terminal", &home]);
    Ok(c)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn terminal_command() -> Result<std::process::Command, String> {
    // O escolhido pelo usuário ($TERMINAL), o padrão do Debian/Ubuntu e os das
    // áreas de trabalho mais comuns, nesta ordem.
    const KNOWN: &[&str] = &[
        "x-terminal-emulator", "gnome-terminal", "kgx", "ptyxis", "konsole", "xfce4-terminal", "mate-terminal", "lxterminal", "qterminal", "tilix", "terminator", "alacritty", "kitty", "foot", "wezterm", "xterm",
    ];
    let chosen = std::env::var("TERMINAL").ok().filter(|t| !t.trim().is_empty() && !t.contains(char::is_whitespace));
    let path = std::env::var_os("PATH").unwrap_or_default();
    let find = |name: &str| -> Option<std::path::PathBuf> {
        if name.contains('/') {
            return Some(std::path::PathBuf::from(name)).filter(|p| p.is_file());
        }
        std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
    };
    chosen.iter().map(String::as_str).chain(KNOWN.iter().copied()).find_map(find).map(std::process::Command::new).ok_or_else(|| "nenhum terminal encontrado neste computador".to_string())
}

pub fn computer_name() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok().filter(|s| !s.is_empty()).or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string())).unwrap_or_else(|| "Computador".into())
}

#[cfg(target_os = "linux")]
pub(crate) use x11::{char_keysym, keysym_of};

/// Captura de tela: portal (Wayland) ou X11.
#[cfg(target_os = "linux")]
pub fn open_screen() -> Result<Box<dyn Screen>, String> {
    if crate::wayland::is_wayland() {
        return crate::wayland::open_screen();
    }
    x11::open_screen()
}

/// Mouse e teclado: portal (Wayland) ou XTest (X11).
#[cfg(target_os = "linux")]
pub fn open_control() -> Result<Box<dyn Control>, String> {
    if crate::wayland::is_wayland() {
        return crate::wayland::open_control();
    }
    x11::open_control()
}

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
        /// Rolagem suave (uinput), se houver permissão.
        fine: Option<crate::uinput::FineScroll>,
        /// Sobra da rolagem fina quando vira cliques (sem uinput).
        fine_rest: [i32; 2],
    }

    fn connect() -> Result<X11, String> {
        let (conn, screen) = RustConnection::connect(None).map_err(|e| format!("sem acesso ao X11 ({e}). No Wayland, entre numa sessão Xorg."))?;
        let s = &conn.setup().roots[screen];
        let (root, sw, sh) = (s.root, s.width_in_pixels, s.height_in_pixels);
        let (min_kc, max_kc) = (conn.setup().min_keycode, conn.setup().max_keycode);
        let fine = if crate::uinput::available() { crate::uinput::FineScroll::open().ok() } else { None };
        let mut x = X11 { conn, root, mons: Vec::new(), min_kc, max_kc, keysyms: Vec::new(), per: 0, fine, fine_rest: [0; 2] };
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

    pub(crate) fn keysym_of(k: Key) -> u32 {
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

    pub(crate) fn char_keysym(c: char) -> u32 {
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

    impl X11 {
        /// Cliques da roda pelo XTest (botões 4/5 e 6/7).
        fn scroll_clicks(&mut self, dx: i32, dy: i32) {
            let clicks = |v: i32, neg: u8, pos: u8| (0..v.unsigned_abs().min(20)).map(move |_| if v < 0 { neg } else { pos });
            for b in clicks(dy, 5, 4).chain(clicks(dx, 7, 6)) {
                self.fake(4, b, 0, 0);
                self.fake(5, b, 0, 0);
            }
        }
    }

    impl Control for X11 {
        /// O servidor X descarta o que ainda não leu quando a conexão fecha:
        /// sem esta volta completa, as teclas soltas no fim da sessão se perdiam.
        fn settle(&mut self) {
            let _ = self.conn.sync();
        }

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
                Input::Scroll { dx, dy } => self.scroll_clicks(*dx, *dy),
                Input::ScrollFine { dx, dy } => {
                    let ok = self.fine.as_mut().is_some_and(|f| f.scroll(*dx, *dy).is_ok());
                    if !ok {
                        // Sem uinput: junta em cliques inteiros.
                        self.fine_rest[0] += dx;
                        self.fine_rest[1] += dy;
                        let (cx, cy) = (self.fine_rest[0] / crate::uinput::PER_CLICK, self.fine_rest[1] / crate::uinput::PER_CLICK);
                        self.fine_rest[0] -= cx * crate::uinput::PER_CLICK;
                        self.fine_rest[1] -= cy * crate::uinput::PER_CLICK;
                        self.scroll_clicks(cx, cy);
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

    /// Guarda só a posição dos monitores: o `xcap::Monitor` tem um ponteiro do
    /// sistema e não é `Send` no Windows, então é buscado a cada captura.
    pub struct Cap {
        origins: Vec<(i32, i32)>,
    }

    impl Screen for Cap {
        fn monitors(&mut self) -> Vec<Monitor> {
            let mons = xcap::Monitor::all().unwrap_or_default();
            self.origins = mons.iter().map(|m| (m.x().unwrap_or(0), m.y().unwrap_or(0))).collect();
            mons.iter()
                .enumerate()
                .map(|(i, m)| Monitor { index: i as u8, name: m.name().unwrap_or_default(), width: m.width().unwrap_or(0), height: m.height().unwrap_or(0), primary: m.is_primary().unwrap_or(false) })
                .collect()
        }

        fn capture(&mut self, monitor: u8) -> Result<Frame, String> {
            let mons = xcap::Monitor::all().map_err(|e| e.to_string())?;
            let m = mons.get(monitor as usize).or(mons.first()).ok_or("sem monitor")?;
            let img = m.capture_image().map_err(|e| e.to_string())?;
            Ok(Frame { width: img.width(), height: img.height(), rgba: img.into_raw() })
        }

        fn cursor(&mut self, monitor: u8) -> Option<(i32, i32)> {
            if self.origins.is_empty() {
                self.monitors();
            }
            let (ox, oy) = *self.origins.get(monitor as usize)?;
            let e = Enigo::new(&Settings::default()).ok()?;
            let (x, y) = e.location().ok()?;
            Some((x - ox, y - oy))
        }
    }

    pub struct Ctl {
        e: Enigo,
        origins: Vec<(i32, i32)>,
        /// Sobra da rolagem fina (vira cliques).
        fine_rest: [i32; 2],
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
                Input::ScrollFine { dx, dy } => {
                    self.fine_rest[0] += dx;
                    self.fine_rest[1] += dy;
                    let (cx, cy) = (self.fine_rest[0] / 120, self.fine_rest[1] / 120);
                    self.fine_rest[0] -= cx * 120;
                    self.fine_rest[1] -= cy * 120;
                    let _ = self.e.scroll(-cy, Axis::Vertical);
                    self.e.scroll(cx, Axis::Horizontal)
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
        let origins = xcap::Monitor::all().map_err(|e| e.to_string())?.iter().map(|m| (m.x().unwrap_or(0), m.y().unwrap_or(0))).collect();
        Ok(Box::new(Cap { origins }))
    }

    pub fn open_control() -> Result<Box<dyn Control>, String> {
        let origins = xcap::Monitor::all().unwrap_or_default().iter().map(|m| (m.x().unwrap_or(0), m.y().unwrap_or(0))).collect();
        Ok(Box::new(Ctl { e: Enigo::new(&Settings::default()).map_err(|e| e.to_string())?, origins, fine_rest: [0; 2] }))
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

    /// `t` acabou de ser lido daqui e o outro lado ainda não o tem? Marca como
    /// enviado. (O egui troca CRLF por LF ao ler.)
    pub fn note_sent(&mut self, t: &str) -> bool {
        if t.is_empty() || self.last.replace("\r\n", "\n") == t {
            return false;
        }
        self.last = t.to_string();
        true
    }

    pub fn set(&mut self, t: &str) {
        self.last = t.to_string();
        if let Some(c) = self.cb.as_mut() {
            let _ = c.set_text(t.to_string());
        }
    }
}
