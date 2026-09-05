import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { displayHotkey } from "./hotkeyFormat";
import { playBeep } from "./sound";
import type { useUpdater } from "./useUpdater";

// Tipo espelhando `AppConfig` no Rust (config.rs).
type Provider =
  | "openai"
  | "anthropic"
  | "openrouter"
  | "groq"
  | "gemini"
  | "xai";
type VisualIndicator = "none" | "floating" | "tray" | "both";
type OverlayPosition = "top" | "bottom";
type TranscriptionProvider = "local" | "openai_cloud" | "groq_cloud";
type WhisperModelSlug = "tiny" | "base" | "small" | "medium" | "large_turbo";
type InferenceDevice = "auto" | "gpu" | "cpu";

interface GpuInfo {
  name: string;
  vendor: string;
  vram_mb: number;
  shared_ram_mb: number;
  is_discrete: boolean;
  recommendation: string;
}

interface HardwareReport {
  gpus: GpuInfo[];
  primary_gpu: GpuInfo | null;
  recommended_device: string;
  cpu_cores: number;
  has_avx: boolean;
}

interface BenchmarkResult {
  duration_ms: number;
  audio_duration_sec: number;
  speedup_factor: number;
  model: string;
  device_used: string;
  message: string;
}

interface GpuRuntimeStatus {
  is_nvidia_detected: boolean;
  installed: boolean;
  is_downloading: boolean;
  size_mb: number;
  cli_path: string | null;
}

interface GpuRuntimeProgress {
  downloaded: number;
  total: number;
}

interface MetricItem {
  current: number;
  limit: number;
  percent: number;
  unit: string;
}

interface UsageReport {
  provider: string;
  stt_audio_seconds_hour: MetricItem;
  stt_audio_seconds_day: MetricItem;
  stt_requests_minute: MetricItem;
  stt_requests_day: MetricItem;
  llm_tokens_minute: MetricItem;
  llm_tokens_day: MetricItem;
  llm_requests_minute: MetricItem;
  llm_requests_day: MetricItem;
  highest_usage_percent: number;
  is_near_limit: boolean;
  alert_message?: string;
}

interface AppConfig {
  provider: Provider;
  openai_api_key: string;
  anthropic_api_key: string;
  openrouter_api_key: string;
  groq_api_key: string;
  gemini_api_key: string;
  xai_api_key: string;
  llm_model: string;
  translate: {
    enabled: boolean;
    target_language: string;
  };
  visual_indicator: VisualIndicator;
  overlay: {
    position: OverlayPosition;
    scale: number;
    opacity: number;
    accent_color: string;
  };
  hotkey: string;
  hands_free_hotkey: string;
  repaste_hotkey: string;
  transcription_provider: TranscriptionProvider;
  transcription_language: string;
  whisper_model: WhisperModelSlug;
  inference_device?: InferenceDevice;
  microphone: string;
  adapt_prompt_to_active_app: boolean;
  sound_feedback: boolean;
  skip_llm_formatting: boolean;
  start_minimized: boolean;
  mute_audio_while_recording: boolean;
  autostart_initialized?: boolean;
}

interface ModelStatus {
  slug: WhisperModelSlug;
  display_name: string;
  size_mb: number;
  downloaded: boolean;
  bytes_on_disk: number;
}

const PROVIDER_LABELS: Record<Provider, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  openrouter: "OpenRouter",
  groq: "Groq",
  gemini: "Google Gemini",
  xai: "xAI (Grok)",
};

const API_KEY_PLACEHOLDER: Record<Provider, string> = {
  openai: "sk-... ou sk-proj-...",
  anthropic: "sk-ant-...",
  openrouter: "sk-or-v1-...",
  groq: "gsk_...",
  gemini: "AIza...",
  xai: "xai-...",
};

const GITHUB_REPO_URL = "https://github.com/rlucio01/whisper-app";

const API_KEY_HELP_URL: Record<Provider, string> = {
  openai: "https://platform.openai.com/api-keys",
  anthropic: "https://console.anthropic.com/settings/keys",
  openrouter: "https://openrouter.ai/keys",
  groq: "https://console.groq.com/keys",
  gemini: "https://aistudio.google.com/apikey",
  xai: "https://console.x.ai",
};

interface CuratedModel {
  id: string;
  label: string;
}

/** Modelos "recomendados" que aparecem no dropdown por provider. O primeiro
 *  item de cada lista deve ser o mesmo default do Rust (config.rs) — assim
 *  o dropdown reflete o que o backend usa quando `llm_model` está vazio.
 *  Novos modelos sempre podem ser digitados via opção "personalizado". */
const CURATED_MODELS: Record<Provider, CuratedModel[]> = {
  openai: [
    { id: "gpt-4o-mini", label: "gpt-4o-mini (Mais barato • Recomendado)" },
    { id: "gpt-4o", label: "gpt-4o (Mais inteligente • Alto desempenho)" },
    { id: "o3-mini", label: "o3-mini (Raciocínio rápido • Custo-benefício)" },
    { id: "o1-mini", label: "o1-mini (Raciocínio avançado rápido)" },
    { id: "o1", label: "o1 (Raciocínio profundo)" },
    { id: "gpt-4.5-preview", label: "gpt-4.5-preview (Mais recente • Alta escala)" },
  ],
  groq: [
    { id: "openai/gpt-oss-20b", label: "openai/gpt-oss-20b (Mais rápido • Econômico • Recomendado)" },
    { id: "openai/gpt-oss-120b", label: "openai/gpt-oss-120b (Mais inteligente • Raciocínio profundo)" },
    { id: "groq/compound-mini", label: "groq/compound-mini (Rápido • Ferramentas integradas)" },
    { id: "groq/compound", label: "groq/compound (Sistema completo • Alta precisão)" },
    { id: "qwen/qwen3.6-27b", label: "qwen/qwen3.6-27b (Qwen 3.6 • Multilíngue)" },
  ],
  anthropic: [
    { id: "claude-3-5-haiku-20241022", label: "claude-3-5-haiku (Mais barato • Rápido)" },
    { id: "claude-3-7-sonnet-latest", label: "claude-3-7-sonnet (Mais recente • Raciocínio híbrido)" },
    { id: "claude-3-5-sonnet-20241022", label: "claude-3-5-sonnet (Recomendado)" },
    { id: "claude-3-opus-20240229", label: "claude-3-opus (Alta precisão)" },
  ],
  openrouter: [
    { id: "openai/gpt-4o-mini", label: "openai/gpt-4o-mini (Mais barato • Recomendado)" },
    { id: "deepseek/deepseek-chat", label: "deepseek/deepseek-chat (Mais barato • V3)" },
    { id: "deepseek/deepseek-r1", label: "deepseek/deepseek-r1 (Raciocínio R1)" },
    { id: "anthropic/claude-3.5-haiku", label: "anthropic/claude-3.5-haiku" },
    { id: "google/gemini-2.0-flash-001", label: "google/gemini-2.0-flash-001" },
    { id: "meta-llama/llama-3.3-70b-instruct", label: "meta-llama/llama-3.3-70b-instruct" },
  ],
  gemini: [
    { id: "gemini-2.0-flash-lite", label: "gemini-2.0-flash-lite (Mais barato • Ultra-rápido)" },
    { id: "gemini-2.0-flash", label: "gemini-2.0-flash (Recomendado • Multimodal)" },
    { id: "gemini-2.5-pro", label: "gemini-2.5-pro (Mais recente • Avançado)" },
    { id: "gemini-1.5-pro", label: "gemini-1.5-pro (Contexto 2M tokens)" },
  ],
  xai: [
    { id: "grok-3-mini", label: "grok-3-mini (Mais barato • Rápido)" },
    { id: "grok-3", label: "grok-3 (Mais inteligente • Raciocínio)" },
    { id: "grok-2", label: "grok-2 (Grok 2 anterior)" },
    { id: "grok-beta", label: "grok-beta" },
  ],
};

const DEFAULT_MODEL_OF = (p: Provider) => CURATED_MODELS[p][0].id;

/** Lê a chave do provider ativo (helper pra evitar `config[activeKey]` que o
 *  TS não estreita bem quando o objeto tem campos não-string). */
function getApiKey(cfg: AppConfig, provider: Provider): string {
  switch (provider) {
    case "openai":
      return cfg.openai_api_key;
    case "anthropic":
      return cfg.anthropic_api_key;
    case "openrouter":
      return cfg.openrouter_api_key;
    case "groq":
      return cfg.groq_api_key;
    case "gemini":
      return cfg.gemini_api_key;
    case "xai":
      return cfg.xai_api_key;
  }
}

function setApiKey(cfg: AppConfig, provider: Provider, value: string): AppConfig {
  switch (provider) {
    case "openai":
      return { ...cfg, openai_api_key: value };
    case "anthropic":
      return { ...cfg, anthropic_api_key: value };
    case "openrouter":
      return { ...cfg, openrouter_api_key: value };
    case "groq":
      return { ...cfg, groq_api_key: value };
    case "gemini":
      return { ...cfg, gemini_api_key: value };
    case "xai":
      return { ...cfg, xai_api_key: value };
  }
}

export type SettingsTab =
  | "audio"
  | "hotkeys"
  | "transcription"
  | "llm"
  | "overlay"
  | "usage"
  | "updates";

function getUsageBadgeClass(percent: number): "safe" | "warning" | "danger" {
  if (percent >= 85) return "danger";
  if (percent >= 60) return "warning";
  return "safe";
}

interface SettingsProps {
  onBack: () => void;
  updater: ReturnType<typeof useUpdater>;
  initialTab?: SettingsTab;
}

export default function Settings({ onBack, updater, initialTab = "audio" }: SettingsProps) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  // Controla se o campo de texto "modelo personalizado" aparece. Não dá pra
  // derivar isso só comparando `llm_model` com a lista curada: ao escolher
  // "Personalizado…" nós semeamos o campo com o modelo default do provider
  // (que É um valor curado), então a comparação de string sozinha escondia
  // o campo de novo. Esse state guarda a intenção explícita do usuário.
  const [customModelMode, setCustomModelMode] = useState(false);
  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab);
  const [hardware, setHardware] = useState<HardwareReport | null>(null);
  const [benchmarking, setBenchmarking] = useState(false);
  const [benchmarkResult, setBenchmarkResult] = useState<BenchmarkResult | null>(null);
  const [benchmarkError, setBenchmarkError] = useState<string | null>(null);
  const [usageProvider, setUsageProvider] = useState<"groq" | "openai">("groq");
  const [usageReport, setUsageReport] = useState<UsageReport | null>(null);
  const [loadingUsage, setLoadingUsage] = useState(false);
  const [gpuStatus, setGpuStatus] = useState<GpuRuntimeStatus | null>(null);
  const [gpuProgress, setGpuProgress] = useState<GpuRuntimeProgress | null>(null);
  const [gpuError, setGpuError] = useState<string | null>(null);

  const refreshGpuStatus = () => {
    invoke<GpuRuntimeStatus>("get_gpu_runtime_status")
      .then(setGpuStatus)
      .catch((e) => console.error("Falha ao obter status GPU:", e));
  };

  useEffect(() => {
    setActiveTab(initialTab);
  }, [initialTab]);

  useEffect(() => {
    invoke<AppConfig>("get_config")
      .then((cfg) => {
        setConfig(cfg);
        setCustomModelMode(
          cfg.llm_model.trim() !== "" &&
            !CURATED_MODELS[cfg.provider].some((m) => m.id === cfg.llm_model),
        );
      })
      .catch((e) => setMessage(`Erro ao carregar config: ${e}`));
    invoke<boolean>("is_autostart_enabled")
      .then(setAutostartState)
      .catch(() => setAutostartState(false));
    invoke<HardwareReport>("get_hardware_info")
      .then(setHardware)
      .catch((e) => console.error("Falha ao detectar hardware:", e));

    refreshGpuStatus();

    const unlistens: UnlistenFn[] = [];
    const track = (p: Promise<UnlistenFn>) => p.then((fn) => unlistens.push(fn));

    track(
      listen<GpuRuntimeProgress>("gpu-runtime-download-progress", (e) => {
        setGpuProgress(e.payload);
      })
    );
    track(
      listen<void>("gpu-runtime-download-complete", () => {
        setGpuProgress(null);
        setGpuError(null);
        refreshGpuStatus();
      })
    );
    track(
      listen<{ error: string }>("gpu-runtime-download-error", (e) => {
        setGpuProgress(null);
        setGpuError(e.payload.error);
        refreshGpuStatus();
      })
    );

    return () => unlistens.forEach((fn) => fn());
  }, []);

  const handleDownloadGpu = () => {
    setGpuError(null);
    invoke("download_gpu_runtime").catch((err) => setGpuError(String(err)));
  };

  const handleDeleteGpu = async () => {
    if (window.confirm("Deseja desinstalar o módulo NVIDIA CUDA para liberar espaço no disco?")) {
      try {
        await invoke("delete_gpu_runtime");
        refreshGpuStatus();
      } catch (err) {
        alert(`Falha ao apagar módulo GPU: ${err}`);
      }
    }
  };

  const loadUsage = (provider: string) => {
    setLoadingUsage(true);
    invoke<UsageReport>("get_api_usage", { provider })
      .then(setUsageReport)
      .catch((e) => console.error("Erro ao carregar métricas de consumo:", e))
      .finally(() => setLoadingUsage(false));
  };

  useEffect(() => {
    if (activeTab === "usage") {
      loadUsage(usageProvider);
    }
  }, [activeTab, usageProvider]);

  async function handleClearUsage() {
    if (window.confirm("Deseja realmente zerar o histórico de consumo local deste aplicativo?")) {
      try {
        await invoke("clear_api_usage");
        loadUsage(usageProvider);
      } catch (e) {
        console.error("Falha ao zerar consumo:", e);
      }
    }
  }

  async function handleRunBenchmark() {
    setBenchmarking(true);
    setBenchmarkError(null);
    setBenchmarkResult(null);
    try {
      const res = await invoke<BenchmarkResult>("run_benchmark", {
        device: config?.inference_device || "auto",
      });
      setBenchmarkResult(res);
    } catch (err: any) {
      setBenchmarkError(String(err));
    } finally {
      setBenchmarking(false);
    }
  }

  async function toggleAutostart(enable: boolean) {
    // Optimistic — se der erro, revertemos.
    setAutostartState(enable);
    try {
      await invoke("set_autostart", { enable });
    } catch (e) {
      setAutostartState(!enable);
      setMessage(`Falha ao ${enable ? "ativar" : "desativar"} autostart: ${e}`);
    }
  }

  if (!config) {
    return (
      <div className="settings">
        <p>Carregando…</p>
      </div>
    );
  }

  const providerLabel = PROVIDER_LABELS[config.provider];
  const curated = CURATED_MODELS[config.provider];

  async function save() {
    if (!config) return;
    setSaving(true);
    setMessage(null);
    try {
      await invoke("save_config", { newConfig: config });
      setMessage("Configurações salvas.");
    } catch (e) {
      setMessage(`Erro ao salvar: ${e}`);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="settings">
      <header className="settings-header">
        <div className="settings-header-left">
          <button className="btn-link" onClick={onBack}>
            ← Voltar
          </button>
          <h2>Configurações</h2>
        </div>
        <div className="settings-header-actions">
          {message && (
            <span
              className={`settings-saved-badge ${message.startsWith("Erro") ? "settings-badge-error" : ""}`}
            >
              {message}
            </span>
          )}
          <button className="btn-primary" onClick={save} disabled={saving}>
            {saving ? "Salvando…" : "Salvar"}
          </button>
        </div>
      </header>

      <nav className="settings-tabs" aria-label="Abas de configurações">
        <button
          type="button"
          className={`settings-tab-btn ${activeTab === "audio" ? "active" : ""}`}
          onClick={() => setActiveTab("audio")}
        >
          <span className="tab-icon">🎙️</span> Áudio & Sistema
        </button>
        <button
          type="button"
          className={`settings-tab-btn ${activeTab === "hotkeys" ? "active" : ""}`}
          onClick={() => setActiveTab("hotkeys")}
        >
          <span className="tab-icon">⌨️</span> Atalhos
        </button>
        <button
          type="button"
          className={`settings-tab-btn ${activeTab === "transcription" ? "active" : ""}`}
          onClick={() => setActiveTab("transcription")}
        >
          <span className="tab-icon">🗣️</span> Transcrição
        </button>
        <button
          type="button"
          className={`settings-tab-btn ${activeTab === "llm" ? "active" : ""}`}
          onClick={() => setActiveTab("llm")}
        >
          <span className="tab-icon">✨</span> IA & Tradução
        </button>
        <button
          type="button"
          className={`settings-tab-btn ${activeTab === "overlay" ? "active" : ""}`}
          onClick={() => setActiveTab("overlay")}
        >
          <span className="tab-icon">🪟</span> Barra Flutuante
        </button>
        <button
          type="button"
          className={`settings-tab-btn ${activeTab === "usage" ? "active" : ""}`}
          onClick={() => setActiveTab("usage")}
        >
          <span className="tab-icon">📊</span> Consumo & Limites
          {usageReport?.is_near_limit && <span className="tab-badge-dot" />}
        </button>
        <button
          type="button"
          className={`settings-tab-btn ${activeTab === "updates" ? "active" : ""}`}
          onClick={() => setActiveTab("updates")}
        >
          <span className="tab-icon">🚀</span> Atualizações
        </button>
      </nav>

      {activeTab === "audio" && (
        <div className="settings-tab-content">
          <div className="settings-card">
            <h3 className="card-title">🎙️ Entrada de Áudio & Microfone</h3>
            <MicrophonePicker
              selected={config.microphone}
              onSelect={(name) => setConfig({ ...config, microphone: name })}
            />

            <section className="field" style={{ marginTop: "1.25rem" }}>
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={config.mute_audio_while_recording}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      mute_audio_while_recording: e.target.checked,
                    })
                  }
                />
                <span>Silenciar áudio do sistema ao ditar</span>
              </label>
              <p className="field-hint">
                Muta automaticamente o som do Windows (músicas, vídeos ou chamadas)
                enquanto você estiver gravando para evitar que ruídos externos
                atrapalhem a transcrição. O som volta ao normal no instante em que
                você solta a tecla ou conclui o ditado.
              </p>
            </section>

            <section className="field" style={{ marginTop: "1rem" }}>
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={config.sound_feedback}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      sound_feedback: e.target.checked,
                    })
                  }
                />
                <span>Beep sonoro ao começar e terminar</span>
              </label>
              <p className="field-hint">
                Toca um bipe curto ao iniciar a gravação e outro quando o texto
                termina de ser colado — feedback útil quando o app está no tray.{" "}
                <button
                  type="button"
                  className="btn-link"
                  onClick={() => {
                    playBeep("start");
                    setTimeout(() => playBeep("end"), 400);
                  }}
                >
                  testar sons
                </button>
              </p>
            </section>
          </div>

          <div className="settings-card">
            <h3 className="card-title">🖥️ Inicialização do Sistema</h3>
            <section className="field">
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={autostart ?? false}
                  disabled={autostart === null}
                  onChange={(e) => toggleAutostart(e.target.checked)}
                />
                <span>Iniciar automaticamente com o Windows</span>
              </label>
              <p className="field-hint">
                O app sobe direto pro tray no login — o atalho fica disponível sem
                você precisar abrir manualmente. Essa opção não passa pelo botão
                Salvar: já vale ao clicar.
              </p>
            </section>

            <section className="field" style={{ marginTop: "1rem" }}>
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={config.start_minimized}
                  onChange={(e) =>
                    setConfig({ ...config, start_minimized: e.target.checked })
                  }
                />
                <span>Iniciar apenas na bandeja do sistema</span>
              </label>
              <p className="field-hint">
                Ao abrir o app (manualmente ou junto com o Windows), a janela
                principal fica escondida — só o ícone na bandeja aparece. O atalho
                de ditado continua funcionando normalmente.
              </p>
            </section>
          </div>
        </div>
      )}

      {activeTab === "hotkeys" && (
        <div className="settings-tab-content">
          <div className="settings-card">
            <h3 className="card-title">⌨️ Atalhos do Teclado</h3>
            <section className="field">
              <label className="field-label">Atalho global (push-to-talk)</label>
              <HotkeyCapture
                value={config.hotkey}
                onChange={(hk) => setConfig({ ...config, hotkey: hk })}
              />
              <p className="field-hint">
                Segure para gravar e solte para transcrever. Clique em{" "}
                <strong>Alterar</strong> e pressione a combinação desejada (ex:{" "}
                <code>F9</code>, <code>Ctrl+Shift+Space</code>, <code>Alt+Space</code>{" "}
                ou <code>Ctrl+Windows</code>).
              </p>
            </section>

            <section className="field" style={{ marginTop: "1.5rem" }}>
              <label className="field-label">Atalho hands-free (opcional)</label>
              <HotkeyCapture
                value={config.hands_free_hotkey}
                onChange={(hk) => setConfig({ ...config, hands_free_hotkey: hk })}
                placeholder="Desativado"
                onClear={() => setConfig({ ...config, hands_free_hotkey: "" })}
              />
              <p className="field-hint">
                Toque uma vez pra começar a gravar, toque de novo pra parar — sem
                precisar segurar. Útil pra ditados longos. Precisa ser diferente do
                atalho push-to-talk acima.
              </p>
            </section>

            <section className="field" style={{ marginTop: "1.5rem" }}>
              <label className="field-label">Atalho de recolar (opcional)</label>
              <HotkeyCapture
                value={config.repaste_hotkey}
                onChange={(hk) => setConfig({ ...config, repaste_hotkey: hk })}
                placeholder="Desativado"
                onClear={() => setConfig({ ...config, repaste_hotkey: "" })}
              />
              <p className="field-hint">
                Cola de novo a última transcrição no app ativo, sem precisar
                regravar.
              </p>
            </section>
          </div>
        </div>
      )}

      {activeTab === "transcription" && (
        <div className="settings-tab-content">
          <div className="settings-card">
            <h3 className="card-title">🗣️ Motor de Transcrição</h3>
            <section className="field">
              <label className="field-label">Provedor</label>
              <div className="toggle-group">
                <button
                  type="button"
                  className={`toggle-btn ${config.transcription_provider === "local" ? "active" : ""}`}
                  onClick={() =>
                    setConfig({ ...config, transcription_provider: "local" })
                  }
                >
                  Local (offline)
                </button>
                <button
                  type="button"
                  className={`toggle-btn ${config.transcription_provider === "openai_cloud" ? "active" : ""}`}
                  onClick={() =>
                    setConfig({
                      ...config,
                      transcription_provider: "openai_cloud",
                    })
                  }
                >
                  OpenAI (cloud)
                </button>
                <button
                  type="button"
                  className={`toggle-btn ${config.transcription_provider === "groq_cloud" ? "active" : ""}`}
                  onClick={() =>
                    setConfig({
                      ...config,
                      transcription_provider: "groq_cloud",
                    })
                  }
                >
                  Groq (cloud)
                </button>
              </div>
            </section>

            <section className="field" style={{ marginTop: "1.25rem" }}>
              <label className="field-label" htmlFor="transcription-language">
                Idioma da fala
              </label>
              <select
                id="transcription-language"
                className="text-input"
                value={config.transcription_language}
                onChange={(e) =>
                  setConfig({ ...config, transcription_language: e.target.value })
                }
              >
                <option value="">Detectar automaticamente</option>
                <option value="pt">Português</option>
                <option value="en">Inglês</option>
                <option value="es">Espanhol</option>
                <option value="fr">Francês</option>
                <option value="de">Alemão</option>
                <option value="it">Italiano</option>
                <option value="ja">Japonês</option>
              </select>
              <p className="field-hint">
                Defina o idioma para acelerar a transcrição e aumentar a precisão
                em ditados monolíngues.
              </p>
            </section>

            {config.transcription_provider === "local" && (
              <div style={{ marginTop: "1.5rem" }}>
                <ModelPicker
                  selected={config.whisper_model}
                  onSelect={(slug) => setConfig({ ...config, whisper_model: slug })}
                />
              </div>
            )}

            {config.transcription_provider === "local" && (
              <div className="settings-card" style={{ marginTop: "1.5rem" }}>
                <h3 className="card-title">⚡ Aceleração de Hardware & Inferência</h3>

                {hardware?.primary_gpu ? (
                  <div
                    className={`hardware-badge ${
                      hardware.primary_gpu.is_discrete ? "discrete" : "cpu"
                    }`}
                  >
                    <span className="hardware-icon">
                      {hardware.primary_gpu.is_discrete ? "🎮" : "💻"}
                    </span>
                    <div className="hardware-text">
                      <span className="hardware-title">
                        {hardware.primary_gpu.name} ({hardware.primary_gpu.vendor})
                      </span>
                      <span className="hardware-detail">
                        {hardware.primary_gpu.is_discrete
                          ? `${hardware.primary_gpu.vram_mb} MB VRAM dedicada • ${hardware.primary_gpu.recommendation}`
                          : `Gráficos integrados • ${hardware.cpu_cores} núcleos de CPU detectados`}
                      </span>
                    </div>
                  </div>
                ) : (
                  <div className="hardware-badge cpu">
                    <span className="hardware-icon">💻</span>
                    <div className="hardware-text">
                      <span className="hardware-title">Processador (CPU)</span>
                      <span className="hardware-detail">
                        {hardware?.cpu_cores ? `${hardware.cpu_cores} núcleos lógicos` : "Inferência via CPU"} • Instruções AVX ativas
                      </span>
                    </div>
                  </div>
                )}

                {gpuStatus?.is_nvidia_detected && (
                  <div className={`gpu-runtime-card ${!gpuStatus.installed ? "not-installed" : ""}`}>
                    <div className="gpu-runtime-header">
                      <div className="gpu-runtime-title">
                        {gpuStatus.installed ? "🎮 Módulo NVIDIA CUDA 12 Instalado" : "⚡ Aceleração por GPU NVIDIA Disponível"}
                      </div>
                      <div className="gpu-runtime-actions">
                        {gpuStatus.installed ? (
                          <button
                            type="button"
                            className="btn-secondary btn-small"
                            onClick={handleDeleteGpu}
                          >
                            Desinstalar módulo ({gpuStatus.size_mb} MB)
                          </button>
                        ) : (
                          <button
                            type="button"
                            className="btn-primary btn-small"
                            onClick={handleDownloadGpu}
                            disabled={!!gpuProgress}
                          >
                            {gpuProgress ? "Baixando e instalando…" : "⚡ Baixar Módulo NVIDIA CUDA (~670 MB)"}
                          </button>
                        )}
                      </div>
                    </div>
                    <div className="gpu-runtime-desc">
                      {gpuStatus.installed
                        ? "Aceleração real por hardware ativada. Transcrições locais e benchmarks utilizam diretamente os núcleos CUDA da sua placa NVIDIA."
                        : "Sua placa de vídeo NVIDIA suporta aceleração local ultrarrápida. Baixe o módulo dedicado do whisper.cpp para habilitar o processamento na GPU sem precisar instalar SDKs externos."}
                    </div>
                    {gpuProgress && (
                      <div style={{ marginTop: "0.5rem" }}>
                        <div className="model-progress-bar">
                          <div
                            className="model-progress-fill"
                            style={{
                              width:
                                gpuProgress.total > 0
                                  ? `${Math.min(100, (gpuProgress.downloaded / gpuProgress.total) * 100)}%`
                                  : "8%",
                            }}
                          />
                        </div>
                        <span className="field-hint" style={{ fontSize: "0.74rem", marginTop: "0.25rem", display: "block" }}>
                          {formatMB(gpuProgress.downloaded)} / {formatMB(gpuProgress.total)} ({gpuProgress.total > 0 ? ((gpuProgress.downloaded / gpuProgress.total) * 100).toFixed(0) : 0}%)
                        </span>
                      </div>
                    )}
                    {gpuError && (
                      <p className="field-hint" style={{ color: "#ef4444", marginTop: "0.4rem" }}>
                        ⚠️ {gpuError}
                      </p>
                    )}
                  </div>
                )}

                <section className="field">
                  <label className="field-label">Dispositivo de execução</label>
                  <div className="toggle-group">
                    <button
                      type="button"
                      className={`toggle-btn ${(config.inference_device || "auto") === "auto" ? "active" : ""}`}
                      onClick={() => setConfig({ ...config, inference_device: "auto" })}
                    >
                      Automático (Recomendado)
                    </button>
                    <button
                      type="button"
                      className={`toggle-btn ${config.inference_device === "gpu" ? "active" : ""}`}
                      onClick={() => setConfig({ ...config, inference_device: "gpu" })}
                    >
                      GPU
                    </button>
                    <button
                      type="button"
                      className={`toggle-btn ${config.inference_device === "cpu" ? "active" : ""}`}
                      onClick={() => setConfig({ ...config, inference_device: "cpu" })}
                    >
                      CPU
                    </button>
                  </div>
                  <p className="field-hint">
                    {(config.inference_device || "auto") === "auto"
                      ? gpuStatus?.installed
                        ? "Modo automático: Módulo NVIDIA CUDA ativo. Máxima velocidade com aceleração física na placa de vídeo."
                        : "Modo automático: Executando via processador (CPU com aceleração AVX). Baixe o módulo CUDA acima para ativar a GPU."
                      : config.inference_device === "gpu"
                      ? gpuStatus?.installed
                        ? "Modo GPU ativo: Execução forçada na placa NVIDIA via núcleos CUDA."
                        : "Modo GPU selecionado, mas o Módulo CUDA ainda não foi baixado (baixe acima para habilitar)."
                      : "Forçando execução puramente pelo processador (CPU com aceleração AVX)."}
                  </p>
                  {config.inference_device === "gpu" && !gpuStatus?.installed && (
                    <p className="field-hint" style={{ color: "#fbbf24", fontWeight: 500, marginTop: "0.25rem" }}>
                      ⚠️ O módulo NVIDIA CUDA ainda não está instalado. Baixe o módulo no botão acima para usar a GPU.
                    </p>
                  )}
                </section>

                <div className="benchmark-box">
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: "0.5rem" }}>
                    <div>
                      <div style={{ fontWeight: 600, fontSize: "0.85rem" }}>⏱️ Medição de Desempenho Local</div>
                      <div style={{ fontSize: "0.75rem", color: "rgba(255,255,255,0.6)", marginTop: "0.15rem" }}>
                        Mede a velocidade real de transcrição no seu computador.
                      </div>
                    </div>
                    <button
                      type="button"
                      className="btn-secondary"
                      onClick={handleRunBenchmark}
                      disabled={benchmarking}
                      style={{ fontSize: "0.8rem", padding: "0.35rem 0.85rem", whiteSpace: "nowrap" }}
                    >
                      {benchmarking ? "Medindo…" : "Testar Desempenho"}
                    </button>
                  </div>

                  {benchmarkError && (
                    <p className="field-hint" style={{ color: "#ef4444", marginTop: "0.6rem" }}>
                      ⚠️ {benchmarkError}
                    </p>
                  )}

                  {benchmarkResult && (
                    <div className="benchmark-metrics">
                      <div className="benchmark-metric-item">
                        <div className={`benchmark-metric-val ${benchmarkResult.device_used === "GPU" ? "gpu" : ""}`}>
                          {benchmarkResult.duration_ms} ms
                        </div>
                        <div className="benchmark-metric-lbl">Tempo de inferência</div>
                      </div>
                      <div className="benchmark-metric-item">
                        <div className={`benchmark-metric-val ${benchmarkResult.device_used === "GPU" ? "gpu" : ""}`}>
                          {benchmarkResult.speedup_factor.toFixed(1)}x
                        </div>
                        <div className="benchmark-metric-lbl">Velocidade real-time</div>
                      </div>
                      <div className="benchmark-metric-item">
                        <div className={`benchmark-metric-val ${benchmarkResult.device_used === "GPU" ? "gpu" : ""}`}>
                          {benchmarkResult.device_used}
                        </div>
                        <div className="benchmark-metric-lbl">Dispositivo ativo</div>
                      </div>
                    </div>
                  )}
                </div>
              </div>
            )}

            {config.transcription_provider === "openai_cloud" &&
              config.provider !== "openai" && (
                <section className="field" style={{ marginTop: "1.25rem" }}>
                  <label className="field-label" htmlFor="openai-transcribe-key">
                    Chave da API OpenAI (para transcrição)
                  </label>
                  <input
                    id="openai-transcribe-key"
                    type="password"
                    className="text-input"
                    placeholder="sk-..."
                    value={config.openai_api_key}
                    onChange={(e) =>
                      setConfig({ ...config, openai_api_key: e.target.value })
                    }
                  />
                  <p className="field-hint">
                    O áudio é enviado para a API do OpenAI Whisper (<code>whisper-1</code>).
                  </p>
                </section>
              )}

            {config.transcription_provider === "groq_cloud" &&
              config.provider !== "groq" && (
                <section className="field" style={{ marginTop: "1.25rem" }}>
                  <label className="field-label" htmlFor="groq-transcribe-key">
                    Chave da API do Groq (para transcrição)
                  </label>
                  <input
                    id="groq-transcribe-key"
                    type="password"
                    className="text-input"
                    placeholder="gsk_..."
                    value={config.groq_api_key}
                    onChange={(e) =>
                      setConfig({ ...config, groq_api_key: e.target.value })
                    }
                  />
                  <p className="field-hint">
                    O áudio é enviado para a API do Groq (modelo{" "}
                    <code>whisper-large-v3-turbo</code>). Pegue a sua em{" "}
                    <a
                      href="https://console.groq.com/keys"
                      target="_blank"
                      rel="noreferrer"
                    >
                      console.groq.com/keys
                    </a>.
                  </p>
                </section>
              )}
          </div>
        </div>
      )}

      {activeTab === "llm" && (
        <div className="settings-tab-content">
          <div className="settings-card">
            <h3 className="card-title">✨ Pós-processamento com IA</h3>
            <section className="field">
              <label className="field-label" htmlFor="provider">
                Provedor de LLM
              </label>
              <select
                id="provider"
                className="text-input"
                value={config.provider}
                onChange={(e) => {
                  const newProvider = e.target.value as Provider;
                  setConfig({ ...config, provider: newProvider, llm_model: "" });
                  setCustomModelMode(false);
                }}
              >
                {(Object.keys(PROVIDER_LABELS) as Provider[]).map((p) => (
                  <option key={p} value={p}>
                    {PROVIDER_LABELS[p]}
                  </option>
                ))}
              </select>
            </section>

            <section className="field" style={{ marginTop: "1.25rem" }}>
              <label className="field-label" htmlFor="apikey">
                Chave da API ({providerLabel})
              </label>
              <input
                id="apikey"
                type="password"
                className="text-input"
                placeholder={API_KEY_PLACEHOLDER[config.provider]}
                value={getApiKey(config, config.provider)}
                onChange={(e) =>
                  setConfig(setApiKey(config, config.provider, e.target.value))
                }
              />
              <p className="field-hint">
                Deixe em branco para desativar (o app cola só a transcrição bruta).
                Pegue sua chave em{" "}
                <a
                  href={API_KEY_HELP_URL[config.provider]}
                  target="_blank"
                  rel="noreferrer"
                >
                  {API_KEY_HELP_URL[config.provider]}
                </a>.
              </p>
            </section>

            <section className="field" style={{ marginTop: "1.25rem" }}>
              <label className="field-label" htmlFor="model">
                Modelo
              </label>
              <select
                id="model"
                className="text-input"
                value={customModelMode ? "__custom" : config.llm_model}
                onChange={(e) => {
                  const v = e.target.value;
                  if (v === "__custom") {
                    setCustomModelMode(true);
                    setConfig({
                      ...config,
                      llm_model: DEFAULT_MODEL_OF(config.provider),
                    });
                  } else {
                    setCustomModelMode(false);
                    setConfig({ ...config, llm_model: v });
                  }
                }}
              >
                <option value="">
                  Padrão ({DEFAULT_MODEL_OF(config.provider)})
                </option>
                {curated.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.label}
                  </option>
                ))}
                <option value="__custom">Personalizado…</option>
              </select>
              {customModelMode && (
                <input
                  type="text"
                  className="text-input"
                  style={{ marginTop: "0.5rem" }}
                  placeholder={DEFAULT_MODEL_OF(config.provider)}
                  value={config.llm_model}
                  onChange={(e) =>
                    setConfig({ ...config, llm_model: e.target.value })
                  }
                />
              )}
            </section>

            <section className="field" style={{ marginTop: "1.25rem" }}>
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={config.adapt_prompt_to_active_app}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      adapt_prompt_to_active_app: e.target.checked,
                    })
                  }
                />
                <span>Adaptar reformatação ao app em foco</span>
              </label>
              <p className="field-hint">
                Detecta o app ativo onde o texto será colado (Slack, Word, VS Code, etc.)
                para ajustar o tom e a pontuação automaticamente.
              </p>
            </section>

            <section className="field" style={{ marginTop: "1rem" }}>
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={config.skip_llm_formatting}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      skip_llm_formatting: e.target.checked,
                    })
                  }
                />
                <span>Não reformatar — colar a transcrição bruta</span>
              </label>
              <p className="field-hint">
                Pula a chamada de LLM e cola exatamente o que foi transcrito.
              </p>
            </section>
          </div>

          <div className="settings-card">
            <h3 className="card-title">🌐 Tradução Automática</h3>
            <section className="field">
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={config.translate.enabled}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      translate: {
                        ...config.translate,
                        enabled: e.target.checked,
                      },
                    })
                  }
                />
                <span>Traduzir automaticamente</span>
              </label>
              {config.translate.enabled && config.skip_llm_formatting && (
                <p className="field-hint">
                  Sem efeito enquanto "Não reformatar" estiver marcado.
                </p>
              )}
              {config.translate.enabled && (
                <div className="translate-target" style={{ marginTop: "0.75rem" }}>
                  <label className="field-label" htmlFor="lang">
                    Idioma alvo (código ISO)
                  </label>
                  <input
                    id="lang"
                    type="text"
                    className="text-input text-input-small"
                    placeholder="en"
                    value={config.translate.target_language}
                    onChange={(e) =>
                      setConfig({
                        ...config,
                        translate: {
                          ...config.translate,
                          target_language: e.target.value,
                        },
                      })
                    }
                  />
                  <p className="field-hint">
                    Ex: <code>en</code>, <code>es</code>, <code>fr</code>,{" "}
                    <code>de</code>, <code>ja</code>.
                  </p>
                </div>
              )}
            </section>
          </div>
        </div>
      )}

      {activeTab === "overlay" && (
        <div className="settings-tab-content">
          <div className="settings-card">
            <h3 className="card-title">🪟 Indicador Visual & Barra Flutuante</h3>
            <section className="field">
              <label className="field-label" htmlFor="visual">
                Indicador visual durante a gravação
              </label>
              <select
                id="visual"
                className="text-input"
                value={config.visual_indicator}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    visual_indicator: e.target.value as VisualIndicator,
                  })
                }
              >
                <option value="both">Barra flutuante + ícone da bandeja</option>
                <option value="floating">Só barra flutuante</option>
                <option value="tray">Só ícone da bandeja</option>
                <option value="none">Desativado</option>
              </select>
            </section>

            {(config.visual_indicator === "floating" ||
              config.visual_indicator === "both") && (
              <section className="field" style={{ marginTop: "1.25rem" }}>
                <label className="field-label">Personalização da barra</label>

                <div className="overlay-custom-row" style={{ marginTop: "0.75rem" }}>
                  <label className="field-label" htmlFor="overlay-position">
                    Posição
                  </label>
                  <select
                    id="overlay-position"
                    className="text-input"
                    value={config.overlay.position}
                    onChange={(e) =>
                      setConfig({
                        ...config,
                        overlay: {
                          ...config.overlay,
                          position: e.target.value as OverlayPosition,
                        },
                      })
                    }
                  >
                    <option value="bottom">Fundo da tela</option>
                    <option value="top">Topo da tela</option>
                  </select>
                </div>

                <div className="overlay-custom-row" style={{ marginTop: "1rem" }}>
                  <label className="field-label" htmlFor="overlay-scale">
                    Tamanho ({Math.round(config.overlay.scale * 100)}%)
                  </label>
                  <input
                    id="overlay-scale"
                    type="range"
                    min={0.75}
                    max={1.75}
                    step={0.05}
                    value={config.overlay.scale}
                    onChange={(e) =>
                      setConfig({
                        ...config,
                        overlay: {
                          ...config.overlay,
                          scale: Number(e.target.value),
                        },
                      })
                    }
                  />
                </div>

                <div className="overlay-custom-row" style={{ marginTop: "1rem" }}>
                  <label className="field-label" htmlFor="overlay-opacity">
                    Opacidade ({Math.round(config.overlay.opacity * 100)}%)
                  </label>
                  <input
                    id="overlay-opacity"
                    type="range"
                    min={0.3}
                    max={1}
                    step={0.05}
                    value={config.overlay.opacity}
                    onChange={(e) =>
                      setConfig({
                        ...config,
                        overlay: {
                          ...config.overlay,
                          opacity: Number(e.target.value),
                        },
                      })
                    }
                  />
                </div>

                <div className="overlay-custom-row" style={{ marginTop: "1rem" }}>
                  <label className="field-label" htmlFor="overlay-accent">
                    Cor de destaque
                  </label>
                  <input
                    id="overlay-accent"
                    type="color"
                    className="overlay-color-input"
                    value={config.overlay.accent_color}
                    onChange={(e) =>
                      setConfig({
                        ...config,
                        overlay: {
                          ...config.overlay,
                          accent_color: e.target.value,
                        },
                      })
                    }
                  />
                </div>
              </section>
            )}
          </div>
        </div>
      )}

      {activeTab === "usage" && (
        <div className="settings-tab-content">
          <div className="settings-card">
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "1rem" }}>
              <h3 className="card-title" style={{ margin: 0 }}>📊 Consumo & Limites de API</h3>
              <div style={{ display: "flex", gap: "0.5rem" }}>
                <button
                  type="button"
                  className="btn-secondary"
                  style={{ fontSize: "0.78rem", padding: "0.3rem 0.65rem" }}
                  onClick={() => loadUsage(usageProvider)}
                  disabled={loadingUsage}
                >
                  {loadingUsage ? "Atualizando…" : "🔄 Atualizar"}
                </button>
                <button
                  type="button"
                  className="btn-secondary"
                  style={{ fontSize: "0.78rem", padding: "0.3rem 0.65rem", color: "#f87171" }}
                  onClick={handleClearUsage}
                >
                  🗑️ Zerar
                </button>
              </div>
            </div>

            <section className="field">
              <label className="field-label">Provedor para visualização</label>
              <div className="toggle-group">
                <button
                  type="button"
                  className={`toggle-btn ${usageProvider === "groq" ? "active" : ""}`}
                  onClick={() => setUsageProvider("groq")}
                >
                  Groq (Cloud)
                </button>
                <button
                  type="button"
                  className={`toggle-btn ${usageProvider === "openai" ? "active" : ""}`}
                  onClick={() => setUsageProvider("openai")}
                >
                  OpenAI (Cloud)
                </button>
              </div>
              <p className="field-hint">
                Acompanhe o consumo acumulado no aplicativo e a proximidade em relação aos limites de requisições, áudio e tokens.
              </p>
            </section>

            {usageReport?.alert_message && (
              <div className="usage-alert-box" style={{ marginTop: "1rem" }}>
                <span style={{ fontSize: "1.1rem" }}>⚠️</span>
                <span>{usageReport.alert_message}</span>
              </div>
            )}
          </div>

          {/* Speech-to-Text Card */}
          <div className="settings-card">
            <h3 className="card-title">🎙️ Speech to Text (Transcrição de Áudio)</h3>
            <p className="field-hint" style={{ marginTop: "-0.25rem", marginBottom: "1.25rem" }}>
              {usageProvider === "groq"
                ? "Limites base do plano Groq Free: 7.2k seg/hora (2h), 28.8k seg/dia (8h), 20 RPM e 2.000 RPD."
                : "Limites de áudio para a API Whisper da OpenAI."}
            </p>

            <div className="usage-grid">
              {/* Audio Seconds per Hour */}
              <div className="usage-meter-card">
                <div className="usage-meter-header">
                  <span className="usage-meter-title">Segundos de Áudio / Hora</span>
                  <span className={`usage-meter-badge ${getUsageBadgeClass(usageReport?.stt_audio_seconds_hour.percent || 0)}`}>
                    {(usageReport?.stt_audio_seconds_hour.percent || 0).toFixed(1)}%
                  </span>
                </div>
                <div className="usage-progress-bar">
                  <div
                    className={`usage-progress-fill ${getUsageBadgeClass(usageReport?.stt_audio_seconds_hour.percent || 0)}`}
                    style={{ width: `${Math.min(100, Math.max(0, usageReport?.stt_audio_seconds_hour.percent || 0))}%` }}
                  />
                </div>
                <div className="usage-meter-footer">
                  <span>{(usageReport?.stt_audio_seconds_hour.current || 0).toFixed(0)}s gastos</span>
                  <span>Limite: {(usageReport?.stt_audio_seconds_hour.limit || 7200).toFixed(0)}s</span>
                </div>
              </div>

              {/* Audio Seconds per Day */}
              <div className="usage-meter-card">
                <div className="usage-meter-header">
                  <span className="usage-meter-title">Segundos de Áudio / Dia</span>
                  <span className={`usage-meter-badge ${getUsageBadgeClass(usageReport?.stt_audio_seconds_day.percent || 0)}`}>
                    {(usageReport?.stt_audio_seconds_day.percent || 0).toFixed(1)}%
                  </span>
                </div>
                <div className="usage-progress-bar">
                  <div
                    className={`usage-progress-fill ${getUsageBadgeClass(usageReport?.stt_audio_seconds_day.percent || 0)}`}
                    style={{ width: `${Math.min(100, Math.max(0, usageReport?.stt_audio_seconds_day.percent || 0))}%` }}
                  />
                </div>
                <div className="usage-meter-footer">
                  <span>{(usageReport?.stt_audio_seconds_day.current || 0).toFixed(0)}s gastos</span>
                  <span>Limite: {(usageReport?.stt_audio_seconds_day.limit || 28800).toFixed(0)}s</span>
                </div>
              </div>

              {/* Requests per Minute (RPM) */}
              <div className="usage-meter-card">
                <div className="usage-meter-header">
                  <span className="usage-meter-title">Requests / Minuto (RPM)</span>
                  <span className={`usage-meter-badge ${getUsageBadgeClass(usageReport?.stt_requests_minute.percent || 0)}`}>
                    {(usageReport?.stt_requests_minute.percent || 0).toFixed(0)}%
                  </span>
                </div>
                <div className="usage-progress-bar">
                  <div
                    className={`usage-progress-fill ${getUsageBadgeClass(usageReport?.stt_requests_minute.percent || 0)}`}
                    style={{ width: `${Math.min(100, Math.max(0, usageReport?.stt_requests_minute.percent || 0))}%` }}
                  />
                </div>
                <div className="usage-meter-footer">
                  <span>{usageReport?.stt_requests_minute.current || 0} no último minuto</span>
                  <span>Limite: {usageReport?.stt_requests_minute.limit || 20} RPM</span>
                </div>
              </div>

              {/* Requests per Day (RPD) */}
              <div className="usage-meter-card">
                <div className="usage-meter-header">
                  <span className="usage-meter-title">Requests / Dia (RPD)</span>
                  <span className={`usage-meter-badge ${getUsageBadgeClass(usageReport?.stt_requests_day.percent || 0)}`}>
                    {(usageReport?.stt_requests_day.percent || 0).toFixed(1)}%
                  </span>
                </div>
                <div className="usage-progress-bar">
                  <div
                    className={`usage-progress-fill ${getUsageBadgeClass(usageReport?.stt_requests_day.percent || 0)}`}
                    style={{ width: `${Math.min(100, Math.max(0, usageReport?.stt_requests_day.percent || 0))}%` }}
                  />
                </div>
                <div className="usage-meter-footer">
                  <span>{usageReport?.stt_requests_day.current || 0} reqs hoje</span>
                  <span>Limite: {usageReport?.stt_requests_day.limit || 2000} RPD</span>
                </div>
              </div>
            </div>
          </div>

          {/* Chat Completions / LLM Card */}
          <div className="settings-card">
            <h3 className="card-title">✨ Chat Completions (Formatação & LLM)</h3>
            <p className="field-hint" style={{ marginTop: "-0.25rem", marginBottom: "1.25rem" }}>
              {usageProvider === "groq"
                ? "Limites base do plano Groq Free: 30 RPM, 1.000 RPD, 30k TPM e 200k TPD."
                : "Limites de tokens e requisições para a API de chat da OpenAI."}
            </p>

            <div className="usage-grid">
              {/* Tokens per Minute (TPM) */}
              <div className="usage-meter-card">
                <div className="usage-meter-header">
                  <span className="usage-meter-title">Tokens / Minuto (TPM)</span>
                  <span className={`usage-meter-badge ${getUsageBadgeClass(usageReport?.llm_tokens_minute.percent || 0)}`}>
                    {(usageReport?.llm_tokens_minute.percent || 0).toFixed(1)}%
                  </span>
                </div>
                <div className="usage-progress-bar">
                  <div
                    className={`usage-progress-fill ${getUsageBadgeClass(usageReport?.llm_tokens_minute.percent || 0)}`}
                    style={{ width: `${Math.min(100, Math.max(0, usageReport?.llm_tokens_minute.percent || 0))}%` }}
                  />
                </div>
                <div className="usage-meter-footer">
                  <span>{usageReport?.llm_tokens_minute.current || 0} tokens</span>
                  <span>Limite: {usageReport?.llm_tokens_minute.limit || 30000} TPM</span>
                </div>
              </div>

              {/* Tokens per Day (TPD) */}
              <div className="usage-meter-card">
                <div className="usage-meter-header">
                  <span className="usage-meter-title">Tokens / Dia (TPD)</span>
                  <span className={`usage-meter-badge ${getUsageBadgeClass(usageReport?.llm_tokens_day.percent || 0)}`}>
                    {(usageReport?.llm_tokens_day.percent || 0).toFixed(1)}%
                  </span>
                </div>
                <div className="usage-progress-bar">
                  <div
                    className={`usage-progress-fill ${getUsageBadgeClass(usageReport?.llm_tokens_day.percent || 0)}`}
                    style={{ width: `${Math.min(100, Math.max(0, usageReport?.llm_tokens_day.percent || 0))}%` }}
                  />
                </div>
                <div className="usage-meter-footer">
                  <span>{usageReport?.llm_tokens_day.current || 0} tokens hoje</span>
                  <span>Limite: {usageReport?.llm_tokens_day.limit || 200000} TPD</span>
                </div>
              </div>

              {/* LLM Requests per Minute (RPM) */}
              <div className="usage-meter-card">
                <div className="usage-meter-header">
                  <span className="usage-meter-title">Requests / Minuto (RPM)</span>
                  <span className={`usage-meter-badge ${getUsageBadgeClass(usageReport?.llm_requests_minute.percent || 0)}`}>
                    {(usageReport?.llm_requests_minute.percent || 0).toFixed(0)}%
                  </span>
                </div>
                <div className="usage-progress-bar">
                  <div
                    className={`usage-progress-fill ${getUsageBadgeClass(usageReport?.llm_requests_minute.percent || 0)}`}
                    style={{ width: `${Math.min(100, Math.max(0, usageReport?.llm_requests_minute.percent || 0))}%` }}
                  />
                </div>
                <div className="usage-meter-footer">
                  <span>{usageReport?.llm_requests_minute.current || 0} no último minuto</span>
                  <span>Limite: {usageReport?.llm_requests_minute.limit || 30} RPM</span>
                </div>
              </div>

              {/* LLM Requests per Day (RPD) */}
              <div className="usage-meter-card">
                <div className="usage-meter-header">
                  <span className="usage-meter-title">Requests / Dia (RPD)</span>
                  <span className={`usage-meter-badge ${getUsageBadgeClass(usageReport?.llm_requests_day.percent || 0)}`}>
                    {(usageReport?.llm_requests_day.percent || 0).toFixed(1)}%
                  </span>
                </div>
                <div className="usage-progress-bar">
                  <div
                    className={`usage-progress-fill ${getUsageBadgeClass(usageReport?.llm_requests_day.percent || 0)}`}
                    style={{ width: `${Math.min(100, Math.max(0, usageReport?.llm_requests_day.percent || 0))}%` }}
                  />
                </div>
                <div className="usage-meter-footer">
                  <span>{usageReport?.llm_requests_day.current || 0} reqs hoje</span>
                  <span>Limite: {usageReport?.llm_requests_day.limit || 1000} RPD</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {activeTab === "updates" && (
        <div className="settings-tab-content">
          <div className="settings-card">
            <h3 className="card-title">🚀 Atualizações & Informações</h3>
            <UpdateSection updater={updater} />

            <div style={{ marginTop: "1.5rem", display: "flex", gap: "0.75rem", flexWrap: "wrap" }}>
              <button
                type="button"
                className="btn-secondary"
                onClick={() => invoke("open_config_folder")}
              >
                📂 Abrir pasta do config
              </button>
            </div>
          </div>
        </div>
      )}

      <footer className="settings-footer-actions">
        <button className="btn-primary btn-large" onClick={save} disabled={saving}>
          {saving ? "Salvando…" : "Salvar Configurações"}
        </button>
      </footer>
    </div>
  );
}

// ---------- UpdateSection ----------

interface UpdateSectionProps {
  updater: ReturnType<typeof useUpdater>;
}

/** Verificação e instalação de atualizações. O check silencioso já roda no
 *  boot (App.tsx); aqui só expomos "Verificar agora" e o botão de instalar
 *  quando há algo disponível, reusando o mesmo state (não dispara um
 *  segundo check independente). */
function UpdateSection({ updater }: UpdateSectionProps) {
  const { info, checkNow, installNow } = updater;

  return (
    <section className="field">
      <label className="field-label">Atualizações</label>
      <p className="field-hint">
        Repositório no GitHub:{" "}
        <a href={GITHUB_REPO_URL} target="_blank" rel="noreferrer">
          {GITHUB_REPO_URL}
        </a>
      </p>

      {info.status === "idle" && (
        <p className="field-hint">Nenhuma verificação feita ainda.</p>
      )}
      {info.status === "checking" && (
        <p className="field-hint">Verificando…</p>
      )}
      {info.status === "up_to_date" && (
        <p className="field-hint">Você já está na versão mais recente.</p>
      )}
      {info.status === "error" && (
        <p className="model-error">Falha ao verificar: {info.error}</p>
      )}
      {info.status === "available" && (
        <div className="model-row model-row-selected">
          <div className="model-info">
            <span className="model-name">Versão {info.version} disponível</span>
            {info.notes && <span className="model-status">{info.notes}</span>}
          </div>
          <div className="model-actions">
            <button
              type="button"
              className="btn-primary btn-small"
              onClick={installNow}
            >
              Atualizar e reiniciar
            </button>
          </div>
        </div>
      )}
      {info.status === "downloading" && (
        <div className="model-row model-row-selected">
          <div className="model-info">
            <span className="model-name">Baixando atualização…</span>
            <span className="model-status">
              {info.total
                ? `${formatMB(info.downloaded)} / ${formatMB(info.total)}`
                : "iniciando…"}
            </span>
          </div>
          <div className="model-progress-bar">
            <div
              className="model-progress-fill"
              style={{
                width: info.total
                  ? `${Math.min(100, (info.downloaded / info.total) * 100)}%`
                  : "8%",
              }}
            />
          </div>
        </div>
      )}
      {info.status === "installing" && (
        <p className="field-hint">Instalando e reiniciando…</p>
      )}

      {(info.status === "idle" ||
        info.status === "up_to_date" ||
        info.status === "error") && (
        <button
          type="button"
          className="btn-secondary btn-small"
          style={{ marginTop: "0.5rem", alignSelf: "flex-start" }}
          onClick={() => checkNow()}
        >
          Verificar agora
        </button>
      )}
    </section>
  );
}

// ---------- ModelPicker ----------

interface ModelPickerProps {
  selected: WhisperModelSlug;
  onSelect: (slug: WhisperModelSlug) => void;
}

/** Payload dos eventos emitidos pelo Rust em `models.rs::spawn_download`. */
interface DownloadProgress {
  name: WhisperModelSlug;
  downloaded: number;
  total: number;
}
interface DownloadComplete {
  name: WhisperModelSlug;
}
interface DownloadError {
  name: WhisperModelSlug;
  error: string;
}

/** Lista os 5 modelos disponíveis mostrando qual está baixado, com botões
 *  de baixar/apagar e barra de progresso durante o download. */
function ModelPicker({ selected, onSelect }: ModelPickerProps) {
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>(
    {},
  );
  const [errors, setErrors] = useState<Record<string, string>>({});

  // Recarrega a lista do Rust (usado no mount e após qualquer mudança).
  const refresh = () => {
    invoke<ModelStatus[]>("list_whisper_models")
      .then(setModels)
      .catch((e) => console.error("list_whisper_models falhou:", e));
  };

  useEffect(() => {
    refresh();
    const unlistens: UnlistenFn[] = [];
    const track = (p: Promise<UnlistenFn>) => p.then((fn) => unlistens.push(fn));

    track(
      listen<DownloadProgress>("model-download-progress", (e) => {
        setProgress((p) => ({ ...p, [e.payload.name]: e.payload }));
      }),
    );
    track(
      listen<DownloadComplete>("model-download-complete", (e) => {
        setProgress((p) => {
          const { [e.payload.name]: _drop, ...rest } = p;
          return rest;
        });
        setErrors((er) => {
          const { [e.payload.name]: _drop, ...rest } = er;
          return rest;
        });
        refresh();
      }),
    );
    track(
      listen<DownloadError>("model-download-error", (e) => {
        setProgress((p) => {
          const { [e.payload.name]: _drop, ...rest } = p;
          return rest;
        });
        setErrors((er) => ({ ...er, [e.payload.name]: e.payload.error }));
      }),
    );

    return () => unlistens.forEach((fn) => fn());
  }, []);

  const startDownload = (slug: WhisperModelSlug) => {
    setErrors((er) => {
      const { [slug]: _drop, ...rest } = er;
      return rest;
    });
    // Coloca 0/0 imediatamente pra UI mostrar "Iniciando…" sem esperar o
    // primeiro evento chegar.
    setProgress((p) => ({
      ...p,
      [slug]: { name: slug, downloaded: 0, total: 0 },
    }));
    invoke("download_whisper_model", { name: slug }).catch((e) => {
      setProgress((p) => {
        const { [slug]: _drop, ...rest } = p;
        return rest;
      });
      setErrors((er) => ({ ...er, [slug]: String(e) }));
    });
  };

  const deleteModel = async (slug: WhisperModelSlug) => {
    if (!confirm("Apagar o arquivo desse modelo?")) return;
    try {
      await invoke("delete_whisper_model", { name: slug });
      refresh();
    } catch (e) {
      alert(`Falha ao apagar: ${e}`);
    }
  };

  return (
    <div className="model-picker">
      {models.map((m) => {
        const prog = progress[m.slug];
        const err = errors[m.slug];
        const isSelected = selected === m.slug;
        return (
          <div
            key={m.slug}
            className={`model-row ${isSelected ? "model-row-selected" : ""}`}
          >
            <label className="model-row-main">
              <input
                type="radio"
                name="whisper_model"
                checked={isSelected}
                onChange={() => onSelect(m.slug)}
                disabled={!m.downloaded}
              />
              <div className="model-info">
                <span className="model-name">{m.display_name}</span>
                <span className="model-status">
                  {m.downloaded
                    ? `${formatMB(m.bytes_on_disk)} no disco`
                    : prog
                      ? formatProgress(prog)
                      : `${m.size_mb}MB a baixar`}
                </span>
              </div>
            </label>

            <div className="model-actions">
              {m.downloaded ? (
                <button
                  type="button"
                  className="btn-secondary btn-small"
                  onClick={() => deleteModel(m.slug)}
                >
                  Apagar
                </button>
              ) : prog ? (
                <button
                  type="button"
                  className="btn-secondary btn-small"
                  disabled
                >
                  Baixando…
                </button>
              ) : (
                <button
                  type="button"
                  className="btn-secondary btn-small"
                  onClick={() => startDownload(m.slug)}
                >
                  Baixar
                </button>
              )}
            </div>

            {prog && (
              <div className="model-progress-bar">
                <div
                  className="model-progress-fill"
                  style={{
                    width:
                      prog.total > 0
                        ? `${Math.min(100, (prog.downloaded / prog.total) * 100)}%`
                        : "8%",
                  }}
                />
              </div>
            )}

            {err && <p className="model-error">{err}</p>}
          </div>
        );
      })}
      <p className="field-hint">
        O modelo selecionado é o que será usado nas transcrições. Só dá pra
        selecionar depois de baixar.
      </p>
    </div>
  );
}

function formatMB(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)}KB`;
  return `${(bytes / 1024 / 1024).toFixed(0)}MB`;
}

function formatProgress(p: DownloadProgress): string {
  if (p.total === 0) return "iniciando…";
  const pct = Math.min(100, (p.downloaded / p.total) * 100);
  return `${formatMB(p.downloaded)} / ${formatMB(p.total)} (${pct.toFixed(0)}%)`;
}

// ---------- MicrophonePicker ----------

interface MicrophonePickerProps {
  selected: string;
  onSelect: (deviceName: string) => void;
}

/** Dropdown de escolha de microfone. Lista vazia = mostra só "Padrão do
 *  sistema" (o backend já usa o device default do SO quando `selected` é
 *  vazio, então a UI não bloqueia nesse caso). */
function MicrophonePicker({ selected, onSelect }: MicrophonePickerProps) {
  const [devices, setDevices] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => {
    setLoading(true);
    invoke<string[]>("list_microphones")
      .then((list) => {
        setDevices(list);
        setError(null);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  };

  useEffect(refresh, []);

  return (
    <section className="field">
      <label className="field-label" htmlFor="microphone">
        Microfone
      </label>
      <div className="hotkey-capture">
        <select
          id="microphone"
          className="text-input"
          value={selected}
          onChange={(e) => onSelect(e.target.value)}
        >
          <option value="">Padrão do sistema</option>
          {devices.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>
        <button
          type="button"
          className="btn-secondary"
          onClick={refresh}
          disabled={loading}
          title="Atualizar lista de microfones"
        >
          {loading ? "…" : "↻"}
        </button>
      </div>
      {error && <p className="model-error">{error}</p>}
      <p className="field-hint">
        Escolha qual microfone usar pra gravar. Se o dispositivo salvo for
        desconectado, a gravação falha com um erro pedindo pra trocar aqui.
      </p>
    </section>
  );
}

// ---------- HotkeyCapture ----------

interface HotkeyCaptureProps {
  value: string;
  onChange: (hotkey: string) => void;
  /** Texto mostrado quando `value` está vazio (ex: "Desativado"). */
  placeholder?: string;
  /** Se fornecido, mostra um botão "Desativar" quando há um valor definido. */
  onClear?: () => void;
}

/**
 * Input "leitura + botão de captura" para escolher a combinação de tecla.
 *
 * No modo capture, escuta `keydown` na window inteira e monta uma string no
 * formato aceito pelo `Shortcut::from_str` do plugin — ex: `Ctrl+Shift+K`,
 * `Alt+Space`, `F9`. Só finaliza quando o usuário aperta uma tecla
 * não-modificadora (aí temos algo válido para registrar).
 */
function HotkeyCapture({ value, onChange, placeholder, onClear }: HotkeyCaptureProps) {
  const [capturing, setCapturing] = useState(false);
  // Ref para o handler ficar estável entre renders — o addEventListener/remove
  // precisa da mesma referência.
  const handlerRef = useRef<((e: KeyboardEvent) => void) | null>(null);

  useEffect(() => {
    if (!capturing) return;

    // Pausa o atalho global no Rust ANTES de começar a escutar. O
    // preventDefault do DOM não bloqueia atalhos globais registrados no SO —
    // se o atalho atual for F9 e o usuário apertar F9 pra trocar, o app
    // começa a gravar sem essa pausa.
    invoke("pause_hotkey").catch((err) =>
      console.error("falha ao pausar atalho:", err)
    );

    // Modificadores mantidos pressionados durante esta captura. Usado pra
    // permitir combinações só de modificador (ex: "Ctrl+Super", que vira
    // "Ctrl+Windows" na prática) — o Windows não registra esse tipo de
    // combinação via atalho global comum, mas o backend tem um caminho
    // separado pra isso (ver `modkey.rs`), então a UI precisa saber montá-la.
    const heldMods = new Set<string>();

    const onKeyDown = (e: KeyboardEvent) => {
      // Bloqueia default e propagação — impede que a tecla vá pra outros
      // handlers da própria UI (ex: um form submit).
      e.preventDefault();
      e.stopPropagation();

      // Escape aborta a captura sem mudar nada.
      if (e.code === "Escape") {
        setCapturing(false);
        return;
      }

      const modName = MOD_CODE_TO_NAME[e.code];
      if (modName) {
        heldMods.add(modName);
        return; // espera outro modificador ou uma tecla final
      }

      // Tecla não-modificadora: combina com os modificadores atualmente
      // pressionados e finaliza imediatamente.
      const parsed = formatHotkey(e);
      if (parsed) {
        onChange(parsed);
        setCapturing(false);
      }
    };

    const onKeyUp = (e: KeyboardEvent) => {
      const modName = MOD_CODE_TO_NAME[e.code];
      if (!modName) return;

      // Já tínhamos 2+ modificadores simultâneos: a combinação que estava
      // pressionada até agora (incluindo o que acabou de soltar) é o alvo.
      if (heldMods.size >= 2) {
        onChange(formatModifierCombo(heldMods));
        setCapturing(false);
        return;
      }

      // Só esse modificador estava pressionado (sem outro par) — não conta
      // como combinação sozinha, continua esperando.
      heldMods.delete(modName);
    };

    handlerRef.current = onKeyDown;
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      handlerRef.current = null;
      // Restaura o atalho ao valor salvo no config (o novo só entra em vigor
      // depois do usuário clicar em Salvar).
      invoke("resume_hotkey").catch((err) =>
        console.error("falha ao restaurar atalho:", err)
      );
    };
  }, [capturing, onChange]);

  return (
    <div className="hotkey-capture">
      <input
        type="text"
        className="text-input hotkey-display"
        readOnly
        value={
          capturing
            ? "Pressione a combinação… (Esc cancela)"
            : displayHotkey(value) || placeholder || ""
        }
      />
      <button
        type="button"
        className="btn-secondary"
        onClick={() => setCapturing((v) => !v)}
      >
        {capturing ? "Cancelar" : "Alterar"}
      </button>
      {onClear && value && !capturing && (
        <button type="button" className="btn-secondary" onClick={onClear}>
          Desativar
        </button>
      )}
    </div>
  );
}

/** Mapeia o `event.code` de cada tecla modificadora (esquerda/direita) para
 *  o nome canônico usado nas strings de atalho. */
const MOD_CODE_TO_NAME: Record<string, string> = {
  ControlLeft: "Ctrl",
  ControlRight: "Ctrl",
  ShiftLeft: "Shift",
  ShiftRight: "Shift",
  AltLeft: "Alt",
  AltRight: "Alt",
  MetaLeft: "Super",
  MetaRight: "Super",
  OSLeft: "Super",
  OSRight: "Super",
};

/** Ordem canônica dos modificadores numa string de atalho — mesma ordem que
 *  `formatHotkey` já usa (Ctrl, Shift, Alt, Super) e que o backend entende
 *  tanto pra combinações padrão quanto pras só-de-modificador. */
const MOD_ORDER = ["Ctrl", "Shift", "Alt", "Super"];

/** Monta a string de uma combinação só de modificadores (ex: "Ctrl+Super"
 *  para "Ctrl+Windows"), na ordem canônica. */
function formatModifierCombo(mods: Set<string>): string {
  return MOD_ORDER.filter((m) => mods.has(m)).join("+");
}

/** Converte um KeyboardEvent numa string aceita pelo `Shortcut::from_str`.
 *  Retorna null se for só um modificador (ex: só Shift). */
function formatHotkey(e: KeyboardEvent): string | null {
  // Só-modificador: espera próxima tecla.
  if (MOD_CODE_TO_NAME[e.code]) return null;

  const key = codeToShortcutKey(e.code);
  if (!key) return null;

  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");
  if (e.metaKey) parts.push("Super");
  parts.push(key);
  return parts.join("+");
}

/** Mapeia `event.code` (código físico da tecla, independente de layout) para
 *  o nome que o parser de accelerator do Tauri aceita. */
function codeToShortcutKey(code: string): string | null {
  // KeyA..KeyZ -> A..Z
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  // Digit0..Digit9 -> 0..9
  if (/^Digit\d$/.test(code)) return code.slice(5);
  // F1..F24
  if (/^F\d{1,2}$/.test(code)) return code;
  // Numpad
  if (/^Numpad\d$/.test(code)) return "Num" + code.slice(6);

  const map: Record<string, string> = {
    Space: "Space",
    Enter: "Enter",
    NumpadEnter: "Enter",
    Escape: "Escape",
    Backspace: "Backspace",
    Tab: "Tab",
    CapsLock: "CapsLock",
    Insert: "Insert",
    Delete: "Delete",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
    Minus: "-",
    Equal: "=",
    Comma: ",",
    Period: ".",
    Slash: "/",
    Backslash: "\\",
    Semicolon: ";",
    Quote: "'",
    BracketLeft: "[",
    BracketRight: "]",
    Backquote: "`",
    PrintScreen: "PrintScreen",
    ScrollLock: "ScrollLock",
    Pause: "Pause",
  };
  return map[code] ?? null;
}
