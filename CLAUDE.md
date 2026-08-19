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
  ├── history                  histórico de ditados (JSON Lines em disco)
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
- **`tauri-plugin-single-instance`** registrado como o primeiro plugin do
  builder: sem ele, cada clique no ícone (ou o autostart disparando com o
  app já de pé) sobe um processo Tauri novo — hotkey registrado duas vezes,
  dois ícones na bandeja, e cada processo com seu próprio state em memória
  (fonte de bugs sutis tipo a UI mostrar um hotkey desatualizado se o
  processo errado responde ao `get_config`). O callback do plugin só foca a
  janela da instância existente.
- **Hook de panic gravando em `crash.log`** (`install_panic_hook` em
  `lib.rs`, instalado antes de qualquer outra coisa em `run()`): o profile
  de release usa `panic = "abort"`, então um panic em qualquer thread —
  inclusive dentro do `setup()`, como os `.expect()` de criação do client
  HTTP em `llm.rs`/`transcription.rs` — mata o processo inteiro sem deixar
  rastro nenhum (nem no Event Viewer). Isso explica falhas tipo "o app não
  abriu depois que o Windows iniciou" sem nenhum log pra investigar.
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
- **Foco restaurado antes de colar**: `active_app::detect()` também guarda o
  `target_hwnd` da janela em foco no momento do hotkey. `insert::paste_text`
  recebe esse HWND e, no Windows, chama `SetForegroundWindow` (via
  `AttachThreadInput`, necessário porque o Windows bloqueia apps em
  background de roubar foco) logo antes do Ctrl+V — protege contra qualquer
  coisa que roube o foco durante os segundos de transcrição/formatação,
  incluindo cliques nos controles do próprio overlay.
- **Toda janela nova precisa entrar em `capabilities/*.json`**: o Tauri 2
  nega permissões (inclusive `event.listen`) por padrão pra qualquer janela
  fora do array `windows` de uma capability — a falha é silenciosa (promise
  rejeitada sem crash) e não aparece em `cargo check`/`tsc`. A janela
  `overlay` ficou de fora por um bom tempo sem que isso fosse percebido: o
  estado inicial da UI (`"Gravando"` com dot vermelho) coincidia
  visualmente com o que a tela deveria mostrar mesmo sem nenhum evento
  chegando, então o bug só ficou óbvio quando a onda de áudio e o
  cronômetro do overlay simplesmente não se moviam. Se uma nova janela for
  criada em `tauri.conf.json`, adicione o label dela em
  `capabilities/default.json` (ou crie uma capability dedicada).
- **`tauri-plugin-updater` + `tauri-plugin-process`** para auto-update: não
  sobem thread/serviço próprio — só código que roda sob demanda (`check()` /
  `downloadAndInstall()` chamados do frontend), então não pesam no processo
  parado no tray. Ver seção "Auto-update" mais abaixo para o fluxo completo
  (chave de assinatura, `latest.json`, repo público).

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
- `hotkey: String` — atalho push-to-talk (segura grava, solta para), formato
  accelerator (`"F9"`, `"Ctrl+Shift+K"`, `"Alt+Space"`, etc.).
- `hands_free_hotkey: String` — atalho opcional pro modo hands-free (toca
  uma vez pra começar, toca de novo pra parar). Vazio = desativado. Os dois
  compartilham um único flag de "gravação em curso" (`hotkey::SharedRecordingActive`)
  — soltar o push-to-talk sempre encerra a gravação, mesmo se ela tiver
  começado via hands-free.
- `repaste_hotkey: String` — atalho opcional que recola a última transcrição
  formatada no app ativo, sem regravar. Vazio = desativado. Lê de
  `llm::SharedLastTranscript`, que `llm.rs` atualiza a cada ditado
  bem-sucedido e o `setup()` semeia com a entrada mais recente do histórico
  no boot (funciona mesmo antes do primeiro ditado da sessão).
- Os três atalhos (`hotkey`, `hands_free_hotkey`, `repaste_hotkey`) são
  validados como mutuamente distintos e re-registrados juntos via
  `hotkey::sync` sempre que `save_config` detecta mudança em qualquer um.
- `transcription_provider: local|openai_cloud|groq_cloud` — onde
  transcrever. `openai_cloud` e `groq_cloud` compartilham a mesma função
  (`transcribe_cloud` em `transcription.rs`, formato multipart idêntico) —
  Groq roda `whisper-large-v3-turbo` em LPU e costuma ser mais rápido que
  a OpenAI pra ditados curtos.
- `whisper_model: tiny|base|small|medium|large_turbo` — modelo local.
- `microphone: String` — nome do dispositivo de entrada (`cpal::Device::name()`).
  Vazio = device default do SO. `audio.rs` resolve o device no momento do
  `Command::Start` (lê `SharedConfig` direto na thread de áudio); se o
  dispositivo salvo não existir mais, a gravação falha com erro amigável.
  Comando `list_microphones` (usa `audio::list_devices()`) alimenta o
  dropdown em settings.
- `adapt_prompt_to_active_app: bool` — enviar hint contextual pro LLM.
- `start_minimized: bool` — se `true`, sobe direto na bandeja sem mostrar a
  janela principal. A janela nasce com `"visible": false` em
  `tauri.conf.json` (evita flash) e `lib.rs` decide se mostra logo após o
  `setup()`, lendo esse campo do config recém-carregado.

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
- `audio-level` (payload `f32` em `[0, 1]`, ~30/s durante a gravação) /
  `recording-cancelled`

## Estado atual

MVP + V2 fechados (Windows). Multi-provider LLM (OpenAI/Anthropic/
OpenRouter/Groq/Gemini/xAI) + transcrição cloud via Groq + feedback sonoro
+ autostart fechados também. Próximos passos possíveis:
- Portar `active_app` pra macOS/Linux.
- Streaming da resposta do LLM (colar por partes) — reduziria latência
  percebida em cloud.
- Whisper.cpp com GPU (CUDA/DirectML) — build ficaria mais complexo.

**Modo hands-free** — implementado (`hands_free_hotkey` no config, opcional).
Ver `hotkey.rs`.

**Histórico de ditados** — implementado. Cada transcrição finalizada (com
texto não-vazio) é gravada em `history.jsonl` no `app_data_dir` — ver
`history.rs`. Comandos: `get_history`, `delete_history_entry`,
`clear_history`, `repaste_text`. UI em `History.tsx`, acessível pelo ícone
🕓 no cabeçalho principal — busca, contagem de palavras total, agrupamento
por dia, copiar/colar de novo/apagar por entrada. Sem limite de retenção
por ora (ver item de retenção abaixo).

Roadmap levantado a partir de telas de um concorrente (Amical), avaliado
em 2026-08-16:
- Retenção/limite de histórico (ex: "nunca apagar" vs "apagar após N dias")
  — hoje cresce sem limite; aceitável pra uso pessoal mas pode virar setting
  em Avançado se incomodar.
**Colar última transcrição** — implementado (`repaste_hotkey`, opcional).
Ver `hotkey.rs` (`repaste_handler`) e `llm::SharedLastTranscript`.

**Botão de copiar o resultado** — implementado. Na tela principal, o card
"Resultado:" tem um botão ⧉ que copia o texto formatado via
`navigator.clipboard.writeText` (mesmo padrão do botão de copiar em
`History.tsx`).

**Iniciar apenas na bandeja** — implementado (`start_minimized` no config,
opcional). Ver seção "Config persistente" acima.

**Seleção de microfone** — implementado (`microphone` no config, vazio =
default do SO). Ver `audio::list_devices()` e `commands::list_microphones`.

**Onda de áudio + controles no hover do overlay** — implementado (v0.3.0).
Durante a gravação, `audio::audio_thread_loop` amostra o RMS da cauda
recente do buffer a cada ~33ms (`recv_timeout` no lugar de `recv` — o
timeout é o próprio tick) e emite `audio-level`; `Overlay.tsx` desenha isso
como uma onda de barras em vez do texto fixo. Passar o mouse por cima troca
a onda por: cancelar (✕, `commands::cancel_recording` →
`audio::Command::Cancel`, descarta o áudio sem transcrever), cronômetro, e
concluir agora (✓, `commands::confirm_recording` → mesma lógica de
`hotkey::end_recording`, como se o atalho tivesse sido solto). Os estados
de transcrição/formatação continuam com o dot+label de antes.

**Auto-update** — implementado (v0.4.0+). O app checa uma vez no boot
(silencioso — falha de rede/repo não mostra nada) e tem um botão "Verificar
agora" em Configurações; hook compartilhado em `useUpdater.ts` (usado por
`App.tsx`, que mostra um banner discreto, e `Settings.tsx`, que mostra a
seção "Atualizações"). Fluxo: `check()` do `@tauri-apps/plugin-updater`
consulta o endpoint configurado em `tauri.conf.json`
(`plugins.updater.endpoints`), que aponta pro
`releases/latest/download/latest.json` do repo GitHub; se a versão for
maior, `update.downloadAndInstall()` baixa o instalador assinado, valida a
assinatura contra a chave pública embutida no `tauri.conf.json`, instala
(modo `passive` — mostra progresso, sem precisar clicar em nada) e
`relaunch()` (`@tauri-apps/plugin-process`) reinicia o app.

Repo precisou ser tornado **público** pra isso funcionar: GitHub Releases
de repo privado exige autenticação, que o updater (requisição HTTP anônima)
não tem como fazer sem embutir um token no binário (inseguro — qualquer um
extrai do executável). Histórico de commits foi auditado antes da troca
(buscado por padrões de chave de todos os providers suportados) — nada
sensível foi exposto.

Chave de assinatura (par minisign, gerada uma vez com
`tauri signer generate`): a privada fica **fora do repo**, em
`C:\Users\rafae\.tauri-keys\whisper_app_updater.key` (sem senha) — se
perdida, updates futuros não podem mais ser assinados e o app precisa ser
reinstalado manualmente numa versão nova com uma chave nova. A pública vai
no `tauri.conf.json` (`plugins.updater.pubkey`), pode ficar no repo sem
problema.

Cada release agora precisa, além do `.msi`/`.exe` de sempre:
1. `.\scripts\dev.ps1 build` — já seta `TAURI_SIGNING_PRIVATE_KEY` a partir
   da chave acima antes de buildar (`createUpdaterArtifacts: true` no
   `tauri.conf.json` faz o bundler gerar um `.sig` ao lado de cada
   instalador).
2. `.\scripts\make-latest-json.ps1 -Notes "..."` — lê o `.sig` do NSIS
   (`bundle/nsis/*.exe.sig`, é o instalador que o updater roda, não o MSI) e
   gera `latest.json` com versão/assinatura/URL de download.
3. Upload de `.msi` + `.exe` (NSIS) + `.sig` do NSIS + `latest.json` como
   assets da release, com a tag no formato `v<version>` (o `latest.json`
   monta a URL de download assumindo essa convenção).

Restam do roadmap levantado das telas do Amical:
- Mute do áudio do sistema durante a gravação (evita capturar vídeo/música
  tocando no fundo) — menor prioridade.
- Widget sempre visível (não só durante o pipeline) — menor prioridade.
- Descartado por ora: telemetria, update channel, machine ID (infra de
  produto SaaS, não cabe num app pessoal), "self correction" via IA
  (complexo, YAGNI), multi-idioma de UI (app é uso pessoal em PT-BR).

Ver memory files em `C:\Users\rafae\.claude\projects\C--DADOS-WEBAPPS-whisper-app\memory\`
para preferências e histórico específicos do usuário.
