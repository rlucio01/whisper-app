import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Tipo espelhando `AppConfig` no Rust (config.rs).
type Provider = "openai" | "anthropic";
type VisualIndicator = "none" | "floating" | "tray" | "both";
type TranscriptionProvider = "local" | "openai_cloud";
type WhisperModelSlug = "tiny" | "base" | "small" | "medium" | "large_turbo";

interface AppConfig {
  provider: Provider;
  openai_api_key: string;
  anthropic_api_key: string;
  llm_model: string;
  translate: {
    enabled: boolean;
    target_language: string;
  };
  visual_indicator: VisualIndicator;
  hotkey: string;
  transcription_provider: TranscriptionProvider;
  whisper_model: WhisperModelSlug;
  adapt_prompt_to_active_app: boolean;
}

interface ModelStatus {
  slug: WhisperModelSlug;
  display_name: string;
  size_mb: number;
  downloaded: boolean;
  bytes_on_disk: number;
}

const DEFAULT_MODELS: Record<Provider, string> = {
  openai: "gpt-4o-mini",
  anthropic: "claude-haiku-4-5-20251001",
};

interface SettingsProps {
  onBack: () => void;
}

export default function Settings({ onBack }: SettingsProps) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [autostart, setAutostartState] = useState<boolean | null>(null);

  useEffect(() => {
    invoke<AppConfig>("get_config")
      .then(setConfig)
      .catch((e) => setMessage(`Erro ao carregar config: ${e}`));
    invoke<boolean>("is_autostart_enabled")
      .then(setAutostartState)
      .catch(() => setAutostartState(false));
  }, []);

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

  const activeKey =
    config.provider === "openai" ? "openai_api_key" : "anthropic_api_key";

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
        <button className="btn-link" onClick={onBack}>
          ← Voltar
        </button>
        <h2>Configurações</h2>
      </header>

      <section className="field">
        <label className="field-label">Provedor de LLM</label>
        <div className="toggle-group">
          <button
            className={`toggle-btn ${config.provider === "openai" ? "active" : ""}`}
            onClick={() => setConfig({ ...config, provider: "openai" })}
          >
            OpenAI
          </button>
          <button
            className={`toggle-btn ${config.provider === "anthropic" ? "active" : ""}`}
            onClick={() => setConfig({ ...config, provider: "anthropic" })}
          >
            Anthropic
          </button>
        </div>
      </section>

      <section className="field">
        <label className="field-label" htmlFor="apikey">
          Chave da API ({config.provider})
        </label>
        <input
          id="apikey"
          type="password"
          className="text-input"
          placeholder={
            config.provider === "openai"
              ? "sk-... ou sk-proj-..."
              : "sk-ant-..."
          }
          value={config[activeKey]}
          onChange={(e) =>
            setConfig({ ...config, [activeKey]: e.target.value })
          }
        />
        <p className="field-hint">
          Deixe em branco para desativar (o app mostra só a transcrição bruta,
          sem formatação).
        </p>
      </section>

      <section className="field">
        <label className="field-label" htmlFor="model">
          Modelo (opcional)
        </label>
        <input
          id="model"
          type="text"
          className="text-input"
          placeholder={DEFAULT_MODELS[config.provider]}
          value={config.llm_model}
          onChange={(e) =>
            setConfig({ ...config, llm_model: e.target.value })
          }
        />
        <p className="field-hint">
          Vazio = default ({DEFAULT_MODELS[config.provider]}).
        </p>
      </section>

      <section className="field">
        <label className="field-label">Transcrição</label>
        <div className="toggle-group">
          <button
            className={`toggle-btn ${config.transcription_provider === "local" ? "active" : ""}`}
            onClick={() =>
              setConfig({ ...config, transcription_provider: "local" })
            }
          >
            Local (offline)
          </button>
          <button
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
        </div>
        {config.transcription_provider === "local" ? (
          <ModelPicker
            selected={config.whisper_model}
            onSelect={(slug) => setConfig({ ...config, whisper_model: slug })}
          />
        ) : (
          <p className="field-hint">
            O áudio é enviado para a API da OpenAI (modelo <code>whisper-1</code>
            ). Usa a chave configurada acima. Nada fica no seu disco, mas
            precisa de internet.
          </p>
        )}
      </section>

      <section className="field">
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
          Ao pressionar o atalho, o Whisper App detecta o app onde você vai
          colar (Slack, Outlook, VS Code, etc.) e passa isso como contexto pro
          LLM — o tom da reformatação fica mais adequado. Nenhum dado do app é
          enviado pra fora além do nome do executável e do título da janela.
        </p>
      </section>

      <section className="field">
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={config.translate.enabled}
            onChange={(e) =>
              setConfig({
                ...config,
                translate: { ...config.translate, enabled: e.target.checked },
              })
            }
          />
          <span>Traduzir automaticamente</span>
        </label>
        {config.translate.enabled && (
          <div className="translate-target">
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

      <section className="field">
        <label className="field-label">Atalho global (push-to-talk)</label>
        <HotkeyCapture
          value={config.hotkey}
          onChange={(hk) => setConfig({ ...config, hotkey: hk })}
        />
        <p className="field-hint">
          Clique em <strong>Alterar</strong> e pressione a combinação desejada
          (ex: <code>F9</code>, <code>Ctrl+Shift+Space</code>,{" "}
          <code>Alt+Space</code>). Só passa a valer depois de{" "}
          <strong>Salvar</strong>.
        </p>
      </section>

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
        <p className="field-hint">
          A barra flutuante fica sempre no topo, útil quando outros apps cobrem
          a janela. O ícone da bandeja muda de cor durante o pipeline.
        </p>
      </section>

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

      {message && <p className="settings-msg">{message}</p>}

      <section className="actions">
        <button className="btn-primary" onClick={save} disabled={saving}>
          {saving ? "Salvando…" : "Salvar"}
        </button>
        <button
          className="btn-secondary"
          onClick={() => invoke("open_config_folder")}
        >
          Abrir pasta do config
        </button>
      </section>
    </div>
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

// ---------- HotkeyCapture ----------

interface HotkeyCaptureProps {
  value: string;
  onChange: (hotkey: string) => void;
}

/**
 * Input "leitura + botão de captura" para escolher a combinação de tecla.
 *
 * No modo capture, escuta `keydown` na window inteira e monta uma string no
 * formato aceito pelo `Shortcut::from_str` do plugin — ex: `Ctrl+Shift+K`,
 * `Alt+Space`, `F9`. Só finaliza quando o usuário aperta uma tecla
 * não-modificadora (aí temos algo válido para registrar).
 */
function HotkeyCapture({ value, onChange }: HotkeyCaptureProps) {
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

      const parsed = formatHotkey(e);
      if (parsed) {
        onChange(parsed);
        setCapturing(false);
      }
      // Se retornou null (só modificador), continua esperando a próxima tecla.
    };

    handlerRef.current = onKeyDown;
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
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
        value={capturing ? "Pressione a combinação… (Esc cancela)" : value}
      />
      <button
        type="button"
        className="btn-secondary"
        onClick={() => setCapturing((v) => !v)}
      >
        {capturing ? "Cancelar" : "Alterar"}
      </button>
    </div>
  );
}

/** Converte um KeyboardEvent numa string aceita pelo `Shortcut::from_str`.
 *  Retorna null se for só um modificador (ex: só Shift). */
function formatHotkey(e: KeyboardEvent): string | null {
  // Só-modificador: espera próxima tecla.
  const modOnly = new Set([
    "ControlLeft",
    "ControlRight",
    "ShiftLeft",
    "ShiftRight",
    "AltLeft",
    "AltRight",
    "MetaLeft",
    "MetaRight",
    "OSLeft",
    "OSRight",
  ]);
  if (modOnly.has(e.code)) return null;

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
