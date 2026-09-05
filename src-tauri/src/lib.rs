// Entry point da lógica Rust do app.
//
// Responsabilidades desta função:
//   1. Registrar os plugins Tauri (opener, global-shortcut).
//   2. Carregar o config do disco e guardar como state (compartilhado).
//   3. Subir os serviços de background (áudio + transcrição + LLM).
//   4. Criar o ícone na bandeja do sistema com menu (Mostrar / Sair).
//   5. Interceptar o "fechar janela" para esconder no tray em vez de sair —
//      o app precisa continuar rodando em background para o hotkey funcionar.
//   6. Registrar o atalho global (F9 push-to-talk).

mod active_app;
mod audio;
mod commands;
mod config;
mod history;
mod hotkey;
mod insert;
mod llm;
mod modkey;
mod models;
mod sound;
mod transcription;
mod visual;

use std::sync::{Arc, Mutex};

use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

/// Mantém uma referência ao item de tradução do tray para que alterações
/// feitas tanto pelo menu quanto pela tela de Configurações atualizem a marca.
struct TranslationTrayItem(CheckMenuItem<tauri::Wry>);

/// Grava panics em `crash.log` (mesma pasta do `config.json`) antes do
/// processo morrer. Sem isso, um panic em qualquer thread — incluindo o
/// `setup()` — é invisível: o binário roda com `panic = "abort"` (ver
/// Cargo.toml), então o processo inteiro some sem deixar rastro nenhum,
/// nem no Event Viewer do Windows. Instalado como a primeira coisa em
/// `run()`, antes de qualquer coisa que possa panicar.
fn install_panic_hook() {
    let log_path = std::env::var("APPDATA")
        .map(|appdata| std::path::PathBuf::from(appdata).join("com.rlucio.whisperapp"));
    std::panic::set_hook(Box::new(move |info| {
        let Ok(dir) = &log_path else { return };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("crash.log"))
        {
            use std::io::Write;
            let _ = writeln!(f, "[{now}] {info}");
        }
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_panic_hook();

    tauri::Builder::default()
        // Precisa ser o primeiro plugin registrado. Se já existir uma
        // instância rodando, o callback dispara nela (em vez de deixar o
        // processo novo continuar) — usado aqui pra só mostrar a janela
        // existente, evitando instâncias duplicadas (hotkey registrado
        // duas vezes, dois ícones na bandeja etc.).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        // Auto-update: expõe `check()` / `downloadAndInstall()` pro frontend
        // (ver Settings.tsx e App.tsx). Não sobe thread nem processo próprio —
        // só faz uma requisição HTTP quando chamado, então não pesa no app
        // parado no tray.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Reinicia o app depois de instalar a atualização (`relaunch()`).
        .plugin(tauri_plugin_process::init())
        // Autostart: registra o app no Startup do SO. No Windows escreve em
        // `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` — não precisa
        // de admin. `LaunchAgent` só afeta macOS.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // O plugin de atalho global precisa ser adicionado no Builder ANTES
        // de qualquer chamada a `app.global_shortcut()`.
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // `setup` roda uma vez após o app inicializar. É o lugar canônico para
        // criar tray icons, subir threads de background, registrar atalhos etc.
        .setup(|app| {
            // Carrega config do disco (ou defaults se ainda não existe).
            // Um erro aqui é logado mas não fatal — seguimos com defaults.
            let loaded = config::load(app.handle()).unwrap_or_else(|e| {
                eprintln!("[config] falha ao carregar, usando defaults: {:#}", e);
                config::AppConfig::default()
            });
            let start_minimized = loaded.start_minimized;
            let shared_config: config::SharedConfig = Arc::new(Mutex::new(loaded));
            app.manage(shared_config.clone());

            // State para o app em foco no último F9. O hotkey escreve, o LLM lê.
            let shared_active_app: active_app::SharedActiveApp =
                Arc::new(Mutex::new(None));
            app.manage(shared_active_app);

            // Flag compartilhado entre push-to-talk e hands-free — ver
            // comentário no topo de hotkey.rs sobre por que é unificado.
            let shared_recording_active: hotkey::SharedRecordingActive =
                Arc::new(Mutex::new(false));
            app.manage(shared_recording_active);

            // Último texto formatado, usado pelo atalho "recolar última
            // transcrição". Semeado com a entrada mais recente do histórico
            // (se houver) — assim o atalho já funciona logo após o boot,
            // sem precisar ditar algo primeiro nessa sessão.
            let last_transcript_seed = history::list(app.handle())
                .ok()
                .and_then(|list| list.into_iter().next())
                .map(|e| e.formatted_text);
            let shared_last_transcript: llm::SharedLastTranscript =
                Arc::new(Mutex::new(last_transcript_seed));
            app.manage(shared_last_transcript);

            // Sobe as threads de background e guarda os handles como state.
            // Ordem importa (dependência entre serviços):
            //   1. LlmService     (final da cadeia)
            //   2. TranscriptionService (chama LlmService via state)
            //   3. AudioService   (chama TranscriptionService via state)
            app.manage(llm::LlmService::spawn(app.handle().clone(), shared_config));
            app.manage(transcription::TranscriptionService::spawn(
                app.handle().clone(),
            ));
            app.manage(audio::AudioService::spawn(app.handle().clone()));

            build_tray(app)?;
            hotkey::register(app.handle())?;

            // A janela principal nasce escondida (`"visible": false` no
            // tauri.conf.json) para não piscar antes de sabermos se o
            // usuário quer o boot direto na bandeja. Mostramos aqui, a
            // menos que `start_minimized` esteja ligado.
            if !start_minimized {
                show_main_window(app.handle());
            }
            Ok(())
        })
        // Interceptar o fechamento da janela: em vez de encerrar o app,
        // apenas escondemos — o app continua rodando no tray e o hotkey
        // continua ativo. Para sair de verdade, o usuário usa o menu do tray.
        // Só aplicamos isso à janela "main" — o overlay tem seu próprio ciclo
        // de vida controlado pelo módulo `visual`.
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_version,
            commands::get_config,
            commands::save_config,
            commands::open_config_folder,
            commands::pause_hotkey,
            commands::resume_hotkey,
            commands::list_microphones,
            commands::list_whisper_models,
            commands::download_whisper_model,
            commands::delete_whisper_model,
            commands::is_autostart_enabled,
            commands::set_autostart,
            commands::get_history,
            commands::delete_history_entry,
            commands::clear_history,
            commands::repaste_text,
            commands::cancel_recording,
            commands::confirm_recording,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o app Tauri");
}

/// Cria o ícone da bandeja com menu de contexto e handlers de clique.
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    // Item de versão: só informativo (enabled=false, não dispara clique) —
    // ajuda a identificar rapidamente qual build está rodando, já que o
    // update é manual (ver CLAUDE.md) e é fácil esquecer qual versão foi
    // reinstalada por último.
    let version_label = format!("v{}", app.package_info().version);
    let version = MenuItem::with_id(app, "version", &version_label, false, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let translate_enabled = app
        .state::<config::SharedConfig>()
        .lock()
        .map(|cfg| cfg.translate.enabled)
        .unwrap_or(false);
    let translation = CheckMenuItem::with_id(
        app,
        "toggle-translation",
        "Tradução automática",
        true,
        translate_enabled,
        None::<&str>,
    )?;
    app.manage(TranslationTrayItem(translation.clone()));

    // MenuItem::with_id — o `id` é como identificamos qual item foi clicado
    // depois, dentro do `on_menu_event`.
    let show = MenuItem::with_id(app, "show", "Mostrar janela", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&version, &separator, &translation, &show, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        // Usa o mesmo ícone da janela principal (definido em tauri.conf.json).
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        // No Windows a convenção é: clique esquerdo abre a janela, clique
        // direito abre o menu. Sem isso, o menu abriria em ambos os cliques.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle-translation" => toggle_translation(app),
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Clique esquerdo (evento `Up` = ao soltar o botão) abre a janela.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// Alterna a tradução pelo tray, persistindo exatamente no mesmo config que
/// a tela de Configurações usa. Se a escrita falhar, o item visual volta ao
/// estado anterior para não prometer uma mudança que não foi salva.
fn toggle_translation<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let state = app.state::<config::SharedConfig>();
    let Ok(mut guard) = state.lock() else {
        return;
    };
    let previous = guard.translate.enabled;
    guard.translate.enabled = !previous;
    let updated = guard.clone();
    drop(guard);

    if config::save(app, &updated).is_err() {
        if let Ok(mut guard) = state.lock() {
            guard.translate.enabled = previous;
        }
    }
    let current_enabled = state
        .lock()
        .map(|cfg| cfg.translate.enabled)
        .unwrap_or(previous);
    set_translation_tray_checked(app, current_enabled);
}

/// Sincroniza a marca do item de tray quando o config é salvo por qualquer
/// caminho (menu ou tela de Configurações).
pub(crate) fn set_translation_tray_checked<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    enabled: bool,
) {
    if let Some(item) = app.try_state::<TranslationTrayItem>() {
        let _ = item.0.set_checked(enabled);
    }
}

/// Mostra a janela principal e traz para o foco.
/// Usado tanto pelo menu "Mostrar" quanto pelo clique esquerdo no tray.
fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
