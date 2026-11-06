//! Mensagens da sessão (sempre dentro do canal cifrado).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tile {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// JPEG (ou `raw` RGBA quando pequeno e sem perdas).
    pub data: Vec<u8>,
    pub raw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// Teclas especiais (as de texto vão como `Text`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    Enter,
    Tab,
    Backspace,
    Delete,
    Escape,
    Space,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    F(u8),
    Shift,
    Control,
    Alt,
    Meta,
    /// Letra/dígito físico (para atalhos: Ctrl+C etc.).
    Char(char),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Input {
    /// Posição em pixels da tela remota.
    MouseMove { x: i32, y: i32 },
    MouseButton { button: Button, down: bool },
    Scroll { dx: i32, dy: i32 },
    Key { key: Key, down: bool },
    Text(String),
    /// Ctrl+Alt+Del e outros atalhos do sistema.
    SecureAttention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    pub control: bool,
    pub clipboard: bool,
    pub files: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self { control: true, clipboard: true, files: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Monitor {
    pub index: u8,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Msg {
    /// Anfitrião → visualizador, logo após o aceite.
    Welcome { name: String, os: String, monitors: Vec<Monitor>, permissions: Permissions },
    /// Visualizador → anfitrião.
    Hello { name: String, from_id: String },
    /// Início de quadro: tamanho da tela (muda se a resolução mudar).
    FrameStart { seq: u32, width: u32, height: u32, full: bool },
    Tiles(Vec<Tile>),
    /// Fim do quadro, com o cursor.
    FrameEnd { seq: u32, cursor: Option<(i32, i32)> },
    /// Visualizador confirma que desenhou (controle de fluxo).
    Ack { seq: u32 },
    Input(Input),
    Clipboard(String),
    Chat(String),
    /// Qualidade JPEG (10–95) e quadros por segundo máximos.
    Quality { jpeg: u8, fps: u8 },
    SelectMonitor(u8),
    /// Pede quadro completo.
    Refresh,
    /// `dir`: pasta de destino no anfitrião (gerenciador de arquivos); sem ela,
    /// vai para a pasta de recebidos. O visualizador sempre usa a dele.
    FileOffer { id: u32, name: String, size: u64, dir: Option<String> },
    FileAccept { id: u32 },
    FileChunk { id: u32, data: Vec<u8> },
    FileEnd { id: u32 },
    FileCancel { id: u32 },
    Permissions(Permissions),
    Ping(u64),
    Pong(u64),
    Bye(String),
    /// Visualizador pede o conteúdo de uma pasta do anfitrião ("" = pasta pessoal).
    ListDir { path: String },
    /// Resposta ao `ListDir`: caminho completo, pasta de cima e itens (ou erro).
    DirListing { path: String, parent: Option<String>, entries: Vec<DirEntry>, error: Option<String> },
    /// Visualizador pede um arquivo do anfitrião (chega como `FileOffer`).
    FileRequest { path: String },
    /// Aviso curto para mostrar ao outro lado (ex.: arquivo não encontrado).
    Notice(String),
    /// Visualizador pede para reiniciar o computador do anfitrião (exige
    /// permissão de controle). O anfitrião responde `Bye(BYE_RESTART)`.
    Restart,
}

/// Motivo do `Bye` quando o anfitrião vai reiniciar (o visualizador reconecta).
pub const BYE_RESTART: &str = "o computador remoto está reiniciando";

/// Item de uma pasta no gerenciador de arquivos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    /// Segundos desde 1970 (0 = desconhecido).
    pub modified: u64,
}
