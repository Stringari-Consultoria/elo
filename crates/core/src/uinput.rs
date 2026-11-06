//! Rolagem suave no Linux: um mouse virtual (uinput) com rodas de alta
//! resolução (1/120 de clique). O X11 só simula cliques inteiros da roda; por
//! aqui os apps recebem rolagem contínua, como a de um touchpad.
//!
//! Precisa de acesso ao /dev/uinput (o .deb instala uma regra do udev que
//! libera para o usuário logado). Sem acesso, fica a rolagem por cliques.

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode};

/// Unidades de alta resolução por clique da roda.
pub const PER_CLICK: i32 = 120;

/// Dá para criar o mouse virtual (permissão no /dev/uinput)?
pub fn available() -> bool {
    std::fs::OpenOptions::new().write(true).open("/dev/uinput").is_ok()
}

pub struct FineScroll {
    dev: VirtualDevice,
    /// Sobra para os eventos de clique inteiro (apps que não usam alta resolução).
    rest: [i32; 2],
}

impl FineScroll {
    pub fn open() -> std::io::Result<FineScroll> {
        let dev = VirtualDevice::builder()?
            .name("Elo (rolagem)")
            // Movimento e botões: sem eles o sistema não trata como mouse.
            .with_relative_axes(&AttributeSet::from_iter([
                RelativeAxisCode::REL_X,
                RelativeAxisCode::REL_Y,
                RelativeAxisCode::REL_WHEEL,
                RelativeAxisCode::REL_HWHEEL,
                RelativeAxisCode::REL_WHEEL_HI_RES,
                RelativeAxisCode::REL_HWHEEL_HI_RES,
            ]))?
            .with_keys(&AttributeSet::from_iter([KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT, KeyCode::BTN_MIDDLE]))?
            .build()?;
        Ok(FineScroll { dev, rest: [0; 2] })
    }

    /// `dx`, `dy` em 1/120 de clique; `dy` positivo = para cima, `dx`
    /// positivo = para a esquerda (como os botões 4 e 6 do X11).
    pub fn scroll(&mut self, dx: i32, dy: i32) -> std::io::Result<()> {
        let rel = |code: RelativeAxisCode, v: i32| InputEvent::new(EventType::RELATIVE.0, code.0, v);
        let mut ev = Vec::with_capacity(4);
        for (i, v, hi, lo) in [(1, dy, RelativeAxisCode::REL_WHEEL_HI_RES, RelativeAxisCode::REL_WHEEL), (0, -dx, RelativeAxisCode::REL_HWHEEL_HI_RES, RelativeAxisCode::REL_HWHEEL)] {
            if v == 0 {
                continue;
            }
            ev.push(rel(hi, v));
            self.rest[i] += v;
            let clicks = self.rest[i] / PER_CLICK;
            if clicks != 0 {
                ev.push(rel(lo, clicks));
                self.rest[i] -= clicks * PER_CLICK;
            }
        }
        if ev.is_empty() { Ok(()) } else { self.dev.emit(&ev) }
    }
}

#[cfg(test)]
mod tests {
    /// Com acesso ao /dev/uinput (não roda no CI):
    /// `cargo test -p elo-core -- --ignored mouse_virtual`.
    #[test]
    #[ignore]
    fn mouse_virtual_de_rolagem() {
        let mut f = super::FineScroll::open().expect("uinput");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let devices = std::fs::read_to_string("/proc/bus/input/devices").unwrap();
        assert!(devices.contains("Elo (rolagem)"), "o sistema não viu o mouse virtual");
        // Ida e volta mínimas (1/120 de clique): não mexe em nada de verdade.
        f.scroll(0, 1).unwrap();
        f.scroll(0, -1).unwrap();
        f.scroll(1, 0).unwrap();
        f.scroll(-1, 0).unwrap();
        // Rodas de alta resolução anunciadas (REL_WHEEL_HI_RES = bit 11).
        let block = devices.split("\n\n").find(|b| b.contains("Elo (rolagem)")).unwrap();
        let rel = block.lines().find(|l| l.starts_with("B: REL=")).unwrap();
        let bits = u64::from_str_radix(rel.trim_start_matches("B: REL=").trim(), 16).unwrap();
        assert!(bits & (1 << 11) != 0 && bits & (1 << 12) != 0, "sem rodas de alta resolução: {rel}");
    }
}
