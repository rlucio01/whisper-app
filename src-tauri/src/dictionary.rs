//! Dicionário Pessoal, vocabulário especializado e correção automática de palavras.
//!
//! Inspirado no funcionamento do Whisper Flow:
//! 1. Vocabulário personalizado (`custom_words`): nomes próprios, siglas e jargões
//!    técnicos que são passados como prompt inicial para o Whisper (CPU, GPU e Nuvem)
//!    e instruídos no prompt do LLM para garantir a grafia correta.
//! 2. Regras de substituição (`replacements`): pares de de -> para para correções
//!    diretas e instantâneas (ex.: "anti gravidade" -> "Antigravity").
//! 3. Rastreador de frequência (`frequency_words.json`): contabiliza termos
//!    recorrentes ditados pelo usuário para sugerir adição ao vocabulário em 1 clique.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

/// Regra de substituição de termo/palavra (de -> para).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WordReplacement {
    /// Texto ou expressão a ser substituída.
    pub from: String,
    /// Texto corrigido substituto.
    pub to: String,
}

/// Configuração do Dicionário e Vocabulário do usuário.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DictionaryConfig {
    /// Se a correção automática e injeção de vocabulário estão ativas.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Lista de palavras personalizadas, termos técnicos e nomes próprios.
    #[serde(default)]
    pub custom_words: Vec<String>,

    /// Lista de regras de substituição direta (de -> para).
    #[serde(default)]
    pub replacements: Vec<WordReplacement>,
}

fn default_true() -> bool {
    true
}

impl Default for DictionaryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            custom_words: Vec::new(),
            replacements: Vec::new(),
        }
    }
}

/// Constrói a string de prompt inicial para o Whisper com base no vocabulário.
/// Whisper aceita até ~200 tokens de prompt para guiar o estilo e vocabulário.
pub fn build_whisper_prompt(cfg: &DictionaryConfig) -> Option<String> {
    if !cfg.enabled || cfg.custom_words.is_empty() {
        return None;
    }

    let words: Vec<&str> = cfg
        .custom_words
        .iter()
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .collect();

    if words.is_empty() {
        return None;
    }

    // Formata como uma lista contextual de termos
    Some(format!("Vocabulário: {}.", words.join(", ")))
}

/// Aplica as regras de substituição direta em um texto.
/// Utiliza correspondência case-insensitive e preserva limites de palavra.
pub fn apply_replacements(text: &str, replacements: &[WordReplacement]) -> String {
    if replacements.is_empty() || text.is_empty() {
        return text.to_string();
    }

    let mut result = text.to_string();

    for rule in replacements {
        let from = rule.from.trim();
        let to = rule.to.trim();
        if from.is_empty() {
            continue;
        }

        result = replace_case_insensitive_word(&result, from, to);
    }

    result
}

/// Substitui todas as ocorrências de `needle` em `haystack` ignorando case,
/// respeitando limites de palavra (início/fim de texto, espaço ou pontuação).
fn replace_case_insensitive_word(haystack: &str, needle: &str, replacement: &str) -> String {
    let lower_haystack = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();
    let needle_len = needle.len();

    let mut out = String::with_capacity(haystack.len());
    let mut last_idx = 0;

    while let Some(match_idx) = lower_haystack[last_idx..].find(&lower_needle) {
        let abs_match = last_idx + match_idx;
        let abs_end = abs_match + needle_len;

        // Checa limite anterior
        let boundary_before = if abs_match == 0 {
            true
        } else {
            let prev_char = haystack[..abs_match].chars().last().unwrap_or(' ');
            !prev_char.is_alphanumeric() && prev_char != '_'
        };

        // Checa limite posterior
        let boundary_after = if abs_end >= haystack.len() {
            true
        } else {
            let next_char = haystack[abs_end..].chars().next().unwrap_or(' ');
            !next_char.is_alphanumeric() && next_char != '_'
        };

        if boundary_before && boundary_after {
            out.push_str(&haystack[last_idx..abs_match]);
            out.push_str(replacement);
            last_idx = abs_end;
        } else {
            // Não bateu o limite de palavra, avança um pedaço
            let next_char_idx = haystack[abs_match..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| abs_match + i)
                .unwrap_or(abs_match + 1);
            out.push_str(&haystack[last_idx..next_char_idx]);
            last_idx = next_char_idx;
        }
    }

    out.push_str(&haystack[last_idx..]);
    out
}

// ---------- Rastreador de Frequência de Palavras ----------

const FREQUENCY_FILE: &str = "frequency_words.json";

/// Palavras comuns / stopwords em português e inglês para não poluir
/// a lista de sugestões de vocabulário técnico.
static STOPWORDS: &[&str] = &[
    "de", "a", "o", "que", "e", "do", "da", "em", "um", "para", "é", "com", "não", "uma",
    "os", "no", "se", "na", "por", "mais", "as", "dos", "como", "mas", "foi", "ao", "ele",
    "das", "tem", "à", "seu", "sua", "ou", "ser", "quando", "muito", "nos", "já", "está",
    "eu", "também", "só", "pelo", "pela", "até", "isso", "ela", "entre", "era", "depois",
    "sem", "mesmo", "aos", "ter", "seus", "quem", "nas", "me", "esse", "eles", "estão",
    "você", "tinha", "foram", "essa", "num", "nem", "suas", "meu", "às", "minha", "têm",
    "numa", "pelos", "elas", "havia", "seja", "qual", "será", "nós", "tenho", "lhe", "deles",
    "essas", "esses", "pelas", "este", "fosse", "dele", "tu", "te", "vocês", "vos", "lhes",
    "meus", "minhas", "teu", "tua", "teus", "tuas", "nosso", "nossa", "nossos", "nossas",
    "dela", "delas", "esta", "estes", "estas", "aquele", "aquela", "aqueles", "aquelas",
    "isto", "aquilo", "estou", "estamos", "estive", "esteve", "estivemos",
    "estiveram", "estava", "estávamos", "estavam", "estivera", "estivéramos", "esteja",
    "estejamos", "estejam", "estivesse", "estivéssemos", "estivessem", "estiver", "estivermos",
    "estiverem", "hei", "há", "havemos", "hão", "houve", "houvemos", "houveram", "houvera",
    "houvéramos", "haja", "hajamos", "hajam", "houvesse", "houvéssemos", "houvessem", "houver",
    "houvermos", "houverem", "houverei", "houverá", "houveremos", "houverão", "houveria",
    "houveríamos", "houveriam", "sou", "somos", "são", "éramos", "eram", "fui",
    "fomos", "fora", "fôramos", "sejamos", "sejam", "fôssemos",
    "forem", "serei", "seremos", "serão", "seria", "seríamos", "seriam",
    "temos", "tém", "tínhamos", "tinham", "tive", "teve", "tivemos", "tivera",
    "tenha", "tenhamos", "tenham", "the", "be", "to", "of", "and", "in", "that",
    "have", "it", "for", "not", "on", "with", "he", "as", "you", "do", "at", "this",
    "but", "his", "by", "from", "they", "we", "say", "her", "she", "or", "an", "will",
    "my", "one", "all", "would", "there", "their", "what",
];

static FREQUENCY_MUTEX: Mutex<()> = Mutex::new(());

fn frequency_file_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("falha ao resolver app_data_dir")?;
    Ok(dir.join(FREQUENCY_FILE))
}

/// Lê o mapa de frequências salvo no disco.
fn load_frequencies<R: Runtime>(app: &AppHandle<R>) -> HashMap<String, u32> {
    let Ok(path) = frequency_file_path(app) else {
        return HashMap::new();
    };
    if !path.exists() {
        return HashMap::new();
    }
    let Ok(data) = fs::read_to_string(&path) else {
        return HashMap::new();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

/// Salva o mapa de frequências no disco.
fn save_frequencies<R: Runtime>(app: &AppHandle<R>, map: &HashMap<String, u32>) -> Result<()> {
    let path = frequency_file_path(app)?;
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json = serde_json::to_string(map)?;
    fs::write(&path, json)?;
    Ok(())
}

/// Registra as palavras de uma transcrição concluída no rastreador de frequência.
pub fn record_dictated_text<R: Runtime>(app: &AppHandle<R>, text: &str) {
    if text.trim().is_empty() {
        return;
    }

    let Ok(_lock) = FREQUENCY_MUTEX.lock() else {
        return;
    };
    let mut map = load_frequencies(app);

    // Extrai palavras separando por pontuação e espaços
    for raw in text.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_') {
        let trimmed = raw.trim();
        if trimmed.chars().count() < 3 {
            continue;
        }
        let lower = trimmed.to_lowercase();

        // Ignora stopwords muito comuns
        if STOPWORDS.contains(&lower.as_str()) {
            continue;
        }

        // Normaliza mantendo maiúsculas se houver
        *map.entry(trimmed.to_string()).or_insert(0) += 1;
    }

    let _ = save_frequencies(app, &map);
}

/// Retorna as palavras mais frequentes detectadas pelo rastreador.
pub fn get_top_frequent_words<R: Runtime>(app: &AppHandle<R>, limit: usize) -> Vec<(String, u32)> {
    let Ok(_lock) = FREQUENCY_MUTEX.lock() else {
        return Vec::new();
    };
    let map = load_frequencies(app);

    let mut list: Vec<(String, u32)> = map.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1));
    list.truncate(limit);
    list
}

/// Limpa o histórico de frequência de palavras.
pub fn clear_frequencies<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let Ok(_lock) = FREQUENCY_MUTEX.lock() else {
        return Ok(());
    };
    let path = frequency_file_path(app)?;
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    Ok(())
}
