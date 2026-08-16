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
mod hotkey;
mod insert;
mod llm;
mod models;
mod sound;
mod transcription;
mod visual;

use std::sync::{Arc, Mutex};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // Autostart: registra o app no Startup do SO. No Windows escreve em
        // `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` — não precisa
        // de admin. `LaunchAgent` só afeta macOS. Sem argumentos extra: o app
        // sobe normal e vai direto pro tray (janela principal fica escondida
        // até o usuário clicar no ícone).
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
            let shared_config: config::SharedConfig = Arc::new(Mutex::new(loaded));
            app.manage(shared_config.clone());

            // State para o app em foco no último F9. O hotkey escreve, o LLM lê.
            let shared_active_app: active_app::SharedActiveApp =
                Arc::new(Mutex::new(None));
            app.manage(shared_active_app);

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
            commands::get_config,
            commands::save_config,
            commands::open_config_folder,
            commands::pause_hotkey,
            commands::resume_hotkey,
            commands::list_whisper_models,
            commands::download_whisper_model,
            commands::delete_whisper_model,
            commands::is_autostart_enabled,
            commands::set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o app Tauri");
}

/// Cria o ícone da bandeja com menu de contexto e handlers de clique.
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    // MenuItem::with_id — o `id` é como identificamos qual item foi clicado
    // depois, dentro do `on_menu_event`.
    let show = MenuItem::with_id(app, "show", "Mostrar janela", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        // Usa o mesmo ícone da janela principal (definido em tauri.conf.json).
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        // No Windows a convenção é: clique esquerdo abre a janela, clique
        // direito abre o menu. Sem isso, o menu abriria em ambos os cliques.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
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

/// Mostra a janela principal e traz para o foco.
/// Usado tanto pelo menu "Mostrar" quanto pelo clique esquerdo no tray.
fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
