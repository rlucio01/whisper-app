<p align="center">
  <img src="src-tauri/icons/icon.png" width="120" alt="Whisper App logo" />
</p>

<h1 align="center">Whisper App</h1>

<p align="center">
  Ditado por voz com IA para Windows. Segure um atalho, fale, solte — o texto
  sai reformatado e já colado no app em que você está.
</p>

<p align="center">
  <a href="https://github.com/rlucio01/whisper-app/releases/latest">
    <img src="https://img.shields.io/github/v/release/rlucio01/whisper-app?label=vers%C3%A3o" alt="Última versão" />
  </a>
  <img src="https://img.shields.io/badge/plataforma-Windows-0078D6" alt="Plataforma: Windows" />
  <img src="https://img.shields.io/badge/tamanho%20em%20idle-%3C7MB-brightgreen" alt="Menos de 7MB em idle" />
</p>

---

## O que é

Você segura um atalho global (ex: `Ctrl+Windows`), fala, e solta. O áudio é
transcrito, passa por um LLM que corrige pontuação/hesitações e adapta o
tom ao app onde você vai colar (chat casual, email formal, editor de
código...), e o resultado é colado automaticamente onde seu cursor estava —
sem precisar copiar nada na mão.

Todo o pipeline roda em background com pegada mínima de recursos: o
processo fica em **menos de 7MB em idle** no gerenciador de tarefas —
bem abaixo de apps concorrentes de ditado, que costumam ficar na casa dos
200MB parados no tray.

## Principais recursos

- **Transcrição local** (whisper.cpp, 100% offline) ou **na nuvem**
  (OpenAI Whisper API ou Groq — mais rápido, roda em hardware dedicado).
- **Reformatação por LLM** com 6 provedores suportados: OpenAI, Anthropic,
  OpenRouter, Groq, Google Gemini e xAI (Grok). Escolha o que preferir e
  cole sua própria chave de API.
- **Adaptação por contexto**: detecta o app em foco (Slack, Outlook, VS
  Code etc.) e ajusta o tom da reformatação de acordo.
- **Tradução automática** opcional pro idioma que você configurar.
- Indicador visual **flutuante** com onda de áudio ao vivo e controles no
  hover (cancelar / concluir agora), indicador na **bandeja do sistema**,
  ou ambos.
- **Atalhos configuráveis**: push-to-talk, modo hands-free (toca uma vez
  pra começar, de novo pra parar) e um atalho pra recolar a última
  transcrição sem regravar.
- **Histórico de ditados** pesquisável, com contagem de palavras e
  copiar/recolar/apagar por entrada.
- **Seleção de microfone**, autostart com o Windows, iniciar direto na
  bandeja, feedback sonoro — tudo opcional, configurável em Configurações.
- **Atualização automática**: o app avisa sozinho quando sai uma versão
  nova e instala com um clique, sem precisar baixar `.exe`/`.msi` manualmente.

Stack: **Tauri 2 + React 19 + TypeScript + Rust**.

## Instalação (uso normal, sem compilar nada)

1. Baixe o instalador mais recente na
   [página de Releases](https://github.com/rlucio01/whisper-app/releases/latest)
   (`.msi` ou `.exe`, qualquer um dos dois instala o app).
2. Rode o instalador e abra o Whisper App.
3. Na primeira execução, abra **Configurações** (ícone ⚙) e escolha um
   provedor de LLM + cole sua chave de API — veja a seção
   [Configuração](#configuração) abaixo.

Depois disso, novas versões aparecem sozinhas dentro do app (veja
"Atualização automática" acima) — não é mais necessário voltar aqui pra
baixar manualmente.

## Fluxo

```
hotkey pressionado
    → grava áudio (cpal)
        → transcreve (whisper.cpp local OU OpenAI/Groq na nuvem)
            → reformata via LLM (provedor à sua escolha)
                → cola no app em foco (clipboard + Ctrl+V)
```

## Configuração

O primeiro uso pede que você abra Configurações e:

1. Escolha o **provedor de LLM** (OpenAI, Anthropic, OpenRouter, Groq,
   Gemini ou xAI) e cole a chave da API correspondente.
2. Escolha o modo de **transcrição** — Local (offline), OpenAI (cloud) ou
   Groq (cloud, mais rápido).
3. Se local: baixe pelo menos um modelo do Whisper (Small é o default e
   cobre uso diário em CPU comum).
4. O **atalho global** padrão é `Ctrl+Windows` (alterável clicando em "Alterar").
5. O app já vem configurado para **iniciar automaticamente com o Windows** por padrão (desativável em Configurações).
6. (Opcional) Ative modo hands-free, atalho de recolar, microfone específico, etc.

Arquivo de config: `%APPDATA%\com.rlucio.whisperapp\config.json`. Nenhuma
chave de API é hardcoded no app — todas ficam só nesse arquivo local.

## Modelos do Whisper (modo local)

Baixe pela UI (Configurações → Transcrição → Local). Ficam em
`%APPDATA%\com.rlucio.whisperapp\models\`.

| Modelo         | Arquivo                            | Tamanho | Uso                           |
|----------------|-------------------------------------|---------|-------------------------------|
| Tiny           | `ggml-tiny-q5_1.bin`               | ~31 MB  | Mais rápido, menos preciso    |
| Base           | `ggml-base-q5_1.bin`               | ~59 MB  | Bom para testes               |
| Small (default)| `ggml-small-q5_1.bin`              | ~181 MB | Recomendado para uso diário   |
| Medium         | `ggml-medium-q5_0.bin`             | ~514 MB | Mais preciso, mais lento      |
| Large-v3 Turbo | `ggml-large-v3-turbo-q5_0.bin`     | ~574 MB | Máxima precisão               |

## Rodando a partir do código-fonte

Só necessário se você quiser modificar o app — pra apenas usá-lo, veja
[Instalação](#instalação-uso-normal-sem-compilar-nada) acima.

### Requisitos (Windows)

- **Node.js 18+** e **Rust** (via `rustup`).
- **Visual Studio 2019/2022 Build Tools** com "Desktop development with C++".
- **LLVM/Clang** (para compilar o whisper.cpp) — `winget install LLVM.LLVM`.
- **CMake** e **Ninja** — `winget install Kitware.CMake Ninja-build.Ninja`.

Após instalar tudo, adicione `C:\Program Files\LLVM\bin` ao PATH ou deixe o
script `scripts/dev.ps1` configurar sozinho.

### Modo dev

```powershell
.\scripts\dev.ps1
```

O script configura o ambiente MSVC + LLVM + Ninja + `CMAKE_GENERATOR=Ninja`
antes de rodar `npm run tauri dev`. Rodar `npm run tauri dev` direto num
PowerShell "normal" falha porque o `whisper-rs` precisa dessas variáveis.

### Build release

```powershell
.\scripts\dev.ps1 build
```

Gera o `.msi` e `.exe` em `src-tauri/target/release/bundle/`.

## Estrutura do projeto

```
src/                   — UI React (App, Settings, History, Overlay)
src-tauri/src/         — Lógica Rust:
  ├── active_app       — detecção do app em foco (Win32)
  ├── audio            — captura via cpal, escreve WAV em memória
  ├── commands         — comandos Tauri chamáveis do frontend
  ├── config           — settings JSON persistentes
  ├── history          — histórico de ditados (JSON Lines em disco)
  ├── hotkey           — atalhos globais (push-to-talk, hands-free, recolar)
  ├── insert           — clipboard + Ctrl+V via enigo + arboard
  ├── llm              — chamadas multi-provedor com prompt contextual
  ├── models           — download e gerenciamento dos modelos Whisper
  ├── modkey           — monitoramento de teclas modificadoras globais
  ├── sound            — feedback sonoro de início/fim de gravação
  ├── system_audio     — captura/mute de áudio do sistema
  ├── transcription    — whisper.cpp local + APIs cloud (OpenAI/Groq)
  └── visual           — overlay flutuante + tray colorido e badge de update
scripts/dev.ps1        — wrapper que configura ambiente e roda Tauri
scripts/make-latest-json.ps1 — gera o manifesto consumido pelo auto-updater
CLAUDE.md              — notas de arquitetura e guia para agentes de IA
```

## Cross-platform

MVP focado em Windows. O código Rust é escrito com `#[cfg(target_os = "...")]`
onde faz diferença — macOS/Linux vão precisar de implementações próprias para
o módulo `active_app`, permissões nativas e ajustes no bundler. Ver `CLAUDE.md`
para o mapa por SO.

## Licença

Repositório público para transparência e uso pessoal — sem licença open
source formal definida. Sinta-se à vontade para explorar o código, mas ele
não está licenciado para redistribuição ou uso comercial.
