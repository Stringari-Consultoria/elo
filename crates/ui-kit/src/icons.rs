//! Ícones vetoriais do design system: traço de 1,5 px numa grade de 24, cantos
//! arredondados. Desenhados direto no painter (sem emoji, sem imagens), então ficam nítidos
//! em qualquer escala e herdam a cor do contexto. A faísca é reservada à IA.

use std::f32::consts::{PI, TAU};

use egui::epaint::{Mesh, PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Database,
    Table,
    View,
    Key,
    Play,
    PlayAll,
    Stop,
    Search,
    Spark,
    Sun,
    Moon,
    Plus,
    ChevronRight,
    Close,
    Send,
    Shield,
    Layers,
    Copy,
    Download,
    Plan,
    Gear,
    Check,
    Alert,
    Edit,
    Folder,
    FolderOpen,
    Clock,
    Trash,
    Upload,
    Globe,
    Lock,
    Eye,
    More,
    /// Marca do Relay: dois nós ligados por um arco (requisição → resposta).
    Relay,
    File,
    Filter,
    ChevronDown,
    Refresh,
    Link,
    /// Caixa de arquivo compactado (marca do Arca).
    Archive,
    Extract,
    Compress,
    Image,
    FileText,
    FileCode,
    Info,
    Home,
    Scissors,
    ShieldCheck,
    ArrowUp,
    ArrowLeft,
    Music,
    Video,
    Monitor,
    // ── Traço (modelagem 3D) ──
    /// Marca do Traço: um traço que vira volume.
    Traco,
    Cursor,
    Star,
    Power,
    Eraser,
    Bucket,
    Pencil,
    Freehand,
    Rect,
    RotRect,
    Circle,
    Polygon,
    Arc,
    Pie,
    PushPull,
    Move,
    Rotate,
    Scale,
    Offset,
    FollowMe,
    Tape,
    Protractor,
    Dimension,
    TextTool,
    Orbit,
    Hand,
    ZoomExtents,
    Undo,
    Redo,
    Cube,
    Component,
    Group,
    Camera,
    Section,
    Xray,
    Shadow,
    Scenes,
    Ruler,
    Smoove,
    Stamp,
    Drape,
    Detail,
    FlipEdge,
    Interact,
    Dynamic,
    Wall,
    Door,
    Window,
    Slab,
    Room,
    Column,
    Beam,
    Railing,
    Stair,
    Ramp,
    Comment,
    Budget,
    Audit,
    Versions,
    Library,
    Remote,
    /// Tela cheia: quatro cantos para fora.
    Expand,
    /// Sair da tela cheia: quatro cantos para dentro.
    Shrink,
    /// Som ligado: alto-falante com ondas.
    Speaker,
    /// Som desligado: alto-falante riscado.
    SpeakerOff,
}

struct Pen<'a> {
    painter: &'a Painter,
    rect: Rect,
    stroke: Stroke,
    color: Color32,
}

impl Pen<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.rect.min + Vec2::new(x / 24.0 * self.rect.width(), y / 24.0 * self.rect.height())
    }
    fn s(&self, v: f32) -> f32 {
        v / 24.0 * self.rect.width()
    }
    fn line(&self, pts: &[(f32, f32)]) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(PathShape::line(
            pts,
            PathStroke::new(self.stroke.width, self.color),
        ));
    }
    fn closed(&self, pts: &[(f32, f32)]) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(PathShape::closed_line(
            pts,
            PathStroke::new(self.stroke.width, self.color),
        ));
    }
    fn circle(&self, x: f32, y: f32, r: f32) {
        self.painter
            .circle_stroke(self.p(x, y), self.s(r), self.stroke);
    }
    fn dot(&self, x: f32, y: f32, r: f32) {
        self.painter
            .circle_filled(self.p(x, y), self.s(r), self.color);
    }
    /// Arco de elipse de `a0` a `a1` (radianos, 0 = direita, sentido horário na tela).
    fn arc(&self, cx: f32, cy: f32, rx: f32, ry: f32, a0: f32, a1: f32) {
        let n = 24;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                (cx + rx * a.cos(), cy + ry * a.sin())
            })
            .collect();
        self.line(&pts);
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let rr = Rect::from_min_max(self.p(x, y), self.p(x + w, y + h));
        self.painter
            .rect_stroke(rr, self.s(r), self.stroke, egui::StrokeKind::Middle);
    }
    /// Polígono cheio em leque a partir de um centro (funciona para estrelas).
    fn fan(&self, center: (f32, f32), pts: &[(f32, f32)]) {
        let mut mesh = Mesh::default();
        mesh.colored_vertex(self.p(center.0, center.1), self.color);
        for &(x, y) in pts {
            mesh.colored_vertex(self.p(x, y), self.color);
        }
        let n = pts.len() as u32;
        for i in 0..n {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
        }
        self.painter.add(mesh);
    }
}

pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let width = (1.5 * rect.width() / 24.0).max(1.2);
    let pen = Pen {
        painter,
        rect,
        stroke: Stroke::new(width, color),
        color,
    };
    match icon {
        Icon::Database => {
            pen.arc(12.0, 6.0, 7.0, 3.0, 0.0, TAU);
            pen.line(&[(5.0, 6.0), (5.0, 18.0)]);
            pen.line(&[(19.0, 6.0), (19.0, 18.0)]);
            pen.arc(12.0, 18.0, 7.0, 3.0, 0.0, PI);
            pen.arc(12.0, 12.0, 7.0, 3.0, 0.0, PI);
        }
        Icon::Table => {
            pen.rect(3.0, 4.0, 18.0, 16.0, 2.0);
            pen.line(&[(3.0, 10.0), (21.0, 10.0)]);
            pen.line(&[(3.0, 15.0), (21.0, 15.0)]);
            pen.line(&[(10.0, 4.0), (10.0, 20.0)]);
        }
        Icon::View => {
            pen.arc(12.0, 12.0, 9.5, 6.0, PI, TAU);
            pen.arc(12.0, 12.0, 9.5, 6.0, 0.0, PI);
            pen.circle(12.0, 12.0, 2.6);
        }
        Icon::Key => {
            pen.circle(8.0, 14.0, 4.0);
            pen.line(&[(11.0, 11.0), (19.0, 3.0)]);
            pen.line(&[(16.0, 6.0), (18.0, 8.0)]);
            pen.line(&[(14.0, 8.0), (16.0, 10.0)]);
        }
        Icon::Play => pen.closed(&[(7.0, 4.0), (19.0, 12.0), (7.0, 20.0)]),
        Icon::PlayAll => {
            pen.closed(&[(4.0, 5.0), (12.0, 12.0), (4.0, 19.0)]);
            pen.closed(&[(12.0, 5.0), (20.0, 12.0), (12.0, 19.0)]);
        }
        Icon::Stop => pen.rect(6.0, 6.0, 12.0, 12.0, 2.0),
        Icon::Search => {
            pen.circle(11.0, 11.0, 7.0);
            pen.line(&[(20.0, 20.0), (16.0, 16.0)]);
        }

        Icon::Spark => {
            pen.fan(
                (12.0, 10.0),
                &[
                    (12.0, 3.0),
                    (13.9, 8.1),
                    (19.0, 10.0),
                    (13.9, 11.9),
                    (12.0, 17.0),
                    (10.1, 11.9),
                    (5.0, 10.0),
                    (10.1, 8.1),
                ],
            );
            pen.fan(
                (18.5, 18.5),
                &[
                    (18.5, 15.5),
                    (19.3, 17.7),
                    (21.5, 18.5),
                    (19.3, 19.3),
                    (18.5, 21.5),
                    (17.7, 19.3),
                    (15.5, 18.5),
                    (17.7, 17.7),
                ],
            );
        }
        Icon::Sun => {
            pen.circle(12.0, 12.0, 4.0);
            for i in 0..8 {
                let a = i as f32 * TAU / 8.0;
                pen.line(&[
                    (12.0 + 7.0 * a.cos(), 12.0 + 7.0 * a.sin()),
                    (12.0 + 9.5 * a.cos(), 12.0 + 9.5 * a.sin()),
                ]);
            }
        }
        Icon::Moon => {
            // Lua crescente: arco externo grande + arco interno.
            pen.arc(12.0, 12.0, 8.5, 8.5, -PI * 0.28, PI * 1.62);
            pen.arc(16.0, 9.0, 6.4, 6.4, PI * 0.95, PI * 0.23);
        }
        Icon::Plus => {
            pen.line(&[(12.0, 5.0), (12.0, 19.0)]);
            pen.line(&[(5.0, 12.0), (19.0, 12.0)]);
        }
        Icon::ChevronRight => pen.line(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)]),
        Icon::Close => {
            pen.line(&[(6.0, 6.0), (18.0, 18.0)]);
            pen.line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Icon::Send => {
            pen.line(&[(12.0, 19.0), (12.0, 5.0)]);
            pen.line(&[(6.0, 11.0), (12.0, 5.0), (18.0, 11.0)]);
        }
        Icon::Shield => pen.closed(&[
            (12.0, 3.0),
            (19.0, 6.0),
            (19.0, 11.0),
            (17.5, 15.5),
            (12.0, 21.0),
            (6.5, 15.5),
            (5.0, 11.0),
            (5.0, 6.0),
        ]),
        Icon::Layers => {
            pen.closed(&[(12.0, 3.0), (21.0, 8.0), (12.0, 13.0), (3.0, 8.0)]);
            pen.line(&[(3.0, 12.5), (12.0, 17.5), (21.0, 12.5)]);
            pen.line(&[(3.0, 16.5), (12.0, 21.5), (21.0, 16.5)]);
        }
        Icon::Copy => {
            pen.rect(8.0, 8.0, 12.0, 12.0, 2.0);
            pen.line(&[
                (16.0, 8.0),
                (16.0, 6.0),
                (15.0, 4.0),
                (6.0, 4.0),
                (4.0, 6.0),
                (4.0, 15.0),
                (6.0, 16.0),
                (8.0, 16.0),
            ]);
        }
        Icon::Download => {
            pen.line(&[(12.0, 4.0), (12.0, 15.0)]);
            pen.line(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]);
            pen.line(&[(4.0, 19.0), (20.0, 19.0)]);
        }
        Icon::Plan => {
            pen.rect(3.0, 3.0, 7.0, 5.0, 1.5);
            pen.rect(14.0, 10.0, 7.0, 5.0, 1.5);
            pen.rect(14.0, 17.0, 7.0, 5.0, 1.5);
            pen.line(&[(6.5, 8.0), (6.5, 19.5), (14.0, 19.5)]);
            pen.line(&[(6.5, 12.5), (14.0, 12.5)]);
        }
        Icon::Gear => {
            pen.circle(12.0, 12.0, 3.0);
            pen.circle(12.0, 12.0, 7.5);
            for i in 0..8 {
                let a = i as f32 * TAU / 8.0;
                pen.line(&[
                    (12.0 + 7.5 * a.cos(), 12.0 + 7.5 * a.sin()),
                    (12.0 + 10.0 * a.cos(), 12.0 + 10.0 * a.sin()),
                ]);
            }
        }
        Icon::Check => pen.line(&[(5.0, 12.5), (10.0, 17.5), (19.0, 7.0)]),
        Icon::Alert => {
            pen.closed(&[(12.0, 3.5), (21.5, 20.0), (2.5, 20.0)]);
            pen.line(&[(12.0, 10.0), (12.0, 14.0)]);
            pen.dot(12.0, 17.0, 0.9);
        }

        Icon::Edit => {
            pen.closed(&[
                (15.0, 4.0),
                (20.0, 9.0),
                (9.0, 20.0),
                (4.0, 20.0),
                (4.0, 15.0),
            ]);
            pen.line(&[(13.0, 6.0), (18.0, 11.0)]);
        }
        Icon::Folder => pen.closed(&[
            (3.0, 6.5),
            (4.5, 5.0),
            (9.5, 5.0),
            (11.5, 7.0),
            (19.5, 7.0),
            (21.0, 8.5),
            (21.0, 17.5),
            (19.5, 19.0),
            (4.5, 19.0),
            (3.0, 17.5),
        ]),
        Icon::FolderOpen => {
            pen.line(&[(3.0, 17.5), (3.0, 6.5), (4.5, 5.0), (9.5, 5.0), (11.5, 7.0), (18.0, 7.0), (19.0, 8.0), (19.0, 10.0)]);
            pen.closed(&[(3.0, 19.0), (6.5, 10.5), (22.0, 10.5), (18.5, 19.0)]);
        }
        Icon::Clock => {
            pen.circle(12.0, 12.0, 8.5);
            pen.line(&[(12.0, 7.0), (12.0, 12.0), (15.5, 14.0)]);
        }
        Icon::Trash => {
            pen.line(&[(4.0, 6.5), (20.0, 6.5)]);
            pen.line(&[(9.0, 6.5), (9.5, 4.0), (14.5, 4.0), (15.0, 6.5)]);
            pen.closed(&[(6.0, 6.5), (7.0, 20.0), (17.0, 20.0), (18.0, 6.5)]);
            pen.line(&[(10.0, 10.0), (10.0, 16.5)]);
            pen.line(&[(14.0, 10.0), (14.0, 16.5)]);
        }
        Icon::Upload => {
            pen.line(&[(12.0, 15.0), (12.0, 4.0)]);
            pen.line(&[(7.0, 9.0), (12.0, 4.0), (17.0, 9.0)]);
            pen.line(&[(4.0, 15.0), (4.0, 20.0), (20.0, 20.0), (20.0, 15.0)]);
        }
        Icon::Globe => {
            pen.circle(12.0, 12.0, 9.0);
            pen.arc(12.0, 12.0, 4.0, 9.0, 0.0, TAU);
            pen.line(&[(3.0, 12.0), (21.0, 12.0)]);
        }
        Icon::Lock => {
            pen.rect(5.0, 10.5, 14.0, 10.0, 2.0);
            pen.arc(12.0, 10.5, 4.0, 4.5, PI, TAU);
            pen.dot(12.0, 15.5, 1.1);
        }
        Icon::Eye => {
            pen.arc(12.0, 12.0, 9.5, 6.0, PI, TAU);
            pen.arc(12.0, 12.0, 9.5, 6.0, 0.0, PI);
            pen.circle(12.0, 12.0, 2.6);
        }
        Icon::More => {
            pen.dot(5.5, 12.0, 1.5);
            pen.dot(12.0, 12.0, 1.5);
            pen.dot(18.5, 12.0, 1.5);
        }
        Icon::Relay => {
            pen.circle(5.5, 17.0, 2.6);
            pen.circle(18.5, 7.0, 2.6);
            pen.arc(12.0, 17.0, 6.5, 10.0, PI, PI * 1.5);
            pen.line(&[(12.0, 7.0), (15.9, 7.0)]);
            pen.line(&[(8.1, 17.0), (12.0, 17.0)]);
            pen.arc(12.0, 7.0, 6.5, 10.0, 0.0, PI * 0.5);
        }
        Icon::File => {
            pen.closed(&[(6.0, 3.0), (14.0, 3.0), (19.0, 8.0), (19.0, 21.0), (6.0, 21.0)]);
            pen.line(&[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]);
        }
        Icon::Filter => pen.closed(&[(3.5, 5.0), (20.5, 5.0), (14.0, 12.5), (14.0, 19.0), (10.0, 17.0), (10.0, 12.5)]),
        Icon::ChevronDown => pen.line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
        Icon::Refresh => {
            pen.arc(12.0, 12.0, 7.5, 7.5, -PI * 0.35, PI * 1.45);
            pen.line(&[(14.5, 3.0), (17.8, 6.3), (14.0, 7.8)]);
        }
        Icon::Link => {
            pen.rect(3.0, 9.0, 10.0, 6.0, 3.0);
            pen.rect(11.0, 9.0, 10.0, 6.0, 3.0);
        }
        Icon::Archive => {
            pen.rect(3.0, 4.0, 18.0, 5.0, 1.5);
            pen.closed(&[(4.5, 9.0), (4.5, 19.0), (6.0, 20.5), (18.0, 20.5), (19.5, 19.0), (19.5, 9.0)]);
            pen.line(&[(10.0, 13.0), (14.0, 13.0)]);
        }
        Icon::Extract => {
            pen.line(&[(4.0, 11.0), (4.0, 19.0), (5.5, 20.5), (18.5, 20.5), (20.0, 19.0), (20.0, 11.0)]);
            pen.line(&[(12.0, 15.0), (12.0, 3.5)]);
            pen.line(&[(8.0, 7.5), (12.0, 3.5), (16.0, 7.5)]);
        }
        Icon::Compress => {
            pen.line(&[(4.0, 11.0), (4.0, 19.0), (5.5, 20.5), (18.5, 20.5), (20.0, 19.0), (20.0, 11.0)]);
            pen.line(&[(12.0, 3.5), (12.0, 15.0)]);
            pen.line(&[(8.0, 11.0), (12.0, 15.0), (16.0, 11.0)]);
        }
        Icon::Image => {
            pen.rect(3.0, 4.0, 18.0, 16.0, 2.0);
            pen.circle(8.5, 9.5, 1.8);
            pen.line(&[(3.5, 17.5), (9.0, 12.5), (13.0, 16.0), (16.0, 13.5), (20.5, 17.5)]);
        }
        Icon::FileText => {
            pen.closed(&[(6.0, 3.0), (14.0, 3.0), (19.0, 8.0), (19.0, 21.0), (6.0, 21.0)]);
            pen.line(&[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]);
            pen.line(&[(9.0, 12.5), (16.0, 12.5)]);
            pen.line(&[(9.0, 16.5), (16.0, 16.5)]);
        }
        Icon::FileCode => {
            pen.closed(&[(6.0, 3.0), (14.0, 3.0), (19.0, 8.0), (19.0, 21.0), (6.0, 21.0)]);
            pen.line(&[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]);
            pen.line(&[(10.5, 11.5), (8.5, 14.0), (10.5, 16.5)]);
            pen.line(&[(14.5, 11.5), (16.5, 14.0), (14.5, 16.5)]);
        }
        Icon::Info => {
            pen.circle(12.0, 12.0, 9.0);
            pen.line(&[(12.0, 11.0), (12.0, 16.5)]);
            pen.dot(12.0, 7.8, 1.0);
        }
        Icon::Home => {
            pen.line(&[(3.5, 11.0), (12.0, 4.0), (20.5, 11.0)]);
            pen.line(&[(6.0, 9.5), (6.0, 20.0), (18.0, 20.0), (18.0, 9.5)]);
            pen.line(&[(10.0, 20.0), (10.0, 14.5), (14.0, 14.5), (14.0, 20.0)]);
        }
        Icon::Scissors => {
            pen.circle(6.5, 6.5, 3.0);
            pen.circle(6.5, 17.5, 3.0);
            pen.line(&[(9.0, 8.5), (20.0, 18.0)]);
            pen.line(&[(9.0, 15.5), (20.0, 6.0)]);
        }
        Icon::ShieldCheck => {
            pen.closed(&[(12.0, 3.0), (19.0, 6.0), (19.0, 11.0), (17.5, 15.5), (12.0, 21.0), (6.5, 15.5), (5.0, 11.0), (5.0, 6.0)]);
            pen.line(&[(8.8, 12.0), (11.2, 14.4), (15.5, 9.6)]);
        }
        Icon::ArrowUp => {
            pen.line(&[(12.0, 19.0), (12.0, 5.0)]);
            pen.line(&[(6.0, 11.0), (12.0, 5.0), (18.0, 11.0)]);
        }
        Icon::ArrowLeft => {
            pen.line(&[(19.0, 12.0), (5.0, 12.0)]);
            pen.line(&[(11.0, 6.0), (5.0, 12.0), (11.0, 18.0)]);
        }
        Icon::Music => {
            pen.line(&[(9.0, 17.5), (9.0, 5.0), (19.0, 3.5), (19.0, 15.5)]);
            pen.circle(6.5, 17.5, 2.5);
            pen.circle(16.5, 15.5, 2.5);
        }
        Icon::Video => {
            pen.rect(3.0, 6.0, 13.0, 12.0, 2.0);
            pen.closed(&[(16.0, 10.5), (21.0, 7.5), (21.0, 16.5), (16.0, 13.5)]);
        }
        Icon::Monitor => {
            pen.rect(3.0, 4.0, 18.0, 12.5, 2.0);
            pen.line(&[(8.0, 20.5), (16.0, 20.5)]);
            pen.line(&[(12.0, 16.5), (12.0, 20.5)]);
        }
        Icon::Traco => {
            pen.line(&[(4.0, 18.0), (12.0, 6.0)]);
            pen.closed(&[(12.0, 6.0), (20.0, 10.0), (20.0, 18.0), (12.0, 14.0)]);
            pen.line(&[(12.0, 14.0), (4.0, 18.0)]);
        }
        Icon::Power => {
            pen.line(&[(12.0, 3.0), (12.0, 11.5)]);
            pen.line(&[(7.2, 6.2), (6.4, 6.9), (5.3, 8.4), (4.6, 10.2), (4.5, 12.1), (4.9, 14.0), (5.8, 15.8), (7.1, 17.3), (8.7, 18.4), (10.5, 19.0), (12.0, 19.2), (13.5, 19.0), (15.3, 18.4), (16.9, 17.3), (18.2, 15.8), (19.1, 14.0), (19.5, 12.1), (19.4, 10.2), (18.7, 8.4), (17.6, 6.9), (16.8, 6.2)]);
        }
        Icon::Star => pen.closed(&[(12.00, 3.10), (14.41, 9.28), (21.04, 9.66), (15.90, 13.87), (17.58, 20.29), (12.00, 16.70), (6.42, 20.29), (8.10, 13.87), (2.96, 9.66), (9.59, 9.28)]),
        Icon::Cursor => pen.closed(&[(6.0, 3.5), (18.5, 13.5), (12.5, 14.5), (15.5, 20.5), (13.0, 21.5), (10.0, 15.5), (6.0, 19.5)]),
        Icon::Eraser => {
            pen.closed(&[(3.5, 15.0), (13.0, 5.5), (20.5, 13.0), (13.5, 20.0), (8.5, 20.0)]);
            pen.line(&[(8.0, 10.5), (15.5, 18.0)]);
            pen.line(&[(13.5, 20.0), (21.0, 20.0)]);
        }
        Icon::Bucket => {
            pen.closed(&[(5.0, 11.0), (12.0, 4.0), (19.0, 11.0), (12.0, 18.0)]);
            pen.line(&[(5.0, 11.0), (19.0, 11.0)]);
            pen.line(&[(20.0, 14.0), (21.0, 17.5), (20.0, 19.0), (19.0, 17.5), (20.0, 14.0)]);
        }
        Icon::Pencil => {
            pen.closed(&[(4.0, 20.0), (5.0, 15.5), (16.0, 4.5), (19.5, 8.0), (8.5, 19.0)]);
            pen.line(&[(13.5, 7.0), (17.0, 10.5)]);
        }
        Icon::Freehand => pen.line(&[(3.5, 16.0), (6.0, 11.0), (9.0, 9.5), (11.0, 13.0), (13.0, 16.5), (16.0, 15.0), (18.0, 9.0), (20.5, 7.0)]),
        Icon::Rect => {
            pen.rect(4.0, 6.0, 16.0, 12.0, 0.5);
            pen.dot(4.0, 6.0, 1.6);
            pen.dot(20.0, 18.0, 1.6);
        }
        Icon::RotRect => {
            pen.closed(&[(3.5, 13.0), (11.0, 5.5), (20.5, 11.0), (13.0, 18.5)]);
            pen.dot(3.5, 13.0, 1.6);
        }
        Icon::Circle => {
            pen.circle(12.0, 12.0, 8.0);
            pen.dot(12.0, 12.0, 1.4);
            pen.line(&[(12.0, 12.0), (19.0, 8.0)]);
        }
        Icon::Polygon => pen.closed(&[(12.0, 3.5), (19.5, 8.0), (19.5, 16.0), (12.0, 20.5), (4.5, 16.0), (4.5, 8.0)]),
        Icon::Arc => {
            pen.arc(12.0, 17.0, 8.5, 9.0, PI, TAU);
            pen.dot(3.5, 17.0, 1.6);
            pen.dot(20.5, 17.0, 1.6);
        }
        Icon::Pie => {
            pen.arc(10.0, 14.0, 9.0, 9.0, -PI * 0.5, 0.0);
            pen.line(&[(10.0, 5.0), (10.0, 14.0), (19.0, 14.0)]);
        }
        Icon::PushPull => {
            pen.closed(&[(4.0, 16.0), (12.0, 20.0), (20.0, 16.0), (12.0, 12.0)]);
            pen.line(&[(12.0, 12.0), (12.0, 3.0)]);
            pen.line(&[(8.5, 6.5), (12.0, 3.0), (15.5, 6.5)]);
        }
        Icon::Move => {
            pen.line(&[(12.0, 3.0), (12.0, 21.0)]);
            pen.line(&[(3.0, 12.0), (21.0, 12.0)]);
            pen.line(&[(9.5, 5.5), (12.0, 3.0), (14.5, 5.5)]);
            pen.line(&[(9.5, 18.5), (12.0, 21.0), (14.5, 18.5)]);
            pen.line(&[(5.5, 9.5), (3.0, 12.0), (5.5, 14.5)]);
            pen.line(&[(18.5, 9.5), (21.0, 12.0), (18.5, 14.5)]);
        }
        Icon::Rotate => {
            pen.arc(12.0, 12.0, 8.0, 8.0, -PI * 0.25, PI * 1.45);
            pen.line(&[(17.6, 2.8), (17.7, 6.4), (14.2, 6.9)]);
        }
        Icon::Scale => {
            pen.rect(3.5, 9.5, 11.0, 11.0, 1.0);
            pen.line(&[(11.0, 13.0), (20.5, 3.5)]);
            pen.line(&[(14.5, 3.5), (20.5, 3.5), (20.5, 9.5)]);
        }
        Icon::Offset => {
            pen.rect(3.5, 3.5, 17.0, 17.0, 1.0);
            pen.rect(8.0, 8.0, 8.0, 8.0, 0.5);
        }
        Icon::FollowMe => {
            pen.rect(3.5, 3.5, 6.0, 6.0, 0.5);
            pen.line(&[(6.5, 9.5), (6.5, 16.0), (8.5, 19.0), (12.0, 20.5), (20.5, 20.5)]);
            pen.line(&[(17.5, 17.5), (20.5, 20.5), (17.5, 23.0)]);
        }
        Icon::Tape => {
            pen.circle(9.0, 12.0, 6.0);
            pen.circle(9.0, 12.0, 1.8);
            pen.line(&[(9.0, 18.0), (21.0, 18.0), (21.0, 15.0)]);
        }
        Icon::Protractor => {
            pen.arc(12.0, 18.0, 9.0, 9.0, PI, TAU);
            pen.line(&[(3.0, 18.0), (21.0, 18.0)]);
            pen.line(&[(12.0, 18.0), (17.0, 11.0)]);
        }
        Icon::Dimension => {
            pen.line(&[(4.0, 6.0), (4.0, 18.0)]);
            pen.line(&[(20.0, 6.0), (20.0, 18.0)]);
            pen.line(&[(4.0, 12.0), (20.0, 12.0)]);
            pen.line(&[(7.0, 9.5), (4.0, 12.0), (7.0, 14.5)]);
            pen.line(&[(17.0, 9.5), (20.0, 12.0), (17.0, 14.5)]);
        }
        Icon::TextTool => {
            pen.line(&[(5.0, 6.0), (19.0, 6.0)]);
            pen.line(&[(12.0, 6.0), (12.0, 19.0)]);
            pen.line(&[(9.0, 19.0), (15.0, 19.0)]);
        }
        Icon::Orbit => {
            pen.arc(12.0, 12.0, 9.0, 4.0, 0.2, TAU - 0.2);
            pen.circle(12.0, 12.0, 3.0);
            pen.line(&[(19.5, 9.5), (21.0, 11.4), (18.6, 12.4)]);
        }
        Icon::Hand => {
            pen.line(&[(8.0, 13.0), (8.0, 5.5), (10.0, 4.5), (11.0, 6.0), (11.0, 11.0)]);
            pen.line(&[(11.0, 5.0), (12.5, 3.5), (14.0, 5.0), (14.0, 11.0)]);
            pen.line(&[(14.0, 6.0), (15.5, 5.0), (17.0, 6.5), (17.0, 12.0)]);
            pen.line(&[(17.0, 9.0), (18.5, 8.0), (20.0, 9.5), (20.0, 15.0), (17.0, 20.5), (10.0, 20.5), (5.0, 15.5), (4.0, 12.5), (6.0, 11.5), (8.0, 13.0)]);
        }
        Icon::ZoomExtents => {
            pen.circle(11.0, 11.0, 5.5);
            pen.line(&[(15.0, 15.0), (19.0, 19.0)]);
            pen.line(&[(3.0, 7.0), (3.0, 3.0), (7.0, 3.0)]);
            pen.line(&[(17.0, 3.0), (21.0, 3.0), (21.0, 7.0)]);
            pen.line(&[(3.0, 17.0), (3.0, 21.0), (7.0, 21.0)]);
        }
        Icon::Undo => {
            pen.arc(13.0, 13.0, 7.0, 6.0, -PI * 0.95, PI * 0.5);
            pen.line(&[(3.0, 8.0), (6.2, 12.4), (10.0, 9.0)]);
        }
        Icon::Redo => {
            pen.arc(11.0, 13.0, 7.0, 6.0, PI * 0.5, PI * 1.95);
            pen.line(&[(21.0, 8.0), (17.8, 12.4), (14.0, 9.0)]);
        }
        Icon::Cube | Icon::Group => {
            pen.closed(&[(12.0, 3.0), (20.0, 7.5), (20.0, 16.5), (12.0, 21.0), (4.0, 16.5), (4.0, 7.5)]);
            pen.line(&[(4.0, 7.5), (12.0, 12.0), (20.0, 7.5)]);
            pen.line(&[(12.0, 12.0), (12.0, 21.0)]);
        }
        Icon::Component => {
            pen.closed(&[(12.0, 3.0), (20.0, 7.5), (20.0, 16.5), (12.0, 21.0), (4.0, 16.5), (4.0, 7.5)]);
            pen.line(&[(4.0, 7.5), (12.0, 12.0), (20.0, 7.5)]);
            pen.line(&[(12.0, 12.0), (12.0, 21.0)]);
            pen.dot(12.0, 7.5, 1.6);
        }
        Icon::Camera => {
            pen.rect(3.0, 7.0, 18.0, 12.0, 2.0);
            pen.circle(12.0, 13.0, 3.5);
            pen.line(&[(8.0, 7.0), (9.5, 4.5), (14.5, 4.5), (16.0, 7.0)]);
        }
        Icon::Section => {
            pen.closed(&[(5.0, 4.0), (19.0, 8.0), (19.0, 20.0), (5.0, 16.0)]);
            pen.line(&[(12.0, 12.0), (21.5, 12.0)]);
            pen.line(&[(19.0, 10.0), (21.5, 12.0), (19.0, 14.0)]);
        }
        Icon::Xray => {
            pen.rect(4.0, 8.0, 12.0, 12.0, 1.0);
            pen.rect(8.0, 4.0, 12.0, 12.0, 1.0);
        }
        Icon::Shadow => {
            pen.circle(8.0, 8.0, 3.5);
            pen.closed(&[(9.0, 18.0), (20.0, 14.0), (20.0, 20.0), (9.0, 21.0)]);
        }
        Icon::Scenes => {
            pen.rect(3.0, 6.0, 18.0, 13.0, 2.0);
            pen.line(&[(3.0, 10.0), (21.0, 10.0)]);
            pen.line(&[(8.0, 6.0), (8.0, 10.0)]);
            pen.line(&[(13.0, 6.0), (13.0, 10.0)]);
        }
        Icon::Smoove => {
            pen.line(&[(2.0, 19.0), (6.0, 18.0), (9.0, 13.0), (12.0, 6.0), (15.0, 13.0), (18.0, 18.0), (22.0, 19.0)]);
            pen.line(&[(12.0, 3.0), (12.0, 10.0)]);
            pen.line(&[(9.5, 5.5), (12.0, 3.0), (14.5, 5.5)]);
        }
        Icon::Stamp => {
            pen.rect(8.0, 3.0, 8.0, 7.0, 1.0);
            pen.line(&[(2.0, 17.0), (6.0, 14.0), (18.0, 14.0), (22.0, 17.0)]);
            pen.line(&[(2.0, 21.0), (22.0, 21.0)]);
            pen.line(&[(12.0, 10.0), (12.0, 14.0)]);
        }
        Icon::Drape => {
            pen.line(&[(2.0, 19.0), (7.0, 15.0), (12.0, 17.0), (17.0, 13.0), (22.0, 15.0)]);
            pen.line(&[(5.0, 4.0), (19.0, 4.0)]);
            pen.line(&[(7.0, 6.0), (7.0, 13.0)]);
            pen.line(&[(17.0, 6.0), (17.0, 11.0)]);
            pen.line(&[(5.5, 11.5), (7.0, 13.0), (8.5, 11.5)]);
            pen.line(&[(15.5, 9.5), (17.0, 11.0), (18.5, 9.5)]);
        }
        Icon::Detail => {
            pen.closed(&[(12.0, 3.0), (21.0, 20.0), (3.0, 20.0)]);
            pen.closed(&[(7.5, 11.5), (16.5, 11.5), (12.0, 20.0)]);
        }
        Icon::FlipEdge => {
            pen.closed(&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0)]);
            pen.line(&[(4.0, 20.0), (20.0, 4.0)]);
            pen.arc(12.0, 12.0, 5.0, 5.0, 0.4, 2.6);
        }
        Icon::Interact => {
            pen.closed(&[(6.0, 3.0), (6.0, 19.0), (10.0, 15.5), (13.0, 21.0), (15.5, 20.0), (12.5, 14.5), (18.0, 14.0)]);
        }
        Icon::Dynamic => {
            pen.closed(&[(12.0, 3.0), (20.0, 7.5), (20.0, 16.5), (12.0, 21.0), (4.0, 16.5), (4.0, 7.5)]);
            pen.line(&[(8.0, 15.0), (12.0, 9.0), (16.0, 15.0)]);
            pen.line(&[(9.5, 12.8), (14.5, 12.8)]);
        }
        Icon::Wall => {
            pen.rect(3.0, 6.0, 18.0, 12.0, 0.5);
            pen.line(&[(3.0, 12.0), (21.0, 12.0)]);
            pen.line(&[(9.0, 6.0), (9.0, 12.0)]);
            pen.line(&[(15.0, 6.0), (15.0, 12.0)]);
            pen.line(&[(6.0, 12.0), (6.0, 18.0)]);
            pen.line(&[(12.0, 12.0), (12.0, 18.0)]);
            pen.line(&[(18.0, 12.0), (18.0, 18.0)]);
        }
        Icon::Door => {
            pen.line(&[(5.0, 21.0), (5.0, 3.0), (17.0, 3.0), (17.0, 21.0)]);
            pen.line(&[(3.0, 21.0), (21.0, 21.0)]);
            pen.dot(14.0, 12.0, 1.3);
        }
        Icon::Window => {
            pen.rect(4.0, 4.0, 16.0, 16.0, 1.0);
            pen.line(&[(12.0, 4.0), (12.0, 20.0)]);
            pen.line(&[(4.0, 12.0), (20.0, 12.0)]);
        }
        Icon::Slab => {
            pen.closed(&[(2.0, 13.0), (12.0, 8.0), (22.0, 13.0), (12.0, 18.0)]);
            pen.line(&[(2.0, 13.0), (2.0, 16.0), (12.0, 21.0), (22.0, 16.0), (22.0, 13.0)]);
            pen.line(&[(12.0, 18.0), (12.0, 21.0)]);
        }
        Icon::Room => {
            pen.line(&[(3.0, 3.0), (21.0, 3.0), (21.0, 21.0), (3.0, 21.0), (3.0, 3.0)]);
            pen.line(&[(8.0, 12.0), (16.0, 12.0)]);
            pen.line(&[(9.0, 15.5), (15.0, 15.5)]);
        }
        Icon::Column => {
            pen.rect(8.0, 5.0, 8.0, 14.0, 0.5);
            pen.line(&[(5.0, 3.0), (19.0, 3.0)]);
            pen.line(&[(5.0, 21.0), (19.0, 21.0)]);
        }
        Icon::Beam => {
            pen.rect(2.0, 8.0, 20.0, 6.0, 0.5);
            pen.line(&[(5.0, 14.0), (5.0, 21.0)]);
            pen.line(&[(19.0, 14.0), (19.0, 21.0)]);
        }
        Icon::Railing => {
            pen.line(&[(2.0, 7.0), (22.0, 7.0)]);
            pen.line(&[(2.0, 20.0), (22.0, 20.0)]);
            for x in [4.0, 9.0, 14.0, 19.0] {
                pen.line(&[(x, 7.0), (x, 20.0)]);
            }
        }
        Icon::Stair => {
            pen.line(&[(2.0, 21.0), (2.0, 17.0), (7.0, 17.0), (7.0, 12.0), (12.0, 12.0), (12.0, 7.0), (17.0, 7.0), (17.0, 3.0), (22.0, 3.0)]);
        }
        Icon::Ramp => {
            pen.closed(&[(2.0, 20.0), (22.0, 20.0), (22.0, 8.0)]);
        }
        Icon::Comment => {
            pen.closed(&[(3.0, 4.0), (21.0, 4.0), (21.0, 16.0), (10.0, 16.0), (5.0, 20.0), (6.0, 16.0), (3.0, 16.0)]);
            pen.line(&[(7.0, 9.0), (17.0, 9.0)]);
            pen.line(&[(7.0, 12.0), (14.0, 12.0)]);
        }
        Icon::Budget => {
            pen.rect(5.0, 2.0, 14.0, 20.0, 1.5);
            pen.line(&[(8.0, 7.0), (16.0, 7.0)]);
            pen.line(&[(8.0, 11.0), (16.0, 11.0)]);
            pen.line(&[(8.0, 15.0), (12.0, 15.0)]);
            pen.line(&[(14.0, 15.0), (16.0, 15.0)]);
            pen.line(&[(14.0, 18.0), (16.0, 18.0)]);
        }
        Icon::Audit => {
            pen.closed(&[(12.0, 2.0), (20.0, 5.0), (20.0, 11.0), (12.0, 22.0), (4.0, 11.0), (4.0, 5.0)]);
            pen.line(&[(12.0, 7.0), (12.0, 13.0)]);
            pen.dot(12.0, 16.5, 1.2);
        }
        Icon::Versions => {
            pen.circle(6.0, 5.0, 2.2);
            pen.circle(6.0, 19.0, 2.2);
            pen.circle(18.0, 9.0, 2.2);
            pen.line(&[(6.0, 7.2), (6.0, 16.8)]);
            pen.line(&[(18.0, 11.2), (18.0, 13.0), (8.0, 17.0)]);
        }
        Icon::Library => {
            pen.rect(3.0, 4.0, 4.5, 16.0, 0.5);
            pen.rect(8.5, 4.0, 4.5, 16.0, 0.5);
            pen.closed(&[(14.5, 5.5), (18.5, 4.5), (21.5, 19.0), (17.5, 20.0)]);
        }
        Icon::Speaker => {
            pen.closed(&[(3.5, 9.0), (7.5, 9.0), (12.5, 4.5), (12.5, 19.5), (7.5, 15.0), (3.5, 15.0)]);
            pen.arc(13.0, 12.0, 3.5, 3.5, -0.9, 0.9);
            pen.arc(13.0, 12.0, 7.0, 7.0, -0.95, 0.95);
        }
        Icon::SpeakerOff => {
            pen.closed(&[(3.5, 9.0), (7.5, 9.0), (12.5, 4.5), (12.5, 19.5), (7.5, 15.0), (3.5, 15.0)]);
            pen.line(&[(15.5, 9.5), (20.5, 14.5)]);
            pen.line(&[(20.5, 9.5), (15.5, 14.5)]);
        }
        Icon::Expand => {
            pen.line(&[(4.0, 9.0), (4.0, 4.0), (9.0, 4.0)]);
            pen.line(&[(15.0, 4.0), (20.0, 4.0), (20.0, 9.0)]);
            pen.line(&[(20.0, 15.0), (20.0, 20.0), (15.0, 20.0)]);
            pen.line(&[(9.0, 20.0), (4.0, 20.0), (4.0, 15.0)]);
        }
        Icon::Shrink => {
            pen.line(&[(4.0, 9.0), (9.0, 9.0), (9.0, 4.0)]);
            pen.line(&[(15.0, 4.0), (15.0, 9.0), (20.0, 9.0)]);
            pen.line(&[(20.0, 15.0), (15.0, 15.0), (15.0, 20.0)]);
            pen.line(&[(9.0, 20.0), (9.0, 15.0), (4.0, 15.0)]);
        }
        Icon::Remote => {
            pen.rect(2.0, 4.0, 13.0, 10.0, 1.5);
            pen.line(&[(6.0, 17.0), (11.0, 17.0)]);
            pen.rect(11.0, 10.0, 11.0, 9.0, 1.5);
            pen.line(&[(14.0, 21.5), (19.0, 21.5)]);
        }
        Icon::Ruler => {
            pen.closed(&[(3.0, 15.0), (15.0, 3.0), (21.0, 9.0), (9.0, 21.0)]);
            pen.line(&[(7.0, 11.0), (9.0, 13.0)]);
            pen.line(&[(10.0, 8.0), (12.0, 10.0)]);
            pen.line(&[(13.0, 5.0), (15.0, 7.0)]);
        }
    }
}

/// Ícone como widget (ocupa `size`×`size`).
pub fn icon(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), rect, icon, color);
    }
    resp
}
