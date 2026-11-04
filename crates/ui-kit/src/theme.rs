//! Stringari Design System aplicado ao egui (compartilhado pelos produtos Stringari).
//!
//! Fonte: o canvas "Stringari Design System" (tokens semânticos light/dark, Geist para a
//! interface, JetBrains Mono para dados e código, Cobalto como acento, Faísca exclusiva da IA).
//! Nenhum componente usa cor crua: tudo passa por `Palette`.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText, Shadow,
    Stroke, TextStyle, Vec2,
};

use crate::icons::{self, Icon};

/// Tokens semânticos (nomes do design system em comentário).
#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,            // bg/app
    pub chrome: Color32,        // barra de título, status
    pub surface: Color32,       // bg/surface
    pub panel: Color32,         // bg/panel
    pub sunken: Color32,        // bg/sunken
    pub hover: Color32,         // bg/hover
    pub selected: Color32,      // bg/selected
    pub border: Color32,        // border/default
    pub border_strong: Color32, // border/strong
    pub text: Color32,          // text/primary
    pub text2: Color32,         // text/secondary
    pub muted: Color32,         // text/muted
    pub line_no: Color32,
    pub line_active: Color32,
    pub accent: Color32,      // accent/fill
    pub accent_text: Color32, // accent/text
    pub focus: Color32,       // focus/ring
    pub on_accent: Color32,   // text/on-accent
    pub ai: Color32,          // ai/fill
    pub ai_text: Color32,     // ai/text
    pub ai_subtle: Color32,   // ai/subtle
    pub ai_border: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub warning_bg: Color32,
    pub danger: Color32,
    pub danger_bg: Color32,
    pub row_alt: Color32,
    // Estados (biblioteca de componentes, out/2026)
    pub raised: Color32,         // menus, popovers e diálogos
    pub pressed: Color32,        // bg/pressed
    pub dis_text: Color32,       // text/disabled
    pub dis_bg: Color32,         // bg/disabled
    pub accent_hover: Color32,   // accent/hover
    pub accent_pressed: Color32, // accent/pressed
    pub success_bg: Color32,
    pub danger_fill: Color32, // fundo do botão de perigo
    pub danger_border: Color32,
    pub scrim: Color32,
    // sintaxe SQL
    pub kw: Color32,
    pub func: Color32,
    pub string: Color32,
    pub num: Color32,
    pub ty: Color32,
    pub comment: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const DARK: Palette = Palette {
    dark: true,
    bg: hex(0x0C0E11),
    chrome: hex(0x101317),
    surface: hex(0x13161B),
    panel: hex(0x171A20),
    sunken: hex(0x0A0B0E),
    hover: hex(0x1E232A),
    selected: hex(0x18234A),
    border: hex(0x23282F),
    border_strong: hex(0x363D48),
    text: hex(0xE8EBF0),
    text2: hex(0xA9B1BD),
    muted: hex(0x8A94A3),
    line_no: hex(0x5C6573),
    line_active: hex(0x1A1E24),
    accent: hex(0x3A63FF),
    accent_text: hex(0x8EA6FF),
    focus: hex(0x6B8BFF),
    on_accent: hex(0xFFFFFF),
    ai: hex(0x7C55FF),
    ai_text: hex(0xB79BFF),
    ai_subtle: hex(0x1F1640),
    ai_border: hex(0x4A2FB0),
    success: hex(0x3FCB7E),
    warning: hex(0xF2A93B),
    warning_bg: hex(0x2E2210),
    danger: hex(0xFF6B6B),
    danger_bg: hex(0x331516),
    row_alt: hex(0x14171C),
    raised: hex(0x1A1E24),
    pressed: hex(0x23282F),
    dis_text: hex(0x4C5563),
    dis_bg: hex(0x1A1E24),
    accent_hover: hex(0x2E52E0),
    accent_pressed: hex(0x2645C2),
    success_bg: hex(0x0F2A1C),
    danger_fill: hex(0xD12F2F),
    danger_border: hex(0x7A2A2C),
    scrim: Color32::from_rgba_premultiplied(0, 0, 0, 140),
    kw: hex(0x8EA6FF),
    func: hex(0xD19BFF),
    string: hex(0x6FD49B),
    num: hex(0xF2A93B),
    ty: hex(0x5ED3DB),
    comment: hex(0x7A8494),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: hex(0xF7F8FA),
    chrome: hex(0xF1F3F6),
    surface: hex(0xFFFFFF),
    panel: hex(0xFBFCFD),
    sunken: hex(0xF1F3F6),
    hover: hex(0xEEF1F5),
    selected: hex(0xEBF0FF),
    border: hex(0xE6E9EE),
    border_strong: hex(0xD3D8E0),
    text: hex(0x13161B),
    text2: hex(0x4C5563),
    muted: hex(0x677181),
    line_no: hex(0x8A94A3),
    line_active: hex(0xF4F6FA),
    accent: hex(0x2E5BFF),
    accent_text: hex(0x2449D8),
    focus: hex(0x2E5BFF),
    on_accent: hex(0xFFFFFF),
    ai: hex(0x6A3FF0),
    ai_text: hex(0x5A2FD9),
    ai_subtle: hex(0xF3EFFF),
    ai_border: hex(0xC9B8FF),
    success: hex(0x1F8A4C),
    warning: hex(0x8F5600),
    warning_bg: hex(0xFFF1D9),
    danger: hex(0xC02626),
    danger_bg: hex(0xFDE8E8),
    row_alt: hex(0xF7F8FA),
    raised: hex(0xFFFFFF),
    pressed: hex(0xE6E9EE),
    dis_text: hex(0xA0A8B4),
    dis_bg: hex(0xF1F3F6),
    accent_hover: hex(0x1F47E0),
    accent_pressed: hex(0x1A3BC0),
    success_bg: hex(0xE3F5EA),
    danger_fill: hex(0xC02626),
    danger_border: hex(0xF4B4B4),
    scrim: Color32::from_rgba_premultiplied(6, 7, 9, 82),
    kw: hex(0x2449D8),
    func: hex(0x8A3FC7),
    string: hex(0x1F7A4D),
    num: hex(0xB4540A),
    ty: hex(0x0E7C86),
    comment: hex(0x677181),
};

pub fn pal(ui: &egui::Ui) -> Palette {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
}

pub fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

/// Raios do design system.
pub mod radius {
    pub const XS: u8 = 4;
    pub const SM: u8 = 6;
    pub const MD: u8 = 8;
    pub const LG: u8 = 12;
    pub const XL: u8 = 16;
}

pub const MEDIUM: &str = "geist-medium";
pub const SEMIBOLD: &str = "geist-semibold";

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        &mut fonts,
        "geist",
        include_bytes!("../../../assets/fonts/Geist-Regular.ttf"),
    );
    add(
        &mut fonts,
        MEDIUM,
        include_bytes!("../../../assets/fonts/Geist-Medium.ttf"),
    );
    add(
        &mut fonts,
        SEMIBOLD,
        include_bytes!("../../../assets/fonts/Geist-SemiBold.ttf"),
    );
    add(
        &mut fonts,
        "jbmono",
        include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"),
    );

    // As fontes padrão ficam como reserva (símbolos e outros alfabetos).
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mono_fallback = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    let with = |first: &str, rest: &[String]| {
        let mut v = vec![first.to_string()];
        v.extend(rest.iter().cloned());
        v
    };
    fonts
        .families
        .insert(FontFamily::Proportional, with("geist", &fallback));
    fonts
        .families
        .insert(FontFamily::Monospace, with("jbmono", &mono_fallback));
    fonts
        .families
        .insert(FontFamily::Name(MEDIUM.into()), with(MEDIUM, &fallback));
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), with(SEMIBOLD, &fallback));
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    let p = if dark { DARK } else { LIGHT };
    let mut style = (*ctx.global_style()).clone();
    style.visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    // Escala tipográfica: base da UI 13 px (ui/md), mono 13 no editor (code/md).
    style.text_styles = [
        (TextStyle::Heading, semibold(20.0)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Button, medium(13.0)),
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Monospace, mono(13.0)),
    ]
    .into();

    // Espaçamento base 4; controles de 28 px.
    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(8.0, 4.0);
    s.button_padding = Vec2::new(10.0, 5.0);
    s.interact_size = Vec2::new(28.0, 28.0);
    s.window_margin = Margin::same(20);
    s.menu_margin = Margin::same(4);
    s.indent = 16.0;
    s.icon_width = 14.0;
    s.icon_spacing = 6.0;
    s.combo_width = 160.0;
    s.scroll.bar_width = 8.0;
    s.scroll.floating = true;

    let v = &mut style.visuals;
    v.dark_mode = dark;
    v.override_text_color = None;
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(radius::XL);
    v.menu_corner_radius = CornerRadius::same(radius::MD);
    // Elevação 2 (diálogos) e 1 (menus), conforme a biblioteca "Menus e sobreposições".
    v.window_fill = p.raised;
    v.window_shadow = if dark {
        Shadow { offset: [0, 24], blur: 64, spread: 0, color: Color32::from_black_alpha(150) }
    } else {
        Shadow { offset: [0, 24], blur: 56, spread: 0, color: Color32::from_rgba_unmultiplied(19, 22, 27, 56) }
    };
    v.popup_shadow = if dark {
        Shadow { offset: [0, 12], blur: 32, spread: 0, color: Color32::from_black_alpha(128) }
    } else {
        Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_rgba_unmultiplied(19, 22, 27, 31) }
    };
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.surface);
    v.faint_bg_color = p.row_alt;
    v.code_bg_color = p.sunken;
    v.hyperlink_color = p.accent_text;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.selection.bg_fill = if dark {
        alpha(p.accent, 0.45)
    } else {
        alpha(p.accent, 0.22)
    };
    v.selection.stroke = Stroke::new(1.0, p.focus);
    v.striped = true;
    v.collapsing_header_frame = false;
    v.button_frame = true;
    v.indent_has_left_vline = false;

    let r = CornerRadius::same(radius::SM);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text2);
    w.noninteractive.corner_radius = r;

    w.inactive.bg_fill = p.surface;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border_strong);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = r;
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.selected;
    w.active.weak_bg_fill = p.selected;
    w.active.bg_stroke = Stroke::new(1.0, p.focus);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;

    w.open = w.hovered;

    ctx.set_global_style(style);
}

// ---------------------------------------------------------------------------------------
// Componentes

fn button_with_icon(
    ui: &mut egui::Ui,
    icon: Option<Icon>,
    text: &str,
    fg: Color32,
    fill: Color32,
    stroke: Stroke,
    hint: Option<&str>,
) -> egui::Response {
    let font = medium(13.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, fg);
    let hint_galley = hint.map(|h| {
        ui.painter()
            .layout_no_wrap(h.to_string(), mono(11.0), alpha(fg, 0.8))
    });
    let icon_w = if icon.is_some() { 14.0 + 6.0 } else { 0.0 };
    let hint_w = hint_galley.as_ref().map_or(0.0, |g| g.size().x + 6.0);
    let pad = if text.is_empty() { 7.0 } else { 10.0 };
    let w = pad * 2.0 + icon_w + galley.size().x + hint_w
        - if text.is_empty() && icon.is_some() {
            6.0
        } else {
            0.0
        };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let down = resp.is_pointer_button_down_on();
        let p = pal(ui);
        let fill = if fill == p.accent {
            if down { p.accent_pressed } else if hovered { p.accent_hover } else { fill }
        } else if fill == Color32::TRANSPARENT || fill == p.surface {
            if down { p.pressed } else if hovered { p.hover } else { fill }
        } else if down {
            lighten(lighten(fill, !ui.visuals().dark_mode), !ui.visuals().dark_mode)
        } else if hovered {
            lighten(fill, ui.visuals().dark_mode)
        } else {
            fill
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(radius::SM),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        let mut x = rect.left() + pad;
        if let Some(i) = icon {
            let r = egui::Rect::from_center_size(
                egui::pos2(x + 7.0, rect.center().y),
                Vec2::splat(14.0),
            );
            icons::paint(ui.painter(), r, i, fg);
            x += icon_w;
        }
        ui.painter().galley(
            egui::pos2(x, rect.center().y - galley.size().y / 2.0),
            galley.clone(),
            fg,
        );
        x += galley.size().x + 6.0;
        if let Some(h) = hint_galley {
            ui.painter()
                .galley(egui::pos2(x, rect.center().y - h.size().y / 2.0), h, fg);
        }
        if resp.has_focus() {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                CornerRadius::same(radius::MD),
                Stroke::new(2.0, pal(ui).focus),
                egui::StrokeKind::Outside,
            );
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn lighten(c: Color32, dark: bool) -> Color32 {
    let f = |x: u8| {
        if dark {
            x.saturating_add(18)
        } else {
            (x as f32 * 0.9) as u8
        }
    };
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// Ação principal (Cobalto).
pub fn primary(
    ui: &mut egui::Ui,
    icon: Option<Icon>,
    text: &str,
    hint: Option<&str>,
) -> egui::Response {
    let p = pal(ui);
    button_with_icon(ui, icon, text, p.on_accent, p.accent, Stroke::NONE, hint)
}

/// Secundário com contorno.
pub fn secondary(ui: &mut egui::Ui, icon: Option<Icon>, text: &str) -> egui::Response {
    let p = pal(ui);
    button_with_icon(
        ui,
        icon,
        text,
        p.text,
        Color32::TRANSPARENT,
        Stroke::new(1.0, p.border_strong),
        None,
    )
}

/// Sem contorno.
pub fn ghost(ui: &mut egui::Ui, icon: Option<Icon>, text: &str) -> egui::Response {
    let p = pal(ui);
    button_with_icon(
        ui,
        icon,
        text,
        p.text2,
        Color32::TRANSPARENT,
        Stroke::NONE,
        None,
    )
}

/// Ação de IA, sutil (sempre com a faísca).
pub fn ai_subtle(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let p = pal(ui);
    button_with_icon(
        ui,
        Some(Icon::Spark),
        text,
        p.ai_text,
        p.ai_subtle,
        Stroke::NONE,
        None,
    )
}

/// Ação de IA, cheia (confirmar o que a IA sugeriu).
pub fn ai_primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let p = pal(ui);
    button_with_icon(
        ui,
        Some(Icon::Spark),
        text,
        p.on_accent,
        p.ai,
        Stroke::NONE,
        None,
    )
}

pub fn danger_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let p = pal(ui);
    button_with_icon(ui, None, text, p.on_accent, p.danger_fill, Stroke::NONE, None)
}

/// Perigo sutil: ações destrutivas secundárias ("Descartar", "Remover do arquivo").
pub fn danger_subtle(ui: &mut egui::Ui, icon: Option<Icon>, text: &str) -> egui::Response {
    let p = pal(ui);
    button_with_icon(ui, icon, text, p.danger, Color32::TRANSPARENT, Stroke::new(1.0, p.danger_border), None)
}

/// Primário com ícone e atalho opcional.
pub fn primary_icon(ui: &mut egui::Ui, icon: Icon, text: &str) -> egui::Response {
    let p = pal(ui);
    button_with_icon(ui, Some(icon), text, p.on_accent, p.accent, Stroke::NONE, None)
}

/// Switch 32×18 (aplica na hora, sem "Salvar").
pub fn switch(ui: &mut egui::Ui, on: &mut bool, label: &str) -> egui::Response {
    let p = pal(ui);
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(13.0), p.text);
    let w = 32.0 + if label.is_empty() { 0.0 } else { 8.0 + galley.size().x };
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let track = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - 9.0), Vec2::new(32.0, 18.0));
    let fill = if *on { p.accent } else if p.dark { p.border_strong } else { hex(0xB4BCC8) };
    ui.painter().rect_filled(track, CornerRadius::same(9), fill);
    let x = egui::lerp(track.left() + 9.0..=track.right() - 9.0, t);
    ui.painter().circle_filled(egui::pos2(x, track.center().y), 7.0, Color32::WHITE);
    if !label.is_empty() {
        ui.painter().galley(egui::pos2(track.right() + 8.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
    }
    if resp.has_focus() {
        ui.painter().rect_stroke(track.expand(2.0), CornerRadius::same(11), Stroke::new(2.0, p.focus), egui::StrokeKind::Outside);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// SegmentedControl: 30 de altura, itens de 26, raio 6. Para trocar visualização.
pub fn segmented(ui: &mut egui::Ui, selected: &mut usize, items: &[&str]) -> egui::Response {
    let p = pal(ui);
    let font = medium(12.0);
    let widths: Vec<f32> = items.iter().map(|t| ui.painter().layout_no_wrap(t.to_string(), font.clone(), p.text).size().x + 20.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(total, 30.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(radius::SM), p.sunken);
    let mut x = rect.left() + 2.0;
    for (i, (label, w)) in items.iter().zip(&widths).enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 2.0), Vec2::new(*w, 26.0));
        let ir = ui.interact(r, resp.id.with(i), egui::Sense::click());
        let active = *selected == i;
        if active {
            ui.painter().rect(r, CornerRadius::same(radius::XS + 1), p.surface, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
        } else if ir.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(radius::XS + 1), p.hover);
        }
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, *label, if active { semibold(12.0) } else { font.clone() }, if active { p.text } else { p.text2 });
        if ir.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !active {
            *selected = i;
            resp.mark_changed();
        }
        x += w;
    }
    resp
}

/// Barra de progresso (4 px). `None` = indeterminada (animada).
pub fn progress(ui: &mut egui::Ui, fraction: Option<f32>, width: f32, color: Color32) -> egui::Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 6.0), egui::Sense::hover());
    let track = egui::Rect::from_center_size(rect.center(), Vec2::new(width, 4.0));
    ui.painter().rect_filled(track, CornerRadius::same(2), if p.dark { p.border } else { p.pressed });
    match fraction {
        Some(f) => {
            let fill = egui::Rect::from_min_size(track.min, Vec2::new(track.width() * f.clamp(0.0, 1.0), 4.0));
            ui.painter().rect_filled(fill, CornerRadius::same(2), color);
        }
        None => {
            let t = ui.input(|i| i.time) as f32;
            let seg = width * 0.3;
            let x = (t * 0.8).fract() * (width + seg) - seg;
            let a = (track.left() + x).max(track.left());
            let b = (track.left() + x + seg).min(track.right());
            if b > a {
                ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(a, track.top()), egui::pos2(b, track.bottom())), CornerRadius::same(2), color);
            }
            ui.ctx().request_repaint();
        }
    }
    resp
}

/// Tecla (Kbd): atalho em mono sobre fundo afundado.
pub fn kbd(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.sunken)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(radius::XS))
        .inner_margin(Margin::symmetric(5, 1))
        .show(ui, |ui| ui.label(RichText::new(text).font(mono(11.0)).color(p.text2)))
        .response
}

/// Botão quadrado só com ícone (28 px, com rótulo acessível na dica).
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str, bordered: bool) -> egui::Response {
    let p = pal(ui);
    let stroke = if bordered {
        Stroke::new(1.0, p.border)
    } else {
        Stroke::NONE
    };
    button_with_icon(
        ui,
        Some(icon),
        "",
        p.text2,
        Color32::TRANSPARENT,
        stroke,
        None,
    )
    .on_hover_text(label)
}

/// Pílula de texto (contexto, tipos, contagens).
pub fn chip(
    ui: &mut egui::Ui,
    text: impl Into<String>,
    fg: Color32,
    bg: Color32,
    monospace: bool,
) -> egui::Response {
    let font = if monospace { mono(11.0) } else { medium(11.0) };
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(radius::XS))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text.into()).color(fg).font(font)).extend())
        })
        .response
}

/// Selo arredondado de ambiente (PROD/STG/DEV).
pub fn pill(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(fg).font(semibold(10.0)))
        })
        .response
}

/// Legenda em caixa alta (caption 11/600, tracking 0.06em).
pub fn caption(ui: &mut egui::Ui, text: &str) {
    let p = pal(ui);
    ui.label(
        RichText::new(text.to_uppercase())
            .color(p.muted)
            .font(semibold(11.0))
            .extra_letter_spacing(0.66),
    );
}

pub fn dot(ui: &mut egui::Ui, color: Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size + 2.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
}

pub fn field_label(ui: &mut egui::Ui, text: &str) {
    let p = pal(ui);
    ui.label(RichText::new(text).color(p.text2).font(medium(12.0)));
}

/// Marca do produto: ícone em branco sobre Cobalto.
pub fn logo(ui: &mut egui::Ui, size: f32, mark: Icon) -> egui::Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same((size * 0.25) as u8),
        if p.dark { p.accent } else { hex(0x2E5BFF) },
    );
    icons::paint(
        ui.painter(),
        rect.shrink(size * 0.16),
        mark,
        Color32::WHITE,
    );
    resp
}

pub fn cmd_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}

/// Moldura de painel/cartão (bg/surface, borda, raio lg).
pub fn card_frame(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(radius::LG))
}
