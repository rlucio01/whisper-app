//! Detecção do app em foco quando o usuário aciona o hotkey.
//!
//! Usada pelo LLM para adaptar o prompt de reformatação ao contexto:
//! chat casual (Slack, WhatsApp) x email formal (Outlook) x IDE (VS Code) etc.
//!
//! ## Portabilidade
//!
//! - Windows: usa Win32 (`GetForegroundWindow` + `GetWindowThreadProcessId`
//!   + `QueryFullProcessImageNameW` + `GetWindowTextW`).
//! - macOS/Linux: por ora retorna `None` (stub). Quando adicionarmos suporte
//!   a esses SOs, cada um vai precisar da sua própria implementação
//!   (NSWorkspace no macOS, xdotool/wmctrl no Linux).

use std::sync::{Arc, Mutex};

/// Informações do app em foco no momento em que o usuário apertou o hotkey.
#[derive(Debug, Clone, Default)]
pub struct ActiveApp {
    /// Nome do executável, ex: `"slack.exe"`, `"chrome.exe"`, `"Code.exe"`.
    pub exe_name: String,
    /// Título da janela ativa, ex: `"#geral - Empresa - Slack"`.
    pub window_title: String,
    /// HWND (Windows) da janela que estava em foco no momento do hotkey.
    /// Sempre `None` fora do Windows. Usado por `insert::paste_text` para
    /// restaurar o foco antes de colar — protege contra qualquer coisa
    /// (ex: um clique nos controles do overlay) que roube o foco durante o
    /// pipeline de gravação/transcrição/formatação, que leva alguns segundos.
    pub target_hwnd: Option<isize>,
}

/// State compartilhado: o hotkey handler escreve, o LlmService lê.
/// `Option::None` = nunca detectado ou detecção falhou.
pub type SharedActiveApp = Arc<Mutex<Option<ActiveApp>>>;

// ---------- Windows ----------

#[cfg(target_os = "windows")]
pub fn detect() -> Option<ActiveApp> {
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    };

    // PLATAFORMA: Windows — captura a janela em foco no momento da chamada.
    // Se retornou 0 (sem foreground), aborta.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }

    // Descobre o PID do processo dono da janela.
    let mut pid: u32 = 0;
    let _tid = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return None;
    }

    // Título da janela — GetWindowTextLengthW retorna o tamanho sem o \0 final;
    // alocamos +1 para caber o terminador.
    let title_len = unsafe { GetWindowTextLengthW(hwnd) };
    let window_title = if title_len > 0 {
        let mut buf = vec![0u16; (title_len + 1) as usize];
        let copied = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        if copied > 0 {
            String::from_utf16_lossy(&buf[..copied as usize])
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Abre o processo com o mínimo de privilégios necessários. `PROCESS_QUERY_
    // LIMITED_INFORMATION` funciona mesmo para processos com integridade mais
    // alta que a do nosso app (ao contrário do PROCESS_QUERY_INFORMATION).
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        // Não conseguimos abrir — provavelmente é um processo protegido do
        // sistema (ex: janela de login). Retornamos só o título mesmo.
        return Some(ActiveApp {
            exe_name: String::new(),
            window_title,
            target_hwnd: Some(hwnd as isize),
        });
    }

    let mut buf = vec![0u16; MAX_PATH as usize];
    let mut size: u32 = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) };
    unsafe { CloseHandle(handle) };

    let exe_name = if ok != 0 && size > 0 {
        let path_os = std::ffi::OsString::from_wide(&buf[..size as usize]);
        PathBuf::from(path_os)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        String::new()
    };

    Some(ActiveApp {
        exe_name,
        window_title,
        target_hwnd: Some(hwnd as isize),
    })
}

// ---------- Não-Windows (stub) ----------

#[cfg(not(target_os = "windows"))]
pub fn detect() -> Option<ActiveApp> {
    // PLATAFORMA: implementar quando adicionarmos suporte a macOS/Linux.
    None
}

// ---------- Categorização para o prompt do LLM ----------

/// Categoria de contexto derivada do nome do exe. Usada para ajustar o tom
/// da reformatação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCategory {
    Chat,
    Email,
    Code,
    Document,
    Terminal,
    Browser,
    Other,
}

impl ActiveApp {
    /// Classifica o app baseado no nome do exe (case-insensitive).
    /// Mapeamento conservador — quando na dúvida, retornamos `Other` e o LLM
    /// usa o prompt padrão sem hint contextual.
    pub fn category(&self) -> AppCategory {
        let name = self.exe_name.to_lowercase();
        match name.as_str() {
            // Chat / mensageria — tom casual, respostas curtas.
            "slack.exe" | "discord.exe" | "whatsapp.exe" | "telegram.exe"
            | "teams.exe" | "signal.exe" | "messenger.exe" | "skype.exe" => AppCategory::Chat,

            // Email — tom mais formal, estrutura de mensagem.
            "outlook.exe" | "thunderbird.exe" | "mailspring.exe" | "hey.exe" => {
                AppCategory::Email
            }

            // Editores de código — preservar termos técnicos e nomes de vars.
            "code.exe" | "cursor.exe" | "devenv.exe" | "idea64.exe" | "webstorm64.exe"
            | "pycharm64.exe" | "rustrover64.exe" | "sublime_text.exe" | "atom.exe"
            | "zed.exe" | "notepad++.exe" => AppCategory::Code,

            // Editores de documento — prosa longa, formal.
            "winword.exe" | "wps.exe" | "libreoffice.exe" | "soffice.exe"
            | "notion.exe" | "obsidian.exe" | "typora.exe" => AppCategory::Document,

            // Terminais — provavelmente comandos, sem "reformatar".
            "windowsterminal.exe" | "wt.exe" | "powershell.exe" | "pwsh.exe"
            | "cmd.exe" | "conhost.exe" | "alacritty.exe" | "wezterm-gui.exe" => {
                AppCategory::Terminal
            }

            // Navegadores — genérico (o conteúdo depende da aba). Poderíamos
            // olhar o título da janela pra refinar (Gmail? WhatsApp Web?)
            // mas por ora tratamos como categoria própria e usamos um hint leve.
            "chrome.exe" | "firefox.exe" | "msedge.exe" | "brave.exe" | "opera.exe"
            | "vivaldi.exe" | "arc.exe" | "zen.exe" => AppCategory::Browser,

            _ => AppCategory::Other,
        }
    }
}
