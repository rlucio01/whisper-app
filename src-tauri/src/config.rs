//! Config do usuário — carregada de um arquivo JSON no `app_data_dir`.
//!
//! Path por SO:
//!   - Windows: `%APPDATA%\com.rlucio.whisperapp\config.json`
//!   - macOS:   `~/Library/Application Support/com.rlucio.whisperapp/config.json`
//!   - Linux:   `~/.config/com.rlucio.whisperapp/config.json`
//!
//! Design:
//!   - Todos os campos têm `#[serde(default)]` para que um `config.json` legado
//!     (sem um campo novo) continue funcionando após updates. Não quebramos
//!     retrocompatibilidade nem forçamos migrações.
//!   - Suporta dois providers de LLM (OpenAI e Anthropic). O campo `provider`
//!     escolhe qual é usado; as duas chaves ficam em campos separados para o
//!     usuário poder trocar sem precisar reconfigurar.
//!   - A UI de settings (etapa 8) vai chamar `load` / `save` via comandos Tauri.
//!     Por enquanto o usuário edita o arquivo manualmente.
//!   - Envolvido em `Arc<Mutex<AppConfig>>` como state Tauri — assim o LLM
//!     service lê a versão mais recente a cada chamada, sem restart do app.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

/// Modelos padrão para cada provider. Trocar aqui muda o default para novos
/// users; users existentes mantêm o que estiver salvo no `config.json`.
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-haiku-4-5-20251001";

/// Qual API de LLM usar para reformatar/traduzir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    Openai,
    Anthropic,
}

/// Onde a transcrição acontece.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionProvider {
    /// Whisper.cpp local — offline, sem enviar áudio pra fora.
    Local,
    /// OpenAI Whisper API (whisper-1) — mais preciso, mas depende de rede
    /// e da chave da OpenAI. Envia o áudio pro servidor da OpenAI.
    OpenaiCloud,
}

impl Default for TranscriptionProvider {
    fn default() -> Self {
        TranscriptionProvider::Local
    }
}

/// Tamanhos de modelo do whisper.cpp que oferecemos. Cada um é uma variante
/// quantizada (q5) — o melhor compromisso tamanho/qualidade. O usuário pode
/// escolher outro tamanho depois pelo settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhisperModel {
    Tiny,
    Base,
    Small,
    Medium,
    /// `large-v3-turbo` quantizado — precisão de large com metade do tamanho
    /// e ~2x mais rápido que large-v3 normal.
    LargeTurbo,
}

impl Default for WhisperModel {
    fn default() -> Self {
        // Small tem o melhor equilíbrio para CPU comum em PT-BR.
        // Também mantém compatibilidade com quem já tinha esse modelo baixado.
        WhisperModel::Small
    }
}

/// Como sinalizar visualmente que a gravação está em curso (importante quando
/// o app está no tray e outras janelas cobrem a UI principal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisualIndicator {
    /// Sem nenhum indicador (só a UI da janela principal, se aberta).
    None,
    /// Só a janelinha flutuante que aparece perto do centro-inferior da tela.
    Floating,
    /// Só troca o ícone da bandeja pra vermelho enquanto grava.
    Tray,
    /// Ambos: janela flutuante + tray colorido.
    Both,
}

impl VisualIndicator {
    pub fn uses_floating(self) -> bool {
        matches!(self, Self::Floating | Self::Both)
    }
    pub fn uses_tray(self) -> bool {
        matches!(self, Self::Tray | Self::Both)
    }
}

impl Default for LlmProvider {
    fn default() -> Self {
        // OpenAI é o default por ser o mais comum. Pode ser trocado no
        // config.json ou na UI de settings (etapa 8).
        LlmProvider::Openai
    }
}

impl Default for VisualIndicator {
    fn default() -> Self {
        VisualIndicator::Both
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Qual provider está ativo. Só a chave correspondente é usada.
    #[serde(default)]
    pub provider: LlmProvider,

    /// Chave da API da OpenAI (formato `sk-...` ou `sk-proj-...`).
    /// Só é usada se `provider = "openai"`.
    #[serde(default)]
    pub openai_api_key: String,

    /// Chave da API da Anthropic (formato `sk-ant-...`).
    /// Só é usada se `provider = "anthropic"`.
    #[serde(default)]
    pub anthropic_api_key: String,

    /// Modelo a usar. **Deve corresponder ao provider ativo**:
    ///   - OpenAI: "gpt-4o-mini", "gpt-4o", "gpt-5", etc.
    ///   - Anthropic: "claude-haiku-4-5-20251001", "claude-sonnet-4-6", etc.
    ///
    /// Se estiver vazio, usamos o default do provider ativo (via `active_model`).
    #[serde(default)]
    pub llm_model: String,

    /// Config de tradução automática.
    #[serde(default)]
    pub translate: TranslateConfig,

    /// Que tipo de indicador visual mostrar durante gravação/processamento.
    #[serde(default)]
    pub visual_indicator: VisualIndicator,

    /// Combinação de atalho global (push-to-talk). Aceita o formato de
    /// accelerator do Tauri/Electron: `"F9"`, `"CommandOrControl+Shift+K"`,
    /// `"Alt+Space"` etc. Vazio ou inválido no arquivo → volta pro default.
    #[serde(default = "default_hotkey")]
    pub hotkey: String,

    /// Onde transcrever: local (whisper.cpp) ou nuvem (OpenAI API).
    #[serde(default)]
    pub transcription_provider: TranscriptionProvider,

    /// Qual modelo local usar (só relevante se `transcription_provider = local`).
    /// Se o arquivo ainda não foi baixado, a transcrição erra com uma mensagem
    /// pedindo pra ir em settings e baixar.
    #[serde(default)]
    pub whisper_model: WhisperModel,

    /// Se `true`, o LLM ganha um hint contextual baseado no app onde a
    /// gravação foi disparada (chat casual x email formal x IDE etc.).
    /// Se `false`, o prompt é o mesmo pra qualquer app.
    #[serde(default = "default_true")]
    pub adapt_prompt_to_active_app: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateConfig {
    /// Se ligado, o LLM traduz o texto para `target_language` em vez de
    /// apenas reformatar no idioma original.
    #[serde(default)]
    pub enabled: bool,

    /// Código de idioma alvo (ex: "en", "es", "fr"). Só usado se `enabled = true`.
    #[serde(default = "default_target_language")]
    pub target_language: String,
}

impl AppConfig {
    /// Retorna a chave da API do provider ativo (ou string vazia se não configurada).
    pub fn active_api_key(&self) -> &str {
        match self.provider {
            LlmProvider::Openai => &self.openai_api_key,
            LlmProvider::Anthropic => &self.anthropic_api_key,
        }
    }

    /// Retorna o modelo a usar (explícito no config ou default do provider).
    pub fn active_model(&self) -> &str {
        if !self.llm_model.trim().is_empty() {
            return &self.llm_model;
        }
        match self.provider {
            LlmProvider::Openai => DEFAULT_OPENAI_MODEL,
            LlmProvider::Anthropic => DEFAULT_ANTHROPIC_MODEL,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: LlmProvider::default(),
            openai_api_key: String::new(),
            anthropic_api_key: String::new(),
            llm_model: String::new(),
            translate: TranslateConfig::default(),
            visual_indicator: VisualIndicator::default(),
            hotkey: default_hotkey(),
            transcription_provider: TranscriptionProvider::default(),
            whisper_model: WhisperModel::default(),
            adapt_prompt_to_active_app: true,
        }
    }
}

/// Ajudante para o `#[serde(default = "default_true")]` acima — serde não
/// aceita literal `true` direto.
fn default_true() -> bool {
    true
}

impl Default for TranslateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            target_language: default_target_language(),
        }
    }
}

fn default_target_language() -> String {
    "en".to_string()
}

fn default_hotkey() -> String {
    "F9".to_string()
}

/// Handle compartilhado para o config — guardado como state do Tauri.
pub type SharedConfig = Arc<Mutex<AppConfig>>;

/// Path do arquivo de config no disco.
pub fn config_file_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("falha ao obter app data dir")?;
    // `create_dir_all` é idempotente — não erra se a pasta já existe.
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("falha ao criar app data dir em {}", dir.display()))?;
    Ok(dir.join("config.json"))
}

/// Carrega o config do disco. Se o arquivo não existir, retorna defaults
/// (não erra — é o caso normal na primeira execução).
pub fn load<R: Runtime>(app: &AppHandle<R>) -> Result<AppConfig> {
    let path = config_file_path(app)?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let s = std::fs::read_to_string(&path)
        .with_context(|| format!("falha ao ler config em {}", path.display()))?;
    let cfg: AppConfig = serde_json::from_str(&s)
        .with_context(|| format!("config.json inválido em {}", path.display()))?;
    Ok(cfg)
}

/// Salva o config no disco. Usado quando a UI de settings edita algo.
#[allow(dead_code)] // usado a partir da etapa 8 (UI de settings)
pub fn save<R: Runtime>(app: &AppHandle<R>, cfg: &AppConfig) -> Result<()> {
    let path = config_file_path(app)?;
    let s = serde_json::to_string_pretty(cfg).context("falha ao serializar config")?;
    std::fs::write(&path, s)
        .with_context(|| format!("falha ao escrever config em {}", path.display()))?;
    Ok(())
}
