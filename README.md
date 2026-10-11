<p align="center">
  <img src="src-tauri/icons/icon.png" width="120" alt="Whisper App logo" />
</p>

<h1 align="center">Whisper App</h1>

<p align="center">
  Ditado por voz com IA para Windows. Segure um atalho, fale e solte: o texto
  sai reformatado e já colado no aplicativo em que você está.
</p>

<p align="center">
  <a href="https://github.com/rlucio01/whisper-app/releases/latest">
    <img src="https://img.shields.io/github/v/release/rlucio01/whisper-app?label=vers%C3%A3o" alt="Última versão" />
  </a>
  <a href="https://github.com/microsoft/winget-pkgs/pull/449091">
    <img src="https://img.shields.io/badge/winget-rlucio01.WhisperApp-0078D6?logo=windows" alt="Disponível no Winget" />
  </a>
  <img src="https://img.shields.io/badge/plataforma-Windows-0078D6" alt="Plataforma: Windows" />
  <img src="https://img.shields.io/badge/tamanho%20em%20idle-%3C7MB-brightgreen" alt="Menos de 7MB em idle" />
</p>

---

## O que é

Você segura um atalho global (ex: `Ctrl+Windows`), fala, e solta. O áudio é
transcrito, passa por um LLM que corrige pontuação ou hesitações e adapta o
tom ao aplicativo onde você vai colar (chat casual, e-mail formal, editor de
código...), e o resultado é colado automaticamente onde seu cursor estava,
sem precisar copiar nada manualmente.

Todo o pipeline roda em segundo plano com consumo mínimo de recursos: o
processo fica em **menos de 7MB em idle** no gerenciador de tarefas,
bem abaixo de outros aplicativos de ditado que costumam ocupar centenas de
megabytes parados na bandeja do sistema.

## Principais recursos

- **Reuniões Inteligentes & Atas com Diarização por Orador**: grave reuniões ao
  vivo pelo microfone ou importe arquivos gravados de reuniões externas (Zoom,
  Microsoft Teams, Google Meet). A IA transcreve, separa as falas por orador com
  timestamps precisos, sintetiza um resumo executivo, registra decisões principais
  e monta um plano de ação interativo com tarefas, responsáveis, prazos e caixas de
  seleção que persistem o status de conclusão no disco.
- **Transcrição Direta de Arquivos de Áudio**: transcreva gravações de áudio locais
  nos formatos **OGG, WAV, MP3, FLAC, M4A e AAC** sem codecs ou compiladores C
  adicionais (decodificação pura em Rust via Symphonia). Visualize em texto completo
  ou em segmentos temporais com timestamps, copie, exporte para `.txt` ou gere uma
  ata de reunião a partir do áudio com 1 clique.
- **Atalho Rápido de Tradução Automática (F10)**: tecla de atalho global e local
  (padrão `F10`) para ligar ou desligar instantaneamente a tradução para inglês (ou
  o idioma configurado), com feedback sonoro de confirmação, sincronização com a
  bandeja do sistema e botão com indicador no cabeçalho.
- **Atalho Secundário de Push-to-Talk (Mouse ou Teclado)**: suporte a botões extras
  do mouse (`Mouse 4 / Lateral Voltar`, `Mouse 5 / Lateral Avançar`, `Botão do Meio / Scroll`)
  via hook Win32 de baixo nível (`WH_MOUSE_LL`) ou combinação alternativa de teclado,
  operando em paralelo ao atalho principal.
- **Área de Teste de Digitação**: bloco interativo na tela inicial com botão
  dedicado de gravação, suporte a Push-to-Talk dentro do campo e contadores em
  tempo real de palavras, caracteres e estimativa de tokens (~BPE).
- **Dicionário Pessoal & Frequência de Palavras**: adicione vocabulário próprio,
  crie regras de substituição automática ("De -> Para") e acompanhe a lista das
  palavras mais frequentes ditadas, com inclusão no dicionário em um clique.
- **Salvamento Automático**: configurações salvas em tempo real com indicador
  sutil de status no cabeçalho, dispensando cliques manuais de confirmação.
- **Transcrição local** (whisper.cpp, 100% offline) ou **na nuvem**
  (OpenAI Whisper API ou Groq: alta velocidade em hardware dedicado).
- **Detecção de GPU & Aceleração de Hardware**: detecção nativa de placas de
  vídeo (NVIDIA, AMD, Intel Arc) no Windows via DXGI, seleção de dispositivo
  (Automático, GPU ou CPU) e ferramenta integrada de teste de desempenho local.
- **Monitoramento de Consumo & Limites de API**: painel em tempo real para
  acompanhar requisições por minuto (RPM), requisições por dia (RPD), segundos de
  áudio por hora/dia e tokens consumidos em APIs como Groq e OpenAI com alertas de proximidade do limite.
- **Reformatação por LLM** com 6 provedores suportados: OpenAI, Anthropic,
  OpenRouter, Groq, Google Gemini e xAI (Grok). Escolha o provedor preferido e
  insira sua própria chave de API.
- **Adaptação por contexto**: detecta a janela em foco (Slack, Outlook, VS
  Code etc.) e ajusta o tom da reformatação de acordo.
- **Tradução automática** configurável com ativação rápida sincronizada pelo
  menu de contexto da bandeja.
- Indicador visual **flutuante** com onda de áudio ao vivo e controles no
  hover (cancelar ou concluir agora), indicador na **bandeja do sistema**,
  ou ambos.
- **Atalhos configuráveis**: push-to-talk principal e secundário, modo hands-free (toca uma vez
  para começar, de novo para parar), atalho de alternância de tradução e atalho para recolar a última
  transcrição sem regravar.
- **Histórico de ditados** pesquisável, com contagem de palavras e opções de
  copiar, recolar ou apagar por entrada.
- **Seleção de microfone**, inicialização automática com o Windows, iniciar direto na
  bandeja e feedback sonoro: opções totalmente configuráveis.
- **Atualização automática**: aviso e instalação com um clique de novas versões
  assinadas digitalmente, sem necessidade de download manual.
- **Visual Limpo e Profissional**: interface moderna com ícones SVG vetorizados,
  sem emojis informais, sem crases e sem travessões.

Stack: **Tauri 2 + React 19 + TypeScript + Rust**.

## Instalação (uso normal, sem compilar)

### Via Windows Package Manager (Winget)

A forma mais rápida para quem já tem o Winget (Windows 10/11):

```powershell
winget install rlucio01.WhisperApp
```

Para atualizar para a versão mais recente:

```powershell
winget upgrade rlucio01.WhisperApp
```

### Via instalador manual

1. Baixe o instalador mais recente na
   [página de Releases](https://github.com/rlucio01/whisper-app/releases/latest)
   (`.msi` ou `.exe`, ambos instalam o aplicativo).
2. Execute o instalador e abra o Whisper App.
3. Na primeira execução, abra **Configurações** (ícone de engrenagem) e escolha um
   provedor de LLM inserindo sua chave de API (veja a seção
   [Configuração](#configuração) abaixo).

Após isso, novas versões são notificadas e atualizadas diretamente dentro do aplicativo.

## Fluxos de Execução

### 1. Ditado Instantâneo (Push-to-Talk / Hands-Free)

```
hotkey (teclado ou mouse) pressionado
    → grava áudio (cpal)
        → transcreve (whisper.cpp local OU OpenAI/Groq na nuvem)
            → aplica dicionário e regras de substituição
                → reformata via LLM (com adaptação de tom pelo app em foco)
                    → cola no app em foco (clipboard + Ctrl+V)
```

### 2. Transcrição de Arquivo de Áudio

```
seleção de arquivo (.mp3, .ogg, .wav, .m4a, .flac, .aac)
    → decodificação nativa para mono 16 kHz (symphonia)
        → transcrição em lote com timestamps (whisper local ou cloud)
            → visualização com timestamps + exportação (.txt ou envio para atas)
```

### 3. Reuniões Inteligentes & Atas com Diarização

```
gravação ao vivo de reunião (cpal) OU importação de gravação externa
    → transcrição segmentada com timestamps
        → diarização por orador e análise com LLM
            → síntese de resumo executivo + deliberações
                → geração de plano de ação interativo com prazos por participante
                    → persistência de ata em JSONL e exportação (.md)
```

## Configuração

Na primeira utilização, acesse Configurações para definir:

1. O **provedor de LLM** (OpenAI, Anthropic, OpenRouter, Groq,
   Gemini ou xAI) e a respectiva chave de API.
2. O modo de **transcrição**: Local (offline), OpenAI (cloud) ou
   Groq (cloud, mais rápido).
3. Se optar pelo modo local: faça o download do modelo desejado (o modelo Small é o
   padrão recomendado para uso diário em CPU ou GPU).
4. O **atalho global principal** padrão é `Ctrl+Windows`. É possível também configurar um **atalho secundário** (com suporte aos botões laterais `Mouse 4` e `Mouse 5` ou botão do meio) e um **atalho para alternar tradução** (padrão `F10`).
5. O aplicativo vem configurado para **iniciar com o Windows** por padrão (opção ajustável nas configurações).
6. (Opcional) Dicionário pessoal, regras de substituição "De -> Para", modo hands-free, microfone específico, etc.

Arquivo de configuração: `%APPDATA%\com.rlucio.whisperapp\config.json`. Nenhuma
chave de API fica gravada no código: tudo permanece salvo apenas no seu arquivo local.

## Modelos do Whisper (modo local)

Os modelos podem ser baixados na interface (Configurações → Transcrição → Local) e ficam salvos em
`%APPDATA%\com.rlucio.whisperapp\models\`.

| Modelo         | Arquivo                            | Tamanho | Uso                           |
|----------------|-------------------------------------|---------|-------------------------------|
| Tiny           | `ggml-tiny-q5_1.bin`               | ~31 MB  | Mais rápido, menor precisão   |
| Base           | `ggml-base-q5_1.bin`               | ~59 MB  | Bom para testes rápidos       |
| Small (padrão) | `ggml-small-q5_1.bin`              | ~181 MB | Recomendado para o dia a dia  |
| Medium         | `ggml-medium-q5_0.bin`             | ~514 MB | Mais preciso, maior exigência |
| Large-v3 Turbo | `ggml-large-v3-turbo-q5_0.bin`     | ~574 MB | Máxima precisão               |

## Rodando a partir do código-fonte

Necessário apenas se você desejar modificar o código. Para uso regular, consulte
[Instalação](#instalação-uso-normal-sem-compilar).

### Requisitos (Windows)

- **Node.js 18+** e **Rust** (via `rustup`).
- **Visual Studio 2019/2022 Build Tools** com a carga de trabalho "Desktop development with C++".
- **LLVM/Clang** (para compilação do whisper.cpp): `winget install LLVM.LLVM`.
- **CMake** e **Ninja**: `winget install Kitware.CMake Ninja-build.Ninja`.

Após a instalação, certifique-se de que `C:\Program Files\LLVM\bin` esteja no PATH ou utilize o
script `scripts/dev.ps1`, que configura o ambiente automaticamente.

### Modo de desenvolvimento

```powershell
.\scripts\dev.ps1
```

O script prepara o ambiente MSVC + LLVM + Ninja antes de executar `npm run tauri dev`.

### Build de release

```powershell
.\scripts\dev.ps1 build
```

Gera os instaladores `.msi` e `.exe` assinados em `src-tauri/target/release/bundle/`.

## Estrutura do projeto

```
src/                   - UI React:
  ├── App              - tela principal com navegação e área de teste de digitação
  ├── AudioFileTranscribe - transcrição de arquivos de áudio (MP3, OGG, WAV, etc.)
  ├── MeetingsView     - gravação de reuniões, diarização por orador e plano de ação
  ├── Settings         - configurações com auto-save, provedores, modelos e atalhos
  ├── History          - histórico pesquisável de ditados com contagem de palavras
  └── Overlay          - indicador flutuante com forma de onda de áudio ao vivo
src-tauri/src/         - Lógica de backend em Rust:
  ├── active_app       - detecção do aplicativo em foco (Win32)
  ├── audio            - captura via cpal, processamento de WAV em memória
  ├── audio_decode     - decodificação pura em Rust de MP3, OGG, WAV, FLAC, M4A/AAC
  ├── commands         - comandos Tauri invocáveis pelo frontend
  ├── config           - configurações persistentes em JSON e auto-save
  ├── dictionary       - vocabulário pessoal, regras de substituição e frequência
  ├── gpu_runtime      - gerenciamento de execução em GPU e CPU
  ├── hardware         - detecção de GPUs instaladas via DXGI
  ├── history          - histórico de ditados em disco (JSON Lines)
  ├── hotkey           - atalhos globais (push-to-talk, hands-free, recolar, tradução)
  ├── insert           - colagem de texto via clipboard e simulação de teclas
  ├── lib              - ponto de entrada, serviços e menu na bandeja do sistema
  ├── llm              - integração com provedores de IA, formatação e atas
  ├── meeting          - serviço de reunião ao vivo (WASAPI thread-safe), diarização e atas
  ├── models           - download e gerenciamento dos modelos do Whisper
  ├── modkey           - hook Win32 de baixo nível para modificadores e botões do mouse
  ├── sound            - efeitos sonoros de início, término e alternância de tradução
  ├── system_audio     - gerenciamento e pausa do áudio do sistema
  ├── transcription    - whisper.cpp local e integração cloud (OpenAI/Groq)
  └── visual           - overlay flutuante, indicador na bandeja e badge de update
scripts/dev.ps1        - script para configuração do ambiente e execução
scripts/make-latest-json.ps1 - geração do manifesto para atualização automática
CLAUDE.md              - notas de arquitetura e diretrizes para desenvolvimento
```

## Compatibilidade

O desenvolvimento inicial é focado no Windows. O código em Rust foi estruturado com `#[cfg(target_os = "...")]`
onde há particularidades de plataforma, permitindo expansões futuras para outros sistemas operacionais.

## Licença

Repositório para uso pessoal e transparência. Fique à vontade para explorar o código; não licenciado para redistribuição ou exploração comercial.
