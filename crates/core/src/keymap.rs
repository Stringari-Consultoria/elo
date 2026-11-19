// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only
//! Texto digitado de longe no Wayland: o GNOME (e o portal) só digitam as
//! teclas que existem no teclado configurado aqui. Uma letra que não existe
//! direto é descartada em silêncio ("No keycode found for keyval"): no
//! `us+intl` e no ABNT2, "ã", "õ", "ê" e "à" só saem por tecla morta, e
//! "não" chegava como "no".
//!
//! O Elo lê o mapa de teclado do próprio compositor (o mesmo que qualquer
//! janela recebe) e, quando a letra não existe direto, manda a tecla morta e
//! a letra sem acento, como uma pessoa digitaria.

use std::collections::HashSet;
use std::io::Read;

use wayland_client::protocol::{wl_keyboard, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};

/// Nomes das teclas (keysyms) que o teclado daqui tem.
pub(crate) struct Keymap(HashSet<String>);

#[derive(Default)]
struct State {
    seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    text: Option<String>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(state: &mut Self, registry: &wl_registry::WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, version } = event
            && interface == "wl_seat"
            && state.seat.is_none()
        {
            state.seat = Some(registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(5), qh, ()));
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(state: &mut Self, seat: &wl_seat::WlSeat, event: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(c) } = event
            && c.contains(wl_seat::Capability::Keyboard)
            && state.keyboard.is_none()
        {
            state.keyboard = Some(seat.get_keyboard(qh, ()));
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(state: &mut Self, _: &wl_keyboard::WlKeyboard, event: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_keyboard::Event::Keymap { format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1), fd, size } = event {
            let mut buf = Vec::with_capacity(size as usize);
            if std::fs::File::from(fd).take(size as u64).read_to_end(&mut buf).is_ok() {
                state.text = Some(String::from_utf8_lossy(&buf).trim_end_matches('\0').to_string());
            }
        }
    }
}

impl Keymap {
    /// O mapa de teclado atual do compositor (`None` fora do Wayland).
    pub(crate) fn current() -> Option<Keymap> {
        let conn = Connection::connect_to_env().ok()?;
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut st = State::default();
        for _ in 0..4 {
            queue.roundtrip(&mut st).ok()?;
            if st.text.is_some() {
                break;
            }
        }
        if let Some(k) = st.keyboard.take() {
            k.release();
        }
        Some(Keymap::parse(&st.text?))
    }

    /// Os nomes que aparecem na parte `xkb_symbols` do mapa.
    fn parse(text: &str) -> Keymap {
        let symbols = text.find("xkb_symbols").map_or(text, |i| &text[i..]);
        Keymap(symbols.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).map(str::to_string).collect())
    }

    fn has(&self, name: &str) -> bool {
        self.0.contains(name)
    }

    /// Teclas (keysyms) para digitar `c` aqui. `None`: manda como sempre
    /// (um keysym só); quem decide se dá é o compositor.
    pub(crate) fn plan(&self, c: char) -> Option<Vec<u32>> {
        let (base, accent) = decompose(c)?;
        let name = format!("{base}{}", accent.suffix);
        if self.has(&name) || !self.has(accent.dead) {
            return None;
        }
        Some(vec![accent.dead_sym, base as u32])
    }
}

struct Accent {
    /// Fim do nome da letra acentuada ("atilde", "ccedilla").
    suffix: &'static str,
    /// A tecla morta que faz o acento.
    dead: &'static str,
    dead_sym: u32,
}

const GRAVE: Accent = Accent { suffix: "grave", dead: "dead_grave", dead_sym: 0xfe50 };
const ACUTE: Accent = Accent { suffix: "acute", dead: "dead_acute", dead_sym: 0xfe51 };
const CIRCUMFLEX: Accent = Accent { suffix: "circumflex", dead: "dead_circumflex", dead_sym: 0xfe52 };
const TILDE: Accent = Accent { suffix: "tilde", dead: "dead_tilde", dead_sym: 0xfe53 };
const DIAERESIS: Accent = Accent { suffix: "diaeresis", dead: "dead_diaeresis", dead_sym: 0xfe57 };
const RING: Accent = Accent { suffix: "ring", dead: "dead_abovering", dead_sym: 0xfe58 };
const CEDILLA: Accent = Accent { suffix: "cedilla", dead: "dead_cedilla", dead_sym: 0xfe5b };

/// Letra acentuada do Latin-1 → letra sem acento e o acento.
fn decompose(c: char) -> Option<(char, &'static Accent)> {
    let lower = c.to_lowercase().next()?;
    let (base, accent) = match lower {
        'à' => ('a', &GRAVE),
        'è' => ('e', &GRAVE),
        'ì' => ('i', &GRAVE),
        'ò' => ('o', &GRAVE),
        'ù' => ('u', &GRAVE),
        'á' => ('a', &ACUTE),
        'é' => ('e', &ACUTE),
        'í' => ('i', &ACUTE),
        'ó' => ('o', &ACUTE),
        'ú' => ('u', &ACUTE),
        'ý' => ('y', &ACUTE),
        'â' => ('a', &CIRCUMFLEX),
        'ê' => ('e', &CIRCUMFLEX),
        'î' => ('i', &CIRCUMFLEX),
        'ô' => ('o', &CIRCUMFLEX),
        'û' => ('u', &CIRCUMFLEX),
        'ã' => ('a', &TILDE),
        'õ' => ('o', &TILDE),
        'ñ' => ('n', &TILDE),
        'ä' => ('a', &DIAERESIS),
        'ë' => ('e', &DIAERESIS),
        'ï' => ('i', &DIAERESIS),
        'ö' => ('o', &DIAERESIS),
        'ü' => ('u', &DIAERESIS),
        'ÿ' => ('y', &DIAERESIS),
        'å' => ('a', &RING),
        'ç' => ('c', &CEDILLA),
        _ => return None,
    };
    // Maiúscula: a letra base também (Ã = tecla morta + A).
    let base = if c.is_uppercase() { base.to_ascii_uppercase() } else { base };
    // (ÿ maiúscula não existe no Latin-1.)
    if c == 'Ÿ' {
        return None;
    }
    Some((base, accent))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trecho de um mapa como o do `us+intl`: "á" direto, "ã" só pela tecla morta.
    const US_INTL: &str = "xkb_keymap {\n xkb_keycodes \"evdev\" { <AC01> = 38; };\n xkb_symbols \"pc+us(intl)\" {\n key <AC01> { [ a, A, aacute, Aacute ] };\n key <AB08> { [ comma, less, ccedilla, Ccedilla ] };\n key <TLDE> { [ dead_grave, dead_tilde, grave, asciitilde ] };\n key <AC11> { [ dead_acute, dead_diaeresis, apostrophe, quotedbl ] };\n key <AE06> { [ 6, dead_circumflex, onequarter, asciicircum ] };\n };\n};\0";

    #[test]
    fn missing_letters_go_through_dead_keys() {
        let k = Keymap::parse(US_INTL);
        // Existe direto: manda como sempre.
        assert_eq!(k.plan('á'), None);
        assert_eq!(k.plan('ç'), None);
        // Só pela tecla morta.
        assert_eq!(k.plan('ã'), Some(vec![0xfe53, 'a' as u32]));
        assert_eq!(k.plan('Õ'), Some(vec![0xfe53, 'O' as u32]));
        assert_eq!(k.plan('ê'), Some(vec![0xfe52, 'e' as u32]));
        assert_eq!(k.plan('à'), Some(vec![0xfe50, 'a' as u32]));
        assert_eq!(k.plan('ü'), Some(vec![0xfe57, 'u' as u32]));
        // Sem acento, ou acento sem tecla morta aqui: como sempre.
        assert_eq!(k.plan('a'), None);
        assert_eq!(k.plan('å'), None);
        assert_eq!(k.plan('€'), None);
    }
}
