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
- **Atalhos configuráveis**: push-to-talk, modo hands-free (toca uma vez
  para começar, de novo para parar) e atalho para recolar a última
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

1. Baixe o instalador mais recente na
   [página de Releases](https://github.com/rlucio01/whisper-app/releases/latest)
   (`.msi` ou `.exe`, ambos instalam o aplicativo).
2. Execute o instalador e abra o Whisper App.
3. Na primeira execução, abra **Configurações** (ícone de engrenagem) e escolha um
   provedor de LLM inserindo sua chave de API (veja a seção
   [Configuração](#configuração) abaixo).

Após isso, novas versões são notificadas e atualizadas diretamente dentro do aplicativo.

## Fluxo de Execução

```
hotkey pressionado
    → grava áudio (cpal)
        → transcreve (whisper.cpp local OU OpenAI/Groq na nuvem)
            → aplica dicionário e regras de substituição
                → reformata via LLM (provedor à sua escolha)
                    → cola no app em foco (clipboard + Ctrl+V)
```

## Configuração

Na primeira utilização, acesse Configurações para definir:

1. O **provedor de LLM** (OpenAI, Anthropic, OpenRouter, Groq,
   Gemini ou xAI) e a respectiva chave de API.
2. O modo de **transcrição**: Local (offline), OpenAI (cloud) ou
   Groq (cloud, mais rápido).
3. Se optar pelo modo local: faça o download do modelo desejado (o modelo Small é o
   padrão recomendado para uso diário em CPU ou GPU).
4. O **atalho global** padrão é `Ctrl+Windows` (alterável na interface).
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
src/                   - UI React (App, Settings, History, Overlay)
src-tauri/src/         - Lógica de backend em Rust:
  ├── active_app       - detecção do aplicativo em foco (Win32)
  ├── audio            - captura via cpal, processamento de WAV em memória
  ├── commands         - comandos Tauri invocáveis pelo frontend
  ├── config           - configurações persistentes em JSON e auto-save
  ├── dictionary       - vocabulário pessoal, regras de substituição e frequência
  ├── gpu_runtime      - gerenciamento de execução em GPU e CPU
  ├── hardware         - detecção de GPUs instaladas via DXGI
  ├── history          - histórico de ditados em disco (JSON Lines)
  ├── hotkey           - atalhos globais (push-to-talk, hands-free, recolar)
  ├── insert           - colagem de texto via clipboard e simulação de teclas
  ├── lib              - ponto de entrada, serviços e menu na bandeja do sistema
  ├── llm              - integração com provedores de IA e formatação de texto
  ├── models           - download e gerenciamento dos modelos do Whisper
  ├── modkey           - monitoramento de teclas modificadoras do teclado
  ├── sound            - efeitos sonoros de início e término de gravação
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
