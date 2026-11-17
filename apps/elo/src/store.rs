// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only
//! Elo instalado pela Microsoft Store (pacote MSIX, `packaging/windows/msix`).
//!
//! Dentro do pacote, duas coisas mudam:
//! - atualização: vem pela loja (é regra dela); o Elo não baixa a própria;
//! - início com o computador: o valor em `...\CurrentVersion\Run` ficaria
//!   num registro virtual do pacote, que o Windows não lê. Vale a tarefa de
//!   inicialização declarada no manifesto ([`TASK`]), ligada pela API do
//!   Windows; ela abre o Elo sem argumentos, e [`started_by_system`] diz que
//!   foi ela (para começar minimizado, como o [`crate::autostart::ARG`]).

/// `TaskId` do `desktop:StartupTask` no `AppxManifest.xml`.
#[cfg(target_os = "windows")]
const TASK: &str = "EloInicio";

/// Rodando de dentro de um pacote MSIX?
pub fn packaged() -> bool {
    #[cfg(target_os = "windows")]
    {
        static P: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *P.get_or_init(|| windows::ApplicationModel::Package::Current().is_ok())
    }
    #[cfg(not(target_os = "windows"))]
    false
}

/// Aberto pela tarefa de inicialização do Windows (login)?
pub fn started_by_system() -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::ApplicationModel::Activation::ActivationKind;
        packaged() && windows::ApplicationModel::AppInstance::GetActivatedEventArgs().and_then(|a| a.Kind()).is_ok_and(|k| k == ActivationKind::StartupTask)
    }
    #[cfg(not(target_os = "windows"))]
    false
}

#[cfg(target_os = "windows")]
fn task() -> Result<windows::ApplicationModel::StartupTask, String> {
    windows::ApplicationModel::StartupTask::GetAsync(&TASK.into()).and_then(|op| op.join()).map_err(|e| format!("tarefa de inicialização: {e}"))
}

/// Liga ou desliga a tarefa de inicialização.
#[cfg(target_os = "windows")]
pub fn set_startup(on: bool) -> Result<(), String> {
    use windows::ApplicationModel::StartupTaskState as S;
    let t = task()?;
    if !on {
        return t.Disable().map_err(|e| e.to_string());
    }
    match t.RequestEnableAsync().and_then(|op| op.join()).map_err(|e| e.to_string())? {
        S::Enabled | S::EnabledByPolicy => Ok(()),
        S::DisabledByUser => Err(t!("desligado nas Configurações do Windows (Aplicativos > Inicialização): ligue o Elo por lá").into()),
        _ => Err(t!("o Windows não deixou (política do computador)").into()),
    }
}

#[cfg(target_os = "windows")]
pub fn startup_enabled() -> bool {
    use windows::ApplicationModel::StartupTaskState as S;
    task().and_then(|t| t.State().map_err(|e| e.to_string())).is_ok_and(|s| s == S::Enabled || s == S::EnabledByPolicy)
}
