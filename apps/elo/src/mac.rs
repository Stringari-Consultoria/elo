//! Peças no estilo do macOS usadas na tela da sessão: cores do sistema,
//! botões em cápsula, ícones em quadrado arredondado e alertas.

use egui::{Align2, Color32, CornerRadius, Margin, Painter, Rect, Sense, Stroke, Vec2};
use stringari_ui::icons::{self, Icon};
use stringari_ui::theme;

#[derive(Clone, Copy)]
pub struct Mac {
    pub dark: bool,
    pub sep: Color32,
    pub label: Color32,
    pub label2: Color32,
    pub label3: Color32,
    pub accent: Color32,
    pub green: Color32,
    pub orange: Color32,
    pub red: Color32,
    pub hover: Color32,
    /// Balão de mensagem do outro lado / campos.
    pub fill: Color32,
    pub button: Color32,
    /// Painéis, alertas e menus.
    pub raised: Color32,
}

pub const LIGHT: Mac = Mac {
    dark: false,
    sep: Color32::from_rgba_premultiplied(0, 0, 0, 22),
    label: Color32::from_rgb(29, 29, 31),
    label2: Color32::from_rgb(110, 110, 115),
    label3: Color32::from_rgb(161, 161, 166),
    accent: Color32::from_rgb(0, 122, 255),
    green: Color32::from_rgb(52, 199, 89),
    orange: Color32::from_rgb(255, 149, 0),
    red: Color32::from_rgb(255, 59, 48),
    hover: Color32::from_rgba_premultiplied(0, 0, 0, 10),
    fill: Color32::from_rgb(233, 233, 235),
    button: Color32::WHITE,
    raised: Color32::from_rgb(246, 246, 248),
};

pub const DARK: Mac = Mac {
    dark: true,
    sep: Color32::from_rgba_premultiplied(22, 22, 22, 22),
    label: Color32::from_rgb(245, 245, 247),
    label2: Color32::from_rgb(152, 152, 157),
    label3: Color32::from_rgb(99, 99, 102),
    accent: Color32::from_rgb(10, 132, 255),
    green: Color32::from_rgb(48, 209, 88),
    orange: Color32::from_rgb(255, 159, 10),
    red: Color32::from_rgb(255, 69, 58),
    hover: Color32::from_rgba_premultiplied(14, 14, 14, 14),
    fill: Color32::from_rgb(58, 58, 60),
    button: Color32::from_rgb(72, 72, 75),
    raised: Color32::from_rgb(40, 40, 43),
};

pub fn of(dark: bool) -> Mac {
    if dark { DARK } else { LIGHT }
}

/// Quadrado arredondado colorido com o desenho em branco (como os ícones
/// dos Ajustes do Sistema).
pub fn icon_tile(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let r = (rect.width() * 0.26) as u8;
    painter.rect_filled(rect, CornerRadius::same(r), color);
    let top = Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.center().y));
    painter.rect_filled(top, CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, Color32::from_white_alpha(22));
    icons::paint(painter, rect.shrink(rect.width() * 0.22), icon, Color32::WHITE);
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Primary,
    Secondary,
    Destructive,
}

/// Botão em cápsula. `size`: `None` = do tamanho do texto.
pub fn button(ui: &mut egui::Ui, m: &Mac, text: &str, kind: Kind, size: Option<Vec2>, enabled: bool) -> egui::Response {
    let font = theme::medium(13.0);
    let g = ui.painter().layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE);
    let size = size.unwrap_or(Vec2::new(g.size().x + 30.0, 30.0));
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let hover = ui.ctx().animate_bool_with_time(resp.id, enabled && resp.hovered(), 0.1);
    let radius = CornerRadius::same((size.y / 2.0) as u8);
    let (fill, text_c, stroke) = match kind {
        Kind::Primary => (m.accent, Color32::WHITE, Stroke::NONE),
        Kind::Destructive => (m.red, Color32::WHITE, Stroke::NONE),
        Kind::Secondary => (m.button, m.label, Stroke::new(1.0, m.sep)),
    };
    let fill = fill.lerp_to_gamma(if m.dark || kind != Kind::Secondary { Color32::WHITE } else { Color32::from_gray(225) }, hover * 0.12);
    let fill = if enabled && resp.is_pointer_button_down_on() { fill.lerp_to_gamma(Color32::BLACK, 0.12) } else { fill };
    let p = ui.painter();
    if !m.dark {
        p.add(egui::Shadow { offset: [0, 1], blur: 2, spread: 0, color: Color32::from_black_alpha(22) }.as_shape(rect, radius));
    }
    p.rect(rect, radius, if enabled { fill } else { fill.gamma_multiply(0.5) }, stroke, egui::StrokeKind::Inside);
    if kind != Kind::Secondary {
        p.line_segment([rect.left_top() + Vec2::new(size.y / 2.0, 1.0), rect.right_top() + Vec2::new(-size.y / 2.0, 1.0)], Stroke::new(1.0, Color32::from_white_alpha(40)));
    }
    p.text(rect.center(), Align2::CENTER_CENTER, text, font, if enabled { text_c } else { text_c.gamma_multiply(0.5) });
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Botão redondo só de ícone.
pub fn round_icon(ui: &mut egui::Ui, m: &Mac, icon: Icon, tip: &str, size: f32, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), if enabled { Sense::click() } else { Sense::hover() });
    let hover = ui.ctx().animate_bool_with_time(resp.id, enabled && resp.hovered(), 0.1);
    ui.painter().circle_filled(rect.center(), size / 2.0, m.hover.gamma_multiply(0.6 + hover * 1.4));
    let c = if enabled { m.label2.lerp_to_gamma(m.label, hover) } else { m.label3 };
    icons::paint(ui.painter(), rect.shrink(size * 0.27), icon, c);
    let resp = resp.on_hover_text(tip);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Moldura de alerta (diálogos).
pub fn alert_frame(m: &Mac) -> egui::Frame {
    egui::Frame::new()
        .fill(m.raised)
        .stroke(Stroke::new(1.0, m.sep))
        .corner_radius(CornerRadius::same(16))
        .shadow(egui::Shadow { offset: [0, 16], blur: 48, spread: 0, color: Color32::from_black_alpha(if m.dark { 140 } else { 60 }) })
        .inner_margin(Margin::same(22))
}
