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
//!   - Suporta seis providers de LLM: OpenAI, Anthropic, OpenRouter, Groq,
//!     Google Gemini e xAI (Grok). O campo `provider` escolhe qual é usado;
//!     cada um tem seu próprio campo de chave para o usuário poder trocar sem
//!     precisar reconfigurar as outras.
//!   - A UI de settings chama `load` / `save` via comandos Tauri.
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
const DEFAULT_OPENROUTER_MODEL: &str = "openai/gpt-4o-mini";
const DEFAULT_GROQ_MODEL: &str = "openai/gpt-oss-120b";
const DEFAULT_GEMINI_MODEL: &str = "gemini-2.0-flash";
const DEFAULT_XAI_MODEL: &str = "grok-3-mini";

/// Qual API de LLM usar para reformatar/traduzir.
///
/// Os quatro últimos (OpenRouter, Groq, xAI) usam a API compatível com o
/// Chat Completions da OpenAI — mudam só endpoint, chave e headers. Gemini
/// tem formato próprio (`generateContent`). Anthropic tem o `messages` dela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    Openai,
    Anthropic,
    Openrouter,
    Groq,
    Gemini,
    Xai,
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
    /// Groq Whisper API (whisper-large-v3-turbo) — mesmo formato de request
    /// da OpenAI (multipart), mas a inferência roda em LPU: normalmente bem
    /// mais rápida que a OpenAI pra áudios curtos. Usa `groq_api_key`.
    GroqCloud,
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

/// Onde a barra flutuante (overlay) aparece na tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverlayPosition {
    Top,
    Bottom,
}

impl Default for OverlayPosition {
    fn default() -> Self {
        OverlayPosition::Bottom
    }
}

/// Customização visual da barra flutuante. Separado num sub-struct (como
/// `TranslateConfig`) porque são campos relacionados que a UI de settings
/// edita juntos numa seção só.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    /// Topo ou fundo da tela.
    #[serde(default)]
    pub position: OverlayPosition,

    /// Fator de escala do tamanho da barra (1.0 = tamanho padrão). Aplicado
    /// redimensionando a janela nativa do overlay — ver `visual::show_overlay`.
    #[serde(default = "default_overlay_scale")]
    pub scale: f32,

    /// Opacidade do fundo da barra, de 0.0 (invisível) a 1.0 (sólido).
    #[serde(default = "default_overlay_opacity")]
    pub opacity: f32,

    /// Cor de destaque (dot de gravação, barras da onda de áudio, ícones dos
    /// botões de hover), em formato hex `"#rrggbb"`.
    #[serde(default = "default_overlay_accent")]
    pub accent_color: String,
}

fn default_overlay_scale() -> f32 {
    1.0
}

fn default_overlay_opacity() -> f32 {
    0.92
}

fn default_overlay_accent() -> String {
    "#ef4444".to_string()
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            position: OverlayPosition::default(),
            scale: default_overlay_scale(),
            opacity: default_overlay_opacity(),
            accent_color: default_overlay_accent(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Qual provider está ativo. Só a chave correspondente é usada.
    #[serde(default)]
    pub provider: LlmProvider,

    /// Chave da API da OpenAI (formato `sk-...` ou `sk-proj-...`).
    /// Só é usada se `provider = "openai"`. Também é a chave usada para
    /// transcrição via `openai_cloud` (independente do provider de LLM).
    #[serde(default)]
    pub openai_api_key: String,

    /// Chave da API da Anthropic (formato `sk-ant-...`).
    /// Só é usada se `provider = "anthropic"`.
    #[serde(default)]
    pub anthropic_api_key: String,

    /// Chave da API do OpenRouter (formato `sk-or-v1-...`).
    /// Só é usada se `provider = "openrouter"`.
    #[serde(default)]
    pub openrouter_api_key: String,

    /// Chave da API do Groq (formato `gsk_...`).
    /// Só é usada se `provider = "groq"`.
    #[serde(default)]
    pub groq_api_key: String,

    /// Chave da API do Google Gemini (formato `AIza...`).
    /// Só é usada se `provider = "gemini"`.
    #[serde(default)]
    pub gemini_api_key: String,

    /// Chave da API do xAI (formato `xai-...`).
    /// Só é usada se `provider = "xai"`.
    #[serde(default)]
    pub xai_api_key: String,

    /// Modelo a usar. **Deve corresponder ao provider ativo**:
    ///   - OpenAI: "gpt-4o-mini", "gpt-4o", "gpt-5", etc.
    ///   - Anthropic: "claude-haiku-4-5-20251001", "claude-sonnet-4-6", etc.
    ///   - OpenRouter: "openai/gpt-4o-mini", "anthropic/claude-haiku-4.5", etc.
    ///   - Groq: "openai/gpt-oss-120b", "openai/gpt-oss-20b", etc.
    ///   - Gemini: "gemini-2.0-flash", "gemini-2.5-pro", etc.
    ///   - xAI: "grok-3-mini", "grok-4-latest", etc.
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

    /// Customização visual da barra flutuante (posição, escala, opacidade,
    /// cor de destaque). Só tem efeito se `visual_indicator` usa floating.
    #[serde(default)]
    pub overlay: OverlayConfig,

    /// Combinação de atalho global (push-to-talk). Aceita o formato de
    /// accelerator do Tauri/Electron: `"F9"`, `"CommandOrControl+Shift+K"`,
    /// `"Alt+Space"` etc. Vazio ou inválido no arquivo → volta pro default.
    #[serde(default = "default_hotkey")]
    pub hotkey: String,

    /// Combinação de atalho pro modo hands-free: toque uma vez pra começar
    /// a gravar, toque de novo pra parar (não precisa segurar). Mesmo
    /// formato de accelerator do campo `hotkey`. Vazio = desativado — só
    /// push-to-talk fica ativo.
    #[serde(default)]
    pub hands_free_hotkey: String,

    /// Combinação de atalho pra recolar a última transcrição no app ativo,
    /// sem precisar regravar. Mesmo formato de accelerator dos outros dois.
    /// Vazio = desativado.
    #[serde(default)]
    pub repaste_hotkey: String,

    /// Onde transcrever: local (whisper.cpp) ou nuvem (OpenAI API).
    #[serde(default)]
    pub transcription_provider: TranscriptionProvider,

    /// Idioma esperado da fala no reconhecimento. Vazio mantém a detecção
    /// automática do Whisper (comportamento das versões anteriores). Quando
    /// preenchido, usamos o código ISO 639-1 tanto no modo local quanto nas
    /// APIs cloud, o que costuma melhorar precisão e reduzir a latência em
    /// ditados monolíngues.
    #[serde(default)]
    pub transcription_language: String,

    /// Qual modelo local usar (só relevante se `transcription_provider = local`).
    /// Se o arquivo ainda não foi baixado, a transcrição erra com uma mensagem
    /// pedindo pra ir em settings e baixar.
    #[serde(default)]
    pub whisper_model: WhisperModel,

    /// Nome do dispositivo de entrada de áudio a usar (exatamente como
    /// `cpal` reporta em `Device::name()`). Vazio = usa o device default do
    /// SO. Se o dispositivo salvo não existir mais (desconectado), a
    /// gravação falha com uma mensagem pedindo pra escolher outro.
    #[serde(default)]
    pub microphone: String,

    /// Se `true`, o LLM ganha um hint contextual baseado no app onde a
    /// gravação foi disparada (chat casual x email formal x IDE etc.).
    /// Se `false`, o prompt é o mesmo pra qualquer app.
    #[serde(default = "default_true")]
    pub adapt_prompt_to_active_app: bool,

    /// Toca um beep curto ao começar a gravação e outro ao terminar o
    /// pipeline (texto colado). Ajuda como feedback quando o app está
    /// no tray e a janela principal não está visível.
    #[serde(default = "default_true")]
    pub sound_feedback: bool,

    /// Se `true`, pula a chamada de LLM e cola a transcrição bruta — mesmo
    /// que uma chave de API válida esteja configurada. Diferente de deixar
    /// a chave em branco: o usuário mantém a chave salva (e pode religar a
    /// formatação a qualquer momento) sem precisar apagar/recolar a chave.
    /// Também desativa a tradução automática, já que ela depende do LLM.
    #[serde(default)]
    pub skip_llm_formatting: bool,

    /// Se `true`, o app sobe direto para a bandeja do sistema, sem mostrar a
    /// janela principal. O hotkey e os serviços de background funcionam
    /// normalmente — só a janela fica escondida até o usuário clicar no
    /// ícone do tray. Padrão `false` (mantém o comportamento atual).
    #[serde(default)]
    pub start_minimized: bool,
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
            LlmProvider::Openrouter => &self.openrouter_api_key,
            LlmProvider::Groq => &self.groq_api_key,
            LlmProvider::Gemini => &self.gemini_api_key,
            LlmProvider::Xai => &self.xai_api_key,
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
            LlmProvider::Openrouter => DEFAULT_OPENROUTER_MODEL,
            LlmProvider::Groq => DEFAULT_GROQ_MODEL,
            LlmProvider::Gemini => DEFAULT_GEMINI_MODEL,
            LlmProvider::Xai => DEFAULT_XAI_MODEL,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: LlmProvider::default(),
            openai_api_key: String::new(),
            anthropic_api_key: String::new(),
            openrouter_api_key: String::new(),
            groq_api_key: String::new(),
            gemini_api_key: String::new(),
            xai_api_key: String::new(),
            llm_model: String::new(),
            translate: TranslateConfig::default(),
            visual_indicator: VisualIndicator::default(),
            overlay: OverlayConfig::default(),
            hotkey: default_hotkey(),
            hands_free_hotkey: String::new(),
            repaste_hotkey: String::new(),
            transcription_provider: TranscriptionProvider::default(),
            transcription_language: String::new(),
            whisper_model: WhisperModel::default(),
            microphone: String::new(),
            adapt_prompt_to_active_app: true,
            sound_feedback: true,
            skip_llm_formatting: false,
            start_minimized: false,
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
