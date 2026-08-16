# Whisper App — notas para agentes de IA

App desktop de ditado por voz com IA (Windows-first, com portabilidade prevista
para macOS/Linux). Stack: Tauri 2 + React 19 + TypeScript + Rust.

## Arquitetura em camadas

```
React (src/)                  — UI: App (status), Settings, Overlay
    ↕ invoke() / events
Rust (src-tauri/src/)         — lógica de sistema:
  ├── active_app               detecção do app em foco (Win32) para hint do LLM
  ├── audio                    captura via cpal → WAV em memória (thread própria)
  ├── commands                 comandos Tauri chamáveis do frontend
  ├── config                   settings persistentes em JSON
  ├── hotkey                   atalho global configurável
  ├── insert                   clipboard + Ctrl+V via enigo + arboard
  ├── llm                      OpenAI/Anthropic/OpenRouter/Groq/Gemini/xAI para reformatar + traduzir
  ├── models                   download e gerenciamento dos modelos Whisper
  ├── transcription            whisper.cpp local OU OpenAI Whisper API
  └── visual                   overlay flutuante + tray colorido
```

Fluxo do usuário:
`hotkey pressionado → grava áudio → solta → transcreve (local ou cloud) → LLM reformata (com hint do app em foco) → cola no app ativo`.

## Decisões tomadas

- **Tauri 2.x** (não 1.x): APIs de plugins e permissões maduras.
- **whisper-rs 0.16** (não 0.14, que tinha bug no tamanho de `whisper_full_params`).
- **cpal em thread dedicada** com `mpsc::channel`: `cpal::Stream` é `!Send`
  no Windows (WASAPI usa thread-local state), então guarda-lo num state
  compartilhado não funciona. Cada serviço (audio, transcription, llm) tem
  seu próprio pattern de thread + canal.
- **reqwest blocking** com `rustls-tls` (sem OpenSSL nativo, sem tokio).
  Cliente HTTP é **reutilizado** por serviço (mantém pool com keep-alive TLS
  quente — economia significativa de latência por chamada).
- **Inserção via clipboard + Ctrl+V** (`enigo` + `arboard`). Salva/restaura
  clipboard. Funciona em ~95% dos apps cross-platform.
- **Transcrição cloud usa mono 16kHz**: antes de enviar pra OpenAI Whisper
  API, downsample local via `hound` — corta o payload em ~5-10x vs 48kHz
  stereo original. Economia visível em conexões medianas.
- **Warmup do modelo local no boot**: o `TranscriptionService` dispara um
  `Command::Warmup` imediatamente após spawn se o provider é local. Sem
  isso, a 1ª transcrição paga ~1-2s de load do modelo.
- **Adaptação por app ativo**: `active_app::detect()` roda no `Pressed` do
  hotkey (janela alvo ainda tem foco), classifica em Chat/Email/Code/etc.
  e o `llm::build_system_prompt` anexa um hint contextual.

## Config persistente

`Arc<Mutex<AppConfig>>` gerenciado como state Tauri. Todos os campos com
`#[serde(default)]` — arquivos antigos abrem sem migração. Campos:

- `provider` — qual API de LLM usar. Suporta 6: `openai`, `anthropic`,
  `openrouter`, `groq`, `gemini`, `xai`. Os quatro OpenAI-compat
  (openai, openrouter, groq, xai) compartilham `call_openai_compatible()`
  em `llm.rs` — só variam endpoint, chave e headers extras. Gemini tem
  função própria (formato `generateContent`), Anthropic também (`messages`).
- `openai_api_key`, `anthropic_api_key`, `openrouter_api_key`,
  `groq_api_key`, `gemini_api_key`, `xai_api_key` — uma chave por provider,
  todas ficam salvas ao trocar de provider. `openai_api_key` também é a
  chave usada por `transcription_provider = openai_cloud`.
- `llm_model` — vazio usa o default do provider ativo. A UI oferece
  dropdown com modelos curados por provider + opção "personalizado" que
  vira input de texto livre.
- `translate: { enabled, target_language }` — tradução automática.
- `visual_indicator: none|floating|tray|both` — indicador visual.
- `hotkey: String` — atalho no formato accelerator (`"F9"`, `"Ctrl+Shift+K"`,
  `"Alt+Space"`, etc.). Trocar em runtime via `hotkey::replace`.
- `transcription_provider: local|openai_cloud|groq_cloud` — onde
  transcrever. `openai_cloud` e `groq_cloud` compartilham a mesma função
  (`transcribe_cloud` em `transcription.rs`, formato multipart idêntico) —
  Groq roda `whisper-large-v3-turbo` em LPU e costuma ser mais rápido que
  a OpenAI pra ditados curtos.
- `whisper_model: tiny|base|small|medium|large_turbo` — modelo local.
- `adapt_prompt_to_active_app: bool` — enviar hint contextual pro LLM.

Autostart **não** vive no `config.json` — o `tauri-plugin-autostart` já
persiste no registro do SO (comandos `is_autostart_enabled` e `set_autostart`).

## Diferenças por SO

| Área | Windows | macOS | Linux |
|---|---|---|---|
| Hotkey global | nativo | precisa "Input Monitoring" | X11 OK, Wayland limitado |
| Captura de áudio | nativo | precisa mic permission + `NSMicrophoneUsageDescription` | ALSA/PulseAudio |
| Inserção de texto | `SendInput` via enigo | Accessibility API (habilitar em Privacidade) | X11 OK, Wayland precisa `ydotool` |
| Detecção app ativo | `GetForegroundWindow` + `QueryFullProcessImageNameW` | NSWorkspace (**não implementado**) | xdotool/wmctrl (**não implementado**) |
| Autostart | registro `HKCU\...\Run` | LaunchAgent | .desktop autostart |
| Bundler | .msi/.exe | .app/.dmg (Xcode CLT + signing) | .deb/.rpm/.AppImage |
| Build deps | CMake + LLVM + Ninja + MSVC (via VS Build Tools) — script `dev.ps1` | CMake + LLVM + clang (Xcode) | CMake + LLVM + clang + build-essential |

**MVP foca em Windows.** O código Rust usa `#[cfg(target_os = "...")]` onde
faz diferença — módulo `active_app` tem stub `None` fora do Windows.

## Convenções

- Comentários em Rust são generosos (usuário vem de JS/TS).
- Toda diferença cross-platform: `#[cfg(target_os = "...")]` + comentário
  `// PLATAFORMA:`.
- Chaves de API **nunca** hardcoded — sempre via `%APPDATA%\com.rlucio.whisperapp\config.json`.
- Cada serviço em background: `struct XService { cmd_tx: mpsc::Sender<Cmd> }`
  + thread própria + handle guardado via `app.manage()`.
- Comandos Tauri retornam `Result<T, String>` (não `anyhow`) — só serializable
  atravessa a ponte JSON.

## Comandos úteis

**Windows** — sempre via wrapper:

```powershell
.\scripts\dev.ps1              # dev com hot reload
.\scripts\dev.ps1 build        # bundle .msi/.exe
cd src-tauri; cargo check      # validação Rust rápida
npx tsc --noEmit               # validação TypeScript
```

Rodar `npm run tauri dev` direto num PowerShell "normal" falha: o `whisper-rs`
precisa das env vars do MSVC (DevShell VS 2019) + `LIBCLANG_PATH` do LLVM +
`CMAKE_GENERATOR=Ninja` + `VSINSTALLDIR` removido. O script faz tudo.

## Modelos do Whisper

Baixe pela UI (Configurações → Transcrição → Local). Localização:
`%APPDATA%\com.rlucio.whisperapp\models\<filename>.bin`.

| Slug | Arquivo | Tamanho | Nota |
|---|---|---|---|
| `tiny` | `ggml-tiny-q5_1.bin` | ~31 MB | Mais rápido, menos preciso |
| `base` | `ggml-base-q5_1.bin` | ~59 MB | Testes |
| `small` | `ggml-small-q5_1.bin` | ~181 MB | **Default**, uso diário |
| `medium` | `ggml-medium-q5_1.bin` | ~514 MB | Mais preciso, mais lento |
| `large_turbo` | `ggml-large-v3-turbo-q5_0.bin` | ~574 MB | Máxima precisão |

Download com progresso via `models::spawn_download` — escreve em `.part` e
renomeia atomicamente. Eventos emitidos:
`model-download-progress` / `-complete` / `-error`.

Modos cloud alternativos: OpenAI Whisper API (`whisper-1`) ou Groq
(`whisper-large-v3-turbo`, mais rápido). Áudio é downsampled pra mono 16kHz
antes do upload em ambos.

## Eventos Tauri emitidos

Frontend escuta:
- `hotkey-pressed` / `hotkey-released`
- `recording-saved` / `recording-error`
- `transcription-status` (`carregando_modelo`, `transcrevendo`, `enviando_para_openai`)
- `transcription-complete` / `transcription-error`
- `formatting-started` / `format-complete` / `format-error`
- `text-inserted` / `insert-error`
- `model-download-progress` / `-complete` / `-error`

## Estado atual

MVP + V2 fechados (Windows). Próximos passos possíveis:
- Portar `active_app` pra macOS/Linux.
- Streaming da resposta do LLM (colar por partes) — reduziria latência
  percebida em cloud.
- Whisper.cpp com GPU (CUDA/DirectML) — build ficaria mais complexo.

Ver memory files em `C:\Users\rafae\.claude\projects\C--DADOS-WEBAPPS-whisper-app\memory\`
para preferências e histórico específicos do usuário.
