// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only
//! Kit de interface do Stringari Design System para egui: tokens, tipografia, componentes
//! e ícones vetoriais. Usado pelo Relay (e pronto para o Atlas e próximos produtos).

pub mod icons;
pub mod theme;

pub use icons::{Icon, icon, paint};
pub use theme::*;
