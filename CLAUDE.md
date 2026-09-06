# Whisper App: notas de arquitetura para agentes de IA

App desktop de ditado por voz com IA (Windows-first, com portabilidade prevista
para macOS/Linux). Stack: Tauri 2 + React 19 + TypeScript + Rust.

## Arquitetura em camadas

```
React (src/)                  - UI: App (status, scratchpad), Settings, History, Overlay
    ↕ invoke() / events
Rust (src-tauri/src/)         - lógica de sistema:
  ├── active_app               detecção do app em foco (Win32) para hint do LLM
  ├── audio                    captura via cpal -> WAV em memória (thread própria)
  ├── commands                 comandos Tauri chamáveis do frontend
  ├── config                   settings persistentes em JSON e auto-save
  ├── dictionary               dicionário pessoal, regras de substituição e frequência
  ├── gpu_runtime              gerenciamento e teste de IA local (GPU/CPU)
  ├── hardware                 detecção de adaptadores de vídeo via DXGI
  ├── history                  histórico de ditados (JSON Lines em disco)
  ├── hotkey                   atalhos globais configuráveis (PTT, hands-free, recolar)
  ├── insert                   clipboard + Ctrl+V via enigo + arboard
  ├── lib                      inicialização, serviços, tray e ciclo de vida
  ├── llm                      OpenAI/Anthropic/OpenRouter/Groq/Gemini/xAI para reformatar e traduzir
  ├── models                   download e gerenciamento dos modelos Whisper
  ├── modkey                   monitoramento de teclas modificadoras globais
  ├── sound                    feedback sonoro de início/fim de gravação
  ├── system_audio             captura/mute de áudio do sistema
  ├── transcription            whisper.cpp local OU cloud APIs (OpenAI/Groq)
  └── visual                   overlay flutuante + tray e badge de update
```

Fluxo do usuário:
`hotkey pressionado -> grava áudio -> solta -> transcreve (local ou cloud) -> aplica dicionário/substituições -> LLM reformata (com hint do app em foco) -> cola no app ativo`.

## Decisões tomadas

- **Tauri 2.x** (não 1.x): APIs de plugins e permissões maduras.
- **`tauri-plugin-single-instance`** registrado como o primeiro plugin do
  builder: sem ele, cada clique no ícone (ou o autostart disparando com o
  app já de pé) sobe um processo Tauri novo: hotkey registrado duas vezes,
  dois ícones na bandeja, e cada processo com seu próprio state em memória
  (fonte de bugs sutis tipo a UI mostrar um hotkey desatualizado se o
  processo errado responde ao `get_config`). O callback do plugin só foca a
  janela da instância existente.
- **Hook de panic gravando em `crash.log`** (`install_panic_hook` em
  `lib.rs`, instalado antes de qualquer outra coisa em `run()`): o profile
  de release usa `panic = "abort"`, então um panic em qualquer thread,
  inclusive dentro do `setup()`, como os `.expect()` de criação do client
  HTTP em `llm.rs`/`transcription.rs`, mata o processo inteiro sem deixar
  rastro nenhum (nem no Event Viewer). Isso explica falhas tipo "o app não
  abriu depois que o Windows iniciou" sem nenhum log para investigar.
- **whisper-rs 0.16** (não 0.14, que tinha bug no tamanho de `whisper_full_params`).
- **cpal em thread dedicada** com `mpsc::channel`: `cpal::Stream` é `!Send`
  no Windows (WASAPI usa thread-local state), então guarda-lo num state
  compartilhado não funciona. Cada serviço (audio, transcription, llm) tem
  seu próprio pattern de thread + canal.
- **reqwest blocking** com `rustls-tls` (sem OpenSSL nativo, sem tokio).
  Cliente HTTP é **reutilizado** por serviço (mantém pool com keep-alive TLS
  quente: economia significativa de latência por chamada).
- **Inserção via clipboard + Ctrl+V** (`enigo` + `arboard`). Salva e restaura
  clipboard. Funciona em ~95% dos apps cross-platform.
- **Transcrição cloud usa mono 16kHz**: antes de enviar para OpenAI Whisper
  API, downsample local via `hound`: corta o payload em ~5-10x vs 48kHz
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
  background de roubar foco) logo antes do Ctrl+V: protege contra qualquer
  coisa que roube o foco durante os segundos de transcrição/formatação,
  incluindo cliques nos controles do próprio overlay.
- **Toda janela nova precisa entrar em `capabilities/*.json`**: o Tauri 2
  nega permissões (inclusive `event.listen`) por padrão para qualquer janela
  fora do array `windows` de uma capability: a falha é silenciosa (promise
  rejeitada sem crash) e não aparece em `cargo check`/`tsc`. Se uma nova janela for
  criada em `tauri.conf.json`, adicione o label dela em
  `capabilities/default.json` (ou crie uma capability dedicada).
- **Compilação do whisper.cpp e flags SIMD portáveis (`GGML_AVX=ON`, `GGML_NATIVE=OFF`, `GGML_AVX512=OFF`, `GGML_AVX2=OFF`, `GGML_FMA=OFF`)**:
  Definidas em `scripts/dev.ps1` e `src-tauri/.cargo/config.toml`. **NUNCA remova ou altere essas variáveis sem entender o seguinte contexto**:
  1. A máquina de compilação do desenvolvedor possui um processador com instruções ultra-recentes AVX-512.
  2. Se `GGML_NATIVE=ON` ou se `GGML_AVX512` for compilado, o binário conterá instruções AVX-512, resultando em crash fatal instantâneo (`0xc000001d / STATUS_ILLEGAL_INSTRUCTION` no Event Viewer) na máquina de usuários comuns (sem AVX-512).
  3. Se o AVX for totalmente desativado (`GGML_AVX=OFF`), o `whisper.cpp` cai em emulação matemática escalar em software (sem vetorização por hardware), fazendo uma transcrição demorar minutos em vez de 1 segundo ("congelando" em *Transcrevendo...*).
  4. Manter `GGML_AVX=ON` com `GGML_AVX512=OFF` e `GGML_AVX2=OFF` ativa SIMD 256-bit AVX 1.0 (presente em 100% dos processadores modernos desde 2011), entregando **alta velocidade (~1s)** com **estabilidade universal** em qualquer computador.
- **Dicionário Pessoal e Frequência de Palavras (`dictionary.rs`)**:
  - Armazenamento em JSON de vocabulário customizado (`custom_words`), regras de substituição ("De -> Para") e frequência de termos ditados (`frequency_words.json`).
  - Vocabulário é injetado como `initial_prompt` no Whisper e fornecido ao prompt de sistema do LLM.
  - Substituições textuais diretas são aplicadas sobre a transcrição antes e após o pós-processamento.
  - As palavras mais ditadas são rastreadas e podem ser adicionadas com 1 clique ao dicionário.
- **Área de Teste de Digitação na Tela Inicial (`App.tsx`)**:
  - Permite validar transcrições diretamente na UI principal.
  - Escuta local de teclado via `useRef` para acionar Push-to-Talk mesmo quando o campo de texto está focado (evitando bloqueios de eventos do WebView2).
  - Contador dinâmico de palavras, caracteres e estimativa de tokens (~BPE: ~3.8 caracteres por token para português/inglês).
  - Prevenção de duplicação: a inserção via clipboard do sistema respeita o foco nativo do campo.
- **Salvamento Automático (Auto-Save)**:
  - Settings.tsx salva mutações com debounce de 280ms, exibindo status discreto ("Salvando...", "Salvo").
- **Sincronização do System Tray**:
  - Ao alternar a tradução automática no menu de contexto, o estado de `skip_llm_formatting` é desativado para garantir que o texto seja traduzido pela LLM.
- **Diretrizes Tipográficas e Estilísticas**:
  - Sem crases e sem travessões na UI e nos prompts do sistema.
  - Sanitização automática em `llm.rs` para remover travessões gerados por modelos de linguagem.
  - Design visual sóbrio, com ícones SVG vetorizados e sem emojis informais.

## Config persistente

`Arc<Mutex<AppConfig>>` gerenciado como state Tauri. Todos os campos com
`#[serde(default)]`: arquivos antigos abrem sem migração. Campos principais:

- `provider`: qual API de LLM usar (`openai`, `anthropic`, `openrouter`, `groq`, `gemini`, `xai`).
- `openai_api_key`, `anthropic_api_key`, `openrouter_api_key`, `groq_api_key`, `gemini_api_key`, `xai_api_key`.
- `llm_model`: modelo do provider ou personalizado.
- `custom_words`: lista de palavras e termos do vocabulário do usuário.
- `replacements`: pares chave-valor para substituição pós-transcrição ("De -> Para").
- `device_selection`: preferência de dispositivo de hardware local (`auto`, `gpu`, `cpu`).
- `translate: { enabled, target_language }`: tradução automática.
- `visual_indicator: none|floating|tray|both`: indicador visual.
- `hotkey`: atalho push-to-talk (formato accelerator, default `"Ctrl+Super"`).
- `hands_free_hotkey`: atalho opcional para modo hands-free.
- `repaste_hotkey`: atalho opcional para recolar o último texto transcrito.
- `transcription_provider: local|openai_cloud|groq_cloud`: backend de transcrição.
- `whisper_model`: modelo local (`tiny`, `base`, `small`, `medium`, `large_turbo`).
- `microphone`: dispositivo de entrada de áudio selecionado.
- `start_minimized`: iniciar direto na bandeja do sistema.
- `mute_system_audio`: mutar áudio do sistema durante a gravação.

## Comandos úteis

Sempre via PowerShell no Windows:

```powershell
.\scripts\dev.ps1              # dev com hot reload
.\scripts\dev.ps1 build        # bundle assinado .msi/.exe
cd src-tauri; cargo check      # validação Rust rápida
npx tsc --noEmit               # validação TypeScript
```

## Modelos do Whisper

Gerenciados via interface ou armazenados em `%APPDATA%\com.rlucio.whisperapp\models\<filename>.bin`.

| Slug | Arquivo | Tamanho | Uso |
|---|---|---|---|
| `tiny` | `ggml-tiny-q5_1.bin` | ~31 MB | Mais rápido, menor precisão |
| `base` | `ggml-base-q5_1.bin` | ~59 MB | Testes |
| `small` | `ggml-small-q5_1.bin` | ~181 MB | Padrão recomendado |
| `medium` | `ggml-medium-q5_0.bin` | ~514 MB | Mais preciso |
| `large_turbo` | `ggml-large-v3-turbo-q5_0.bin` | ~574 MB | Máxima precisão |

## Auto-update e Releases

- Releases no GitHub precisam seguir o padrão de título: `whisper_app v<versão>` (exemplo: `whisper_app v0.4.12`).
- As notas de release são passadas no parâmetro `--notes`.
- Assets obrigatórios: instalador NSIS `.exe`, assinatura `.sig`, instalador MSI `.msi`, assinatura `.msi.sig` e manifesto `latest.json`.
- Geração de manifesto: `.\scripts\make-latest-json.ps1 -Notes "..."`.
