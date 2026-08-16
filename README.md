# Whisper App

App desktop de ditado por voz com IA para Windows. Pressione um atalho global,
fale, solte — o texto reformatado é colado automaticamente no app em foco.

- Transcrição **local** (whisper.cpp, offline) ou **cloud** (OpenAI Whisper API).
- Reformatação por LLM (OpenAI ou Anthropic) com adaptação por contexto do app
  em foco (chat casual vs email formal vs IDE, etc.).
- Tradução automática opcional.
- Indicador visual flutuante (always-on-top) + tray colorido enquanto grava.
- Atalho global configurável com combinações (`F9`, `Ctrl+Shift+Space`, etc.).
- Autostart com o sistema (opcional).

Stack: **Tauri 2 + React 19 + TypeScript + Rust**.

## Fluxo

```
hotkey pressionado
    → grava áudio (cpal)
        → transcreve (whisper.cpp local OU OpenAI Whisper API)
            → reformata via LLM (OpenAI ou Anthropic)
                → cola no app em foco (clipboard + Ctrl+V)
```

## Requisitos (Windows)

- **Node.js 18+** e **Rust** (via `rustup`).
- **Visual Studio 2019/2022 Build Tools** com "Desktop development with C++".
- **LLVM/Clang** (para compilar o whisper.cpp) — `winget install LLVM.LLVM`.
- **CMake** e **Ninja** — `winget install Kitware.CMake Ninja-build.Ninja`.

Após instalar tudo, adicione `C:\Program Files\LLVM\bin` ao PATH ou deixe o
script `scripts/dev.ps1` configurar sozinho.

## Rodar em modo dev

```powershell
.\scripts\dev.ps1
```

O script configura o ambiente MSVC + LLVM + Ninja + `CMAKE_GENERATOR=Ninja`
antes de rodar `npm run tauri dev`. Rodar `npm run tauri dev` direto num
PowerShell "normal" falha porque o `whisper-rs` precisa dessas variáveis.

## Build release

```powershell
.\scripts\dev.ps1 build
```

Gera o `.msi` e `.exe` em `src-tauri/target/release/bundle/`.

## Configuração

O primeiro uso pede que você abra Configurações e:

1. Escolha o **provedor de LLM** (OpenAI ou Anthropic) e cole a chave.
2. Escolha o modo de **transcrição** — Local (offline) ou OpenAI (cloud).
3. Se local: baixe pelo menos um modelo do Whisper (Small é o default e cobre
   uso diário em CPU comum).
4. Ajuste o **atalho global** clicando em "Alterar" (default: `F9`).
5. (Opcional) Ative "Iniciar automaticamente com o Windows".

Arquivo de config: `%APPDATA%\com.rlucio.whisperapp\config.json`.

## Modelos do Whisper

Baixe pela UI (Configurações → Transcrição → Local). Ficam em
`%APPDATA%\com.rlucio.whisperapp\models\`.

| Modelo         | Arquivo                            | Tamanho | Uso                           |
|----------------|------------------------------------|---------|-------------------------------|
| Tiny           | `ggml-tiny-q5_1.bin`               | ~31 MB  | Mais rápido, menos preciso    |
| Base           | `ggml-base-q5_1.bin`               | ~59 MB  | Bom para testes               |
| Small (default)| `ggml-small-q5_1.bin`              | ~181 MB | Recomendado para uso diário   |
| Medium         | `ggml-medium-q5_1.bin`             | ~514 MB | Mais preciso, mais lento      |
| Large-v3 Turbo | `ggml-large-v3-turbo-q5_0.bin`     | ~574 MB | Máxima precisão               |

## Estrutura do projeto

```
src/                   — UI React (App, Settings, Overlay)
src-tauri/src/         — Lógica Rust:
  ├── active_app       — detecção do app em foco (Win32)
  ├── audio            — captura via cpal, escreve WAV em memória
  ├── commands         — comandos Tauri chamáveis do frontend
  ├── config           — settings JSON persistentes
  ├── hotkey           — atalho global (tauri-plugin-global-shortcut)
  ├── insert           — clipboard + Ctrl+V via enigo + arboard
  ├── llm              — chamadas OpenAI/Anthropic com prompt contextual
  ├── models           — download e gerenciamento dos modelos Whisper
  ├── transcription    — whisper.cpp local + OpenAI Whisper API cloud
  └── visual           — overlay flutuante + tray colorido
scripts/dev.ps1        — wrapper que configura ambiente e roda Tauri
CLAUDE.md              — notas de arquitetura para agentes de IA
```

## Cross-platform

MVP focado em Windows. O código Rust é escrito com `#[cfg(target_os = "...")]`
onde faz diferença — macOS/Linux vão precisar de implementações próprias para
o módulo `active_app`, permissões nativas e ajustes no bundler. Ver `CLAUDE.md`
para o mapa por SO.

## Licença

Uso pessoal por ora — sem licença aberta definida.
