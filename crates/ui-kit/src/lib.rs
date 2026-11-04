//! Kit de interface do Stringari Design System para egui: tokens, tipografia, componentes
//! e ícones vetoriais. Usado pelo Relay (e pronto para o Atlas e próximos produtos).

pub mod icons;
pub mod theme;

pub use icons::{Icon, icon, paint};
pub use theme::*;
