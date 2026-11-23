// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only
//! Terminal de texto do outro computador (como SSH), dentro da janela da
//! conexão: quando a imagem da tela de lá some (tela gráfica travada,
//! console de texto) ou quando a pessoa pede pelo menu.

use std::sync::mpsc::Sender;

use egui::{Align2, Color32, CornerRadius, Event, Key as K, Modifiers, Rect, Sense, Stroke, Vec2};
use elo_core::session::Cmd;
use stringari_ui::theme;

pub struct Term {
    parser: vt100::Parser,
    /// Já pediu para abrir lá.
    started: bool,
    /// Aparecendo no lugar da imagem.
    pub shown: bool,
    /// Abriu sozinho porque a imagem sumiu: volta para a imagem quando ela voltar.
    pub auto: bool,
    /// O terminal de lá acabou (motivo). Fica o que estava escrito.
    pub ended: Option<String>,
    /// Colunas e linhas atuais.
    size: (u16, u16),
}

const BG: Color32 = Color32::from_rgb(18, 18, 20);
const FG: Color32 = Color32::from_rgb(222, 222, 226);
const PAD: f32 = 10.0;

impl Term {
    pub fn new(auto: bool) -> Term {
        Term { parser: vt100::Parser::new(24, 80, 5000), started: false, shown: true, auto, ended: None, size: (0, 0) }
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.parser.process(data);
    }

    pub fn end(&mut self, why: String) {
        self.started = false;
        self.ended = Some(why);
    }

    /// Abrir de novo depois que acabou (na próxima vez que aparecer).
    pub fn reopen(&mut self) {
        self.ended = None;
        self.size = (0, 0);
        self.feed(b"\r\n");
    }
}

/// Cores do terminal: as 16 do xterm, o cubo de 216 e os 24 cinzas.
fn color(c: vt100::Color, fallback: Color32, bold: bool) -> Color32 {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 49, 49),
        (13, 188, 121),
        (229, 229, 16),
        (36, 114, 200),
        (188, 63, 188),
        (17, 168, 205),
        (229, 229, 229),
        (102, 102, 102),
        (241, 76, 76),
        (35, 209, 139),
        (245, 245, 67),
        (59, 142, 234),
        (214, 112, 214),
        (41, 184, 219),
        (255, 255, 255),
    ];
    match c {
        vt100::Color::Default => fallback,
        vt100::Color::Idx(i) => {
            // Negrito nas 8 primeiras: a versão clara (como os terminais fazem).
            let i = if bold && i < 8 { i + 8 } else { i };
            match i {
                0..=15 => {
                    let (r, g, b) = BASE[i as usize];
                    Color32::from_rgb(r, g, b)
                }
                16..=231 => {
                    let n = i - 16;
                    let v = |x: u8| if x == 0 { 0 } else { 55 + x * 40 };
                    Color32::from_rgb(v(n / 36), v((n / 6) % 6), v(n % 6))
                }
                _ => {
                    let g = 8 + (i - 232) * 10;
                    Color32::from_rgb(g, g, g)
                }
            }
        }
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// Bytes de uma tecla especial (setas, Enter...) ou de um atalho com Ctrl.
fn key_bytes(key: K, m: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let csi = |s: &str| format!("\x1b[{s}").into_bytes();
    let arrow = |c: char| if app_cursor { format!("\x1bO{c}").into_bytes() } else { format!("\x1b[{c}").into_bytes() };
    if m.ctrl && !m.alt {
        // Ctrl+letra: os códigos de controle (Ctrl+C = 3, Ctrl+D = 4...).
        let name = key.name();
        if name.len() == 1 {
            let c = name.as_bytes()[0].to_ascii_lowercase();
            if c.is_ascii_lowercase() {
                return Some(vec![c - b'a' + 1]);
            }
        }
        match key {
            K::OpenBracket => return Some(vec![0x1b]),
            K::Backslash => return Some(vec![0x1c]),
            K::CloseBracket => return Some(vec![0x1d]),
            K::Space => return Some(vec![0]),
            _ => {}
        }
    }
    Some(match key {
        K::Enter => b"\r".to_vec(),
        K::Backspace => vec![0x7f],
        K::Tab if m.shift => csi("Z"),
        K::Tab => b"\t".to_vec(),
        K::Escape => vec![0x1b],
        K::ArrowUp => arrow('A'),
        K::ArrowDown => arrow('B'),
        K::ArrowRight => arrow('C'),
        K::ArrowLeft => arrow('D'),
        K::Home => arrow('H'),
        K::End => arrow('F'),
        K::Insert => csi("2~"),
        K::Delete => csi("3~"),
        K::PageUp => csi("5~"),
        K::PageDown => csi("6~"),
        K::F1 => b"\x1bOP".to_vec(),
        K::F2 => b"\x1bOQ".to_vec(),
        K::F3 => b"\x1bOR".to_vec(),
        K::F4 => b"\x1bOS".to_vec(),
        K::F5 => csi("15~"),
        K::F6 => csi("17~"),
        K::F7 => csi("18~"),
        K::F8 => csi("19~"),
        K::F9 => csi("20~"),
        K::F10 => csi("21~"),
        K::F11 => csi("23~"),
        K::F12 => csi("24~"),
        _ => return None,
    })
}

/// Desenha o terminal em `rect` e manda para lá o que for digitado.
pub fn show(ui: &mut egui::Ui, rect: Rect, t: &mut Term, cmd: &Sender<Cmd>) {
    let font = theme::mono(13.5);
    let (cw, rh) = ui.fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
    let cols = (((rect.width() - 2.0 * PAD) / cw).floor() as u16).max(10);
    let rows = (((rect.height() - 2.0 * PAD) / rh).floor() as u16).max(2);
    if t.size != (cols, rows) {
        t.size = (cols, rows);
        t.parser.screen_mut().set_size(rows, cols);
        if t.started {
            let _ = cmd.send(Cmd::ShellResize { cols, rows });
        }
    }
    if !t.started && t.ended.is_none() {
        t.started = true;
        let _ = cmd.send(Cmd::ShellOpen { cols, rows });
    }
    let id = ui.scope_id().with("terminal-de-texto");
    let resp = ui.interact(rect, id, Sense::click());
    if resp.clicked() || ui.memory(|m| m.focused().is_none()) {
        resp.request_focus();
    }
    let focused = resp.has_focus();
    if focused {
        // Tab, setas e Esc são do terminal, não para trocar de campo.
        ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
        let events = ui.input(|i| i.events.clone());
        let mut out: Vec<u8> = Vec::new();
        let app_cursor = t.parser.screen().application_cursor();
        for e in events {
            match e {
                Event::Text(s) => out.extend_from_slice(s.as_bytes()),
                Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(b) = key_bytes(key, modifiers, app_cursor) {
                        out.extend(b);
                    }
                }
                // Num terminal, Ctrl+C e Ctrl+X são teclas, não copiar e recortar.
                Event::Copy => out.push(3),
                Event::Cut => out.push(0x18),
                Event::Paste(s) => {
                    if t.parser.screen().bracketed_paste() {
                        out.extend_from_slice(b"\x1b[200~");
                        out.extend_from_slice(s.as_bytes());
                        out.extend_from_slice(b"\x1b[201~");
                    } else {
                        out.extend_from_slice(s.replace('\n', "\r").as_bytes());
                    }
                }
                _ => {}
            }
        }
        if !out.is_empty() && t.ended.is_none() {
            // Digitou: volta para o fim do histórico.
            t.parser.screen_mut().set_scrollback(0);
            let _ = cmd.send(Cmd::ShellInput(out));
        }
    }
    // Roda do mouse: o que já passou (histórico).
    if resp.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy.abs() >= 1.0 {
            let lines = (dy / rh).round() as i64;
            let now = t.parser.screen().scrollback() as i64;
            t.parser.screen_mut().set_scrollback((now + lines).max(0) as usize);
        }
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(10), BG);
    let screen = t.parser.screen();
    let origin = rect.min + Vec2::splat(PAD);
    let at = |row: u16, col: u16| origin + Vec2::new(col as f32 * cw, row as f32 * rh);
    for row in 0..rows {
        let mut col = 0;
        while col < cols {
            let Some(first) = screen.cell(row, col) else { break };
            let style = |c: &vt100::Cell| (c.fgcolor(), c.bgcolor(), c.bold(), c.inverse());
            let st = style(first);
            let start = col;
            let mut text = String::new();
            while col < cols {
                let Some(c) = screen.cell(row, col) else { break };
                if style(c) != st {
                    break;
                }
                if !c.is_wide_continuation() {
                    let s = c.contents();
                    text.push_str(if s.is_empty() { " " } else { s });
                }
                col += 1;
            }
            let (mut fg, mut bg) = (color(st.0, FG, st.2), color(st.1, BG, false));
            if st.3 {
                std::mem::swap(&mut fg, &mut bg);
            }
            if bg != BG {
                painter.rect_filled(Rect::from_min_size(at(row, start), Vec2::new((col - start) as f32 * cw, rh)), CornerRadius::ZERO, bg);
            }
            if !text.trim().is_empty() {
                painter.text(at(row, start), Align2::LEFT_TOP, text, font.clone(), fg);
            }
        }
    }
    if !screen.hide_cursor() && screen.scrollback() == 0 && t.ended.is_none() {
        let (r, c) = screen.cursor_position();
        let cur = Rect::from_min_size(at(r, c), Vec2::new(cw, rh));
        if focused {
            painter.rect_filled(cur, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(222, 222, 226, 150));
        } else {
            painter.rect_stroke(cur, CornerRadius::ZERO, Stroke::new(1.0, FG), egui::StrokeKind::Inside);
        }
    }
    // A saída chega a qualquer momento: desenha de novo logo.
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_become_terminal_bytes() {
        let none = Modifiers::NONE;
        assert_eq!(key_bytes(K::Enter, none, false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(K::ArrowUp, none, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(K::ArrowUp, none, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes(K::C, Modifiers::CTRL, false), Some(vec![3]));
        assert_eq!(key_bytes(K::D, Modifiers::CTRL, false), Some(vec![4]));
        assert_eq!(key_bytes(K::Delete, none, false), Some(b"\x1b[3~".to_vec()));
        // Letra sem Ctrl vem pelo texto, não pela tecla.
        assert_eq!(key_bytes(K::A, none, false), None);
    }

    #[test]
    fn colors_follow_xterm() {
        assert_eq!(color(vt100::Color::Idx(1), FG, false), Color32::from_rgb(205, 49, 49));
        assert_eq!(color(vt100::Color::Idx(1), FG, true), Color32::from_rgb(241, 76, 76));
        assert_eq!(color(vt100::Color::Idx(16), FG, false), Color32::from_rgb(0, 0, 0));
        assert_eq!(color(vt100::Color::Idx(231), FG, false), Color32::from_rgb(255, 255, 255));
        assert_eq!(color(vt100::Color::Default, FG, false), FG);
    }
}
