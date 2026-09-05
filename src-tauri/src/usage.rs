//! Rastreamento e monitoramento de consumo e limites de API (Groq e OpenAI).
//!
//! Monitora em tempo real:
//!   - Speech-to-Text: Requests per Minute (RPM), Requests per Day (RPD),
//!     Audio Seconds per Hour (ASPH) e Audio Seconds per Day (ASPD).
//!   - Chat/LLM: Requests per Minute (RPM), Requests per Day (RPD),
//!     Tokens per Minute (TPM) e Tokens per Day (TPD).
//!
//! Os dados são persistidos em `%APPDATA%\com.rlucio.whisperapp\usage.json`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

/// Um evento de consumo registrado pelo app.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    /// Timestamp Unix em segundos.
    pub timestamp: u64,
    /// Provedor ("groq", "openai", "openrouter", etc.).
    pub provider: String,
    /// Serviço ("stt" para Speech-to-Text ou "llm" para Chat Completions).
    pub service: String,
    /// Duração do áudio processado em segundos (0 para chamadas de LLM).
    pub audio_seconds: f32,
    /// Tokens consumidos (0 para chamadas de STT).
    pub tokens: u32,
}

/// Limites conhecidos ou sincronizados para um provedor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderLimits {
    pub stt_rpm: u32,
    pub stt_rpd: u32,
    pub stt_asph: f32, // Audio Seconds per Hour
    pub stt_aspd: f32, // Audio Seconds per Day
    pub llm_rpm: u32,
    pub llm_rpd: u32,
    pub llm_tpm: u32,
    pub llm_tpd: u32,
}

impl ProviderLimits {
    pub fn default_for(provider: &str) -> Self {
        match provider.to_lowercase().as_str() {
            "groq" => Self {
                stt_rpm: 20,
                stt_rpd: 2_000,
                stt_asph: 7_200.0, // 2 horas de áudio por hora
                stt_aspd: 28_800.0, // 8 horas de áudio por dia
                llm_rpm: 30,
                llm_rpd: 1_000,
                llm_tpm: 30_000,
                llm_tpd: 200_000,
            },
            "openai" => Self {
                stt_rpm: 50,
                stt_rpd: 1_000,
                stt_asph: 7_200.0,
                stt_aspd: 28_800.0,
                llm_rpm: 500,
                llm_rpd: 10_000,
                llm_tpm: 200_000,
                llm_tpd: 2_000_000,
            },
            _ => Self {
                stt_rpm: 30,
                stt_rpd: 1_000,
                stt_asph: 7_200.0,
                stt_aspd: 28_800.0,
                llm_rpm: 60,
                llm_rpd: 2_000,
                llm_tpm: 60_000,
                llm_tpd: 500_000,
            },
        }
    }
}

/// Métrica individual com valor consumido, limite e porcentagem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricItem {
    pub current: f32,
    pub limit: f32,
    pub percent: f32,
    pub unit: String,
}

/// Relatório de métricas formatado para a interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageReport {
    pub provider: String,
    // Speech-to-Text
    pub stt_audio_seconds_hour: MetricItem,
    pub stt_audio_seconds_day: MetricItem,
    pub stt_requests_minute: MetricItem,
    pub stt_requests_day: MetricItem,
    // LLM / Chat Completions
    pub llm_tokens_minute: MetricItem,
    pub llm_tokens_day: MetricItem,
    pub llm_requests_minute: MetricItem,
    pub llm_requests_day: MetricItem,
    // Alertas
    pub highest_usage_percent: f32,
    pub is_near_limit: bool,
    pub alert_message: Option<String>,
}

/// Estrutura persistida no disco.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedUsage {
    events: Vec<UsageEvent>,
}

pub struct UsageTracker {
    events: VecDeque<UsageEvent>,
    file_path: PathBuf,
}

pub type SharedUsage = Arc<Mutex<UsageTracker>>;

impl UsageTracker {
    pub fn load_or_new(app_data_dir: &Path) -> Self {
        let file_path = app_data_dir.join("usage.json");
        let mut events = VecDeque::new();

        if file_path.exists() {
            if let Ok(content) = fs::read_to_string(&file_path) {
                if let Ok(persisted) = serde_json::from_str::<PersistedUsage>(&content) {
                    let now = now_secs();
                    // Descarta eventos mais velhos que 48 horas (172800 segundos)
                    for ev in persisted.events {
                        if now.saturating_sub(ev.timestamp) <= 172_800 {
                            events.push_back(ev);
                        }
                    }
                }
            }
        }

        Self { events, file_path }
    }

    /// Registra uma chamada de transcrição de áudio (STT).
    pub fn record_stt(&mut self, provider: &str, audio_seconds: f32, _headers: Option<&HeaderMap>) {
        let event = UsageEvent {
            timestamp: now_secs(),
            provider: provider.to_lowercase(),
            service: "stt".to_string(),
            audio_seconds,
            tokens: 0,
        };
        self.add_event(event);
    }

    /// Registra uma chamada de LLM (Chat Completions).
    pub fn record_llm(&mut self, provider: &str, tokens: u32, _headers: Option<&HeaderMap>) {
        let event = UsageEvent {
            timestamp: now_secs(),
            provider: provider.to_lowercase(),
            service: "llm".to_string(),
            audio_seconds: 0.0,
            tokens,
        };
        self.add_event(event);
    }

    fn add_event(&mut self, event: UsageEvent) {
        self.events.push_back(event);
        self.cleanup();
        let _ = self.save();
    }

    /// Limpa eventos com mais de 48 horas.
    fn cleanup(&mut self) {
        let now = now_secs();
        while let Some(front) = self.events.front() {
            if now.saturating_sub(front.timestamp) > 172_800 {
                self.events.pop_front();
            } else {
                break;
            }
        }
    }

    /// Salva o estado atual no disco.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let data = PersistedUsage {
            events: self.events.iter().cloned().collect(),
        };
        let json = serde_json::to_string_pretty(&data).context("falha ao serializar usage")?;
        fs::write(&self.file_path, json).context("falha ao salvar usage.json")?;
        Ok(())
    }

    /// Zera o histórico de eventos.
    pub fn clear(&mut self) -> Result<()> {
        self.events.clear();
        self.save()
    }

    /// Gera o relatório consolidado de métricas para o provedor solicitado.
    pub fn get_report(&self, provider: &str) -> UsageReport {
        let now = now_secs();
        let provider_norm = provider.to_lowercase();
        let limits = ProviderLimits::default_for(&provider_norm);

        // Início do dia UTC atual (timestamp zerado às 00:00 UTC)
        let day_start = (now / 86_400) * 86_400;
        let hour_ago = now.saturating_sub(3_600);
        let minute_ago = now.saturating_sub(60);

        let mut stt_reqs_minute = 0u32;
        let mut stt_reqs_day = 0u32;
        let mut stt_sec_hour = 0.0f32;
        let mut stt_sec_day = 0.0f32;

        let mut llm_reqs_minute = 0u32;
        let mut llm_reqs_day = 0u32;
        let mut llm_tokens_minute = 0u32;
        let mut llm_tokens_day = 0u32;

        for ev in &self.events {
            if ev.provider != provider_norm {
                continue;
            }

            if ev.service == "stt" {
                if ev.timestamp >= minute_ago {
                    stt_reqs_minute += 1;
                }
                if ev.timestamp >= hour_ago {
                    stt_sec_hour += ev.audio_seconds;
                }
                if ev.timestamp >= day_start {
                    stt_reqs_day += 1;
                    stt_sec_day += ev.audio_seconds;
                }
            } else if ev.service == "llm" {
                if ev.timestamp >= minute_ago {
                    llm_reqs_minute += 1;
                    llm_tokens_minute += ev.tokens;
                }
                if ev.timestamp >= day_start {
                    llm_reqs_day += 1;
                    llm_tokens_day += ev.tokens;
                }
            }
        }

        let make_metric = |curr: f32, limit: f32, unit: &str| -> MetricItem {
            let percent = if limit > 0.0 {
                ((curr / limit) * 100.0).min(100.0)
            } else {
                0.0
            };
            MetricItem {
                current: curr,
                limit,
                percent,
                unit: unit.to_string(),
            }
        };

        let stt_audio_seconds_hour =
            make_metric(stt_sec_hour, limits.stt_asph, "segundos/hora");
        let stt_audio_seconds_day =
            make_metric(stt_sec_day, limits.stt_aspd, "segundos/dia");
        let stt_requests_minute =
            make_metric(stt_reqs_minute as f32, limits.stt_rpm as f32, "req/min");
        let stt_requests_day =
            make_metric(stt_reqs_day as f32, limits.stt_rpd as f32, "req/dia");

        let llm_tokens_minute =
            make_metric(llm_tokens_minute as f32, limits.llm_tpm as f32, "tokens/min");
        let llm_tokens_day =
            make_metric(llm_tokens_day as f32, limits.llm_tpd as f32, "tokens/dia");
        let llm_requests_minute =
            make_metric(llm_reqs_minute as f32, limits.llm_rpm as f32, "req/min");
        let llm_requests_day =
            make_metric(llm_reqs_day as f32, limits.llm_rpd as f32, "req/dia");

        let mut max_pct = 0.0f32;
        let mut alert_item = None;

        for (item, name) in [
            (&stt_audio_seconds_hour, "Segundos de Áudio por Hora"),
            (&stt_audio_seconds_day, "Segundos de Áudio por Dia"),
            (&stt_requests_minute, "Requisições STT por Minuto"),
            (&stt_requests_day, "Requisições STT por Dia"),
            (&llm_tokens_minute, "Tokens por Minuto"),
            (&llm_tokens_day, "Tokens por Dia"),
            (&llm_requests_minute, "Requisições LLM por Minuto"),
            (&llm_requests_day, "Requisições LLM por Dia"),
        ] {
            if item.percent > max_pct {
                max_pct = item.percent;
                if item.percent >= 80.0 {
                    alert_item = Some(format!(
                        "Atenção: você atingiu {:.0}% do limite de {} ({:.0}/{:.0} {}) do {}.",
                        item.percent, name, item.current, item.limit, item.unit, provider
                    ));
                }
            }
        }

        let is_near_limit = max_pct >= 80.0;

        UsageReport {
            provider: provider.to_string(),
            stt_audio_seconds_hour,
            stt_audio_seconds_day,
            stt_requests_minute,
            stt_requests_day,
            llm_tokens_minute,
            llm_tokens_day,
            llm_requests_minute,
            llm_requests_day,
            highest_usage_percent: max_pct,
            is_near_limit,
            alert_message: alert_item,
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
