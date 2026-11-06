//! Ícone na bandeja do sistema (área de notificação / barra de menus): o Elo
//! continua rodando com a janela fechada. Menu: seu ID, abrir, copiar ID, sair.
//!
//! No Linux usa StatusNotifierItem (D-Bus, sem GTK): funciona no KDE e no
//! GNOME com a extensão AppIndicator (padrão no Ubuntu). Se a bandeja não
//! estiver disponível, `create` devolve `None` e fechar a janela encerra o app.

use std::sync::mpsc::{Receiver, Sender, channel};

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum Act {
    Open,
    CopyId,
    Quit,
}

pub struct Tray {
    _icon: TrayIcon,
    id_item: MenuItem,
    shown_id: String,
    pub rx: Receiver<Act>,
}

fn icon() -> Option<tray_icon::Icon> {
    let img = image::load_from_memory(include_bytes!("../../../packaging/icons/elo-256.png")).ok()?.resize(64, 64, image::imageops::FilterType::Lanczos3).to_rgba8();
    let (w, h) = img.dimensions();
    tray_icon::Icon::from_rgba(img.into_raw(), w, h).ok()
}

pub fn create(ctx: &egui::Context) -> Option<Tray> {
    let menu = Menu::new();
    let id_item = MenuItem::new(t!("Seu ID: —"), false, None);
    let open = MenuItem::new(t!("Abrir o {}", crate::variant::app_name()), true, None);
    let copy = MenuItem::new(t!("Copiar ID"), true, None);
    let quit = MenuItem::new(t!("Sair do {}", crate::variant::app_name()), true, None);
    menu.append_items(&[&id_item, &PredefinedMenuItem::separator(), &open, &copy, &PredefinedMenuItem::separator(), &quit]).ok()?;
    let icon = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip(crate::variant::app_name()).with_icon(icon()?).build().ok()?;

    // Os eventos chegam em outra thread: repassa e acorda o app (que roda
    // `logic` mesmo com a janela escondida).
    let (tx, rx) = channel::<Act>();
    let ids: [(MenuId, fn() -> Act); 3] = [(open.id().clone(), || Act::Open), (copy.id().clone(), || Act::CopyId), (quit.id().clone(), || Act::Quit)];
    let (c, t): (egui::Context, Sender<Act>) = (ctx.clone(), tx.clone());
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if let Some((_, f)) = ids.iter().find(|(id, _)| *id == e.id) {
            let _ = t.send(f());
            c.request_repaint();
        }
    }));
    let c = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        // Clique esquerdo no ícone abre a janela (o direito mostra o menu).
        let open = matches!(e, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. });
        if open {
            let _ = tx.send(Act::Open);
            c.request_repaint();
        }
    }));
    Some(Tray { _icon: icon, id_item, shown_id: String::new(), rx })
}

impl Tray {
    /// Mostra o ID atual no menu.
    pub fn set_id(&mut self, id: Option<&str>) {
        let text = match id {
            Some(id) => t!("Seu ID: {}", elo_proto::ctrl::pretty_id(id)),
            None => "Seu ID: —".to_string(),
        };
        if text != self.shown_id {
            self.id_item.set_text(&text);
            self.shown_id = text;
        }
    }
}
