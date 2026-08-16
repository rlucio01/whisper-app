//! Formatação (+ tradução opcional) do texto transcrito via API de LLM.
//!
//! Suporta dois providers, escolhidos pelo `provider` do config:
//!   - **OpenAI** (`gpt-4o-mini` por padrão) — Chat Completions API
//!   - **Anthropic** (`claude-haiku-4-5` por padrão) — Messages API
//!
//! ## Comportamento
//!
//! - Se a chave do provider ativo estiver **vazia**, este serviço vira
//!   passthrough: emite `format-complete` com o texto original, sem chamar
//!   API alguma. O MVP funciona sem chave — a transcrição bruta ainda é útil.
//! - Com chave, envia o texto pro LLM com um prompt system que instrui:
//!   reformatar (remover hesitações, corrigir pontuação, manter tom informal)
//!   e opcionalmente traduzir para outro idioma.
//!
//! ## Thread dedicada
//!
//! Mesmo padrão de [`crate::audio`] e [`crate::transcription`]. A thread
//! escuta em um `mpsc::channel` — cada mensagem é o texto bruto para formatar.
//! A chamada HTTP é bloqueante (via `reqwest::blocking`), então cada request
//! bloqueia essa thread — não bloqueia o UI nem o hotkey.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::active_app::{ActiveApp, AppCategory, SharedActiveApp};
use crate::config::{AppConfig, LlmProvider, SharedConfig};
use crate::insert;
use crate::visual;

const OPENAI_ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";
const ANTHROPIC_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";

/// Limite de tokens da resposta. Ditado curto raramente passa de algumas
/// centenas — 2048 dá folga sem correr risco de truncar.
const MAX_TOKENS: u32 = 2048;

/// Timeout total da chamada HTTP (conexão + resposta). Se o LLM demorar
/// mais que isso, aborta e o usuário vê "format-error".
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct LlmService {
    cmd_tx: mpsc::Sender<String>,
}

impl LlmService {
    pub fn spawn<R: Runtime>(app: AppHandle<R>, config: SharedConfig) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<String>();

        // Um único client reutilizado entre todas as chamadas. Isso mantém
        // o pool de conexões (com keep-alive TLS) quente — economiza o
        // handshake em cada request. Diferença é significativa: ~200-400ms
        // por chamada em vez de estabelecer TLS do zero toda vez.
        let client = reqwest::blocking::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .pool_max_idle_per_host(4)
            .build()
            .expect("falha ao criar HTTP client do LLM");

        thread::spawn(move || llm_thread_loop(cmd_rx, app, config, client));
        Self { cmd_tx }
    }

    /// Enfileira uma formatação. Não bloqueia.
    pub fn format(&self, raw_text: String) {
        let _ = self.cmd_tx.send(raw_text);
    }
}

fn llm_thread_loop<R: Runtime>(
    cmd_rx: mpsc::Receiver<String>,
    app: AppHandle<R>,
    config: SharedConfig,
    client: reqwest::blocking::Client,
) {
    while let Ok(raw_text) = cmd_rx.recv() {
        // Snapshot do config no momento da chamada. Se a UI de settings
        // mudar algo enquanto uma request está em curso, a próxima usa
        // a nova config — não afeta a atual.
        let cfg = match config.lock() {
            Ok(g) => g.clone(),
            Err(_) => {
                let _ = app.emit("format-error", "config mutex envenenado".to_string());
                continue;
            }
        };

        // Se o usuário tem "adaptar ao app ativo" ligado, pegamos o app que
        // estava em foco no F9 (capturado por `hotkey.rs`) para enriquecer
        // o prompt. Se estiver desligado ou sem info, passa `None`.
        let active_app = if cfg.adapt_prompt_to_active_app {
            app.state::<SharedActiveApp>()
                .lock()
                .ok()
                .and_then(|g| g.clone())
        } else {
            None
        };

        // Determina o texto final: LLM formatado, ou passthrough se sem chave.
        let final_text = if cfg.active_api_key().trim().is_empty() {
            raw_text
        } else {
            let _ = app.emit("formatting-started", ());
            let result = match cfg.provider {
                LlmProvider::Openai => {
                    call_openai(&client, &raw_text, &cfg, active_app.as_ref())
                }
                LlmProvider::Anthropic => {
                    call_anthropic(&client, &raw_text, &cfg, active_app.as_ref())
                }
            };
            match result {
                Ok(t) => t,
                Err(e) => {
                    let _ = app.emit("format-error", format!("{:#}", e));
                    visual::set(&app, visual::State::Idle);
                    continue;
                }
            }
        };

        // Sempre emite o texto pronto — a UI mostra antes da colagem.
        let _ = app.emit("format-complete", final_text.clone());

        // Cola no app ativo. Falha aqui não bloqueia — o usuário ainda vê o
        // texto na UI e pode copiar manualmente se precisar.
        match insert::paste_text(&final_text) {
            Ok(()) => {
                let _ = app.emit("text-inserted", ());
            }
            Err(e) => {
                let _ = app.emit("insert-error", format!("{:#}", e));
            }
        }

        // Pipeline concluído — esconde overlay e restaura tray.
        visual::set(&app, visual::State::Idle);
    }
}

// ---------- OpenAI ----------

fn call_openai(
    client: &reqwest::blocking::Client,
    raw_text: &str,
    cfg: &AppConfig,
    active_app: Option<&ActiveApp>,
) -> Result<String> {
    let system_prompt = build_system_prompt(cfg, active_app);

    let body = OpenaiRequest {
        model: cfg.active_model(),
        max_tokens: MAX_TOKENS,
        messages: vec![
            OpenaiMessage {
                role: "system",
                content: &system_prompt,
            },
            OpenaiMessage {
                role: "user",
                content: raw_text,
            },
        ],
    };

    let response = client
        .post(OPENAI_ENDPOINT)
        .bearer_auth(&cfg.openai_api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .context("falha ao enviar request para a API OpenAI")?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "(sem corpo de resposta)".to_string());
        return Err(anyhow!("OpenAI API retornou {}: {}", status, body));
    }

    let parsed: OpenaiResponse = response
        .json()
        .context("resposta da OpenAI não é JSON válido")?;

    let text = parsed
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("resposta da OpenAI veio sem `choices`"))?
        .message
        .content;

    if text.trim().is_empty() {
        return Err(anyhow!("resposta da OpenAI veio sem texto"));
    }

    Ok(text.trim().to_string())
}

// ---------- Anthropic ----------

fn call_anthropic(
    client: &reqwest::blocking::Client,
    raw_text: &str,
    cfg: &AppConfig,
    active_app: Option<&ActiveApp>,
) -> Result<String> {
    let system_prompt = build_system_prompt(cfg, active_app);

    let body = AnthropicRequest {
        model: cfg.active_model(),
        max_tokens: MAX_TOKENS,
        system: &system_prompt,
        messages: vec![AnthropicMessage {
            role: "user",
            content: raw_text,
        }],
    };

    let response = client
        .post(ANTHROPIC_ENDPOINT)
        .header("x-api-key", &cfg.anthropic_api_key)
        .header("anthropic-version", ANTHROPIC_API_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .context("falha ao enviar request para a API Anthropic")?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "(sem corpo de resposta)".to_string());
        return Err(anyhow!("Anthropic API retornou {}: {}", status, body));
    }

    let parsed: AnthropicResponse = response
        .json()
        .context("resposta da Anthropic não é JSON válido")?;

    // A API retorna um array de "blocks" — normalmente só um bloco de texto
    // para nosso caso (sem tool use). Concatenamos os text blocks.
    let text: String = parsed
        .content
        .into_iter()
        .filter_map(|block| match block {
            AnthropicBlock::Text { text } => Some(text),
        })
        .collect::<Vec<_>>()
        .join("");

    if text.trim().is_empty() {
        return Err(anyhow!("resposta da Anthropic veio sem texto"));
    }

    Ok(text.trim().to_string())
}

// ---------- Prompt ----------

/// Monta o system prompt combinando reformatação + hint contextual do app +
/// tradução opcional numa única chamada (evita 2 round-trips). O prompt é o
/// mesmo para os dois providers — tanto GPT quanto Claude seguem instruções
/// em português bem.
fn build_system_prompt(cfg: &AppConfig, active_app: Option<&ActiveApp>) -> String {
    let mut rules = String::from(
        "Você reformata texto ditado por voz para deixá-lo natural, bem \
         pontuado e sem hesitações.\n\n\
         Regras:\n\
         - Corrija pontuação, capitalização e concordância gramatical.\n\
         - Remova hesitações e muletas: \"ah\", \"uhm\", \"tipo\", \"então\" \
         quando não fizer sentido, palavras repetidas por engano.\n\
         - Mantenha o significado original e o tom informal do usuário — não \
         formalize demais nem parafraseie sem necessidade.\n\
         - Não adicione informação que não estava no áudio nem interprete \
         além do que foi dito.\n\
         - Responda APENAS com o texto reformatado, sem preâmbulo, sem \
         explicação, sem aspas ao redor.",
    );

    if let Some(app) = active_app {
        if let Some(hint) = context_hint_for(app) {
            rules.push_str("\n\n");
            rules.push_str(&hint);
        }
    }

    if cfg.translate.enabled {
        rules.push_str(&format!(
            "\n\n- Traduza o texto reformatado para o idioma cujo código ISO é \
             \"{}\". Mantenha as demais regras acima aplicadas ao texto \
             traduzido.",
            cfg.translate.target_language
        ));
    }

    rules
}

/// Retorna um trecho adicional de prompt baseado na categoria do app em foco.
/// `None` = usar apenas as regras genéricas (não adiciona nada).
fn context_hint_for(app: &ActiveApp) -> Option<String> {
    // Se não temos nome de exe, não temos como classificar.
    if app.exe_name.is_empty() {
        return None;
    }

    // Cada categoria vira uma frase curta que orienta o tom sem sobrepor as
    // regras genéricas. Título da janela vai como contexto extra (o LLM decide
    // se usa — ex: se o título contém "Gmail" num navegador, entende email).
    let base = match app.category() {
        AppCategory::Chat => {
            "Contexto: o texto vai ser colado num app de chat/mensageria — \
             use tom informal e conversacional, mantenha frases curtas."
        }
        AppCategory::Email => {
            "Contexto: o texto vai ser colado num cliente de email — use tom \
             um pouco mais estruturado e formal do que chat, mas ainda natural. \
             Se o ditado começar como resposta, mantenha isso."
        }
        AppCategory::Code => {
            "Contexto: o texto vai ser colado num editor de código ou IDE — \
             preserve nomes técnicos, variáveis, funções, sintaxe e trechos \
             de código exatamente como ditados. Se o usuário ditar código, \
             não parafraseie."
        }
        AppCategory::Document => {
            "Contexto: o texto vai ser colado num editor de documentos — o \
             usuário provavelmente está redigindo prosa mais longa e estruturada. \
             Pode usar frases mais elaboradas, mas sem inventar conteúdo."
        }
        AppCategory::Terminal => {
            "Contexto: o texto vai ser colado num terminal/shell — se o ditado \
             for claramente um comando, mantenha a sintaxe exata (flags, paths, \
             pipes) sem \"reformatar\" gramaticalmente. Não adicione pontuação \
             que quebraria o comando."
        }
        AppCategory::Browser => {
            "Contexto: o texto vai ser colado numa página web — use tom neutro \
             e adaptável, sem assumir se é chat, email ou formulário."
        }
        AppCategory::Other => return None,
    };

    // Anexa o título da janela como pista adicional (ex: "Gmail — Google Chrome").
    let mut hint = String::from(base);
    let title = app.window_title.trim();
    if !title.is_empty() {
        hint.push_str(&format!(
            "\nApp em foco: {} — janela: \"{}\".",
            app.exe_name, title
        ));
    } else {
        hint.push_str(&format!("\nApp em foco: {}.", app.exe_name));
    }

    Some(hint)
}

// ---------- Tipos do JSON: OpenAI Chat Completions ----------

#[derive(Serialize)]
struct OpenaiRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<OpenaiMessage<'a>>,
}

#[derive(Serialize)]
struct OpenaiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenaiResponse {
    choices: Vec<OpenaiChoice>,
}

#[derive(Deserialize)]
struct OpenaiChoice {
    message: OpenaiResponseMessage,
}

#[derive(Deserialize)]
struct OpenaiResponseMessage {
    content: String,
}

// ---------- Tipos do JSON: Anthropic Messages ----------

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicBlock {
    #[serde(rename = "text")]
    Text { text: String },
}
