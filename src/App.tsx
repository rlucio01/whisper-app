import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import Settings, { type SettingsTab } from "./Settings";
import History from "./History";
import {
  DEFAULT_HOTKEY,
  displayHotkey,
  isHotkeyMatch,
  MOD_CODE_TO_NAME,
} from "./hotkeyFormat";
import { playBeep, type SoundKind } from "./sound";
import { useUpdater } from "./useUpdater";
import "./App.css";

type Status =
  | "idle"
  | "recording"
  | "loading_model"
  | "transcribing"
  | "formatting"
  | "complete"
  | "error";

type View = "main" | "settings" | "history";

function App() {
  const [view, setView] = useState<View>("main");
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("audio");
  const [status, setStatus] = useState<Status>("idle");
  const [rawTranscript, setRawTranscript] = useState<string | null>(null);
  const [formattedText, setFormattedText] = useState<string | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [hotkey, setHotkey] = useState(DEFAULT_HOTKEY);
  const [handsFreeHotkey, setHandsFreeHotkey] = useState("");
  const [copyMsg, setCopyMsg] = useState<string | null>(null);
  const [testText, setTestText] = useState("");
  const [testCopyMsg, setTestCopyMsg] = useState<string | null>(null);
  const isPttActiveLocalRef = useRef(false);
  const heldModsRef = useRef(new Set<string>());
  const statusRef = useRef(status);
  statusRef.current = status;
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const updater = useUpdater();

  useEffect(() => {
    invoke<string>("get_app_version").then(setAppVersion).catch(() => {});
  }, []);

  useEffect(() => {
    updater.checkNow({ silent: true });
  }, []);

  // Relê o config sempre que a tela principal volta a ficar visível
  useEffect(() => {
    if (view !== "main") return;
    invoke<{ hotkey: string; hands_free_hotkey?: string }>("get_config")
      .then((cfg) => {
        setHotkey(cfg.hotkey.trim() || DEFAULT_HOTKEY);
        setHandsFreeHotkey(
          cfg.hands_free_hotkey ? cfg.hands_free_hotkey.trim() : "",
        );
      })
      .catch(() => {});
  }, [view]);

  useEffect(() => {
    const unlistenFns: UnlistenFn[] = [];
    const track = (p: Promise<UnlistenFn>) =>
      p.then((fn) => unlistenFns.push(fn));

    track(
      listen("open-updates-tab", () => {
        setSettingsTab("updates");
        setView("settings");
      })
    );

    track(
      listen("hotkey-pressed", () => {
        setStatus("recording");
        setErrorMsg(null);
        setFormattedText(null);
      })
    );
    track(
      listen<string>("transcription-status", (e) => {
        if (e.payload === "carregando_modelo") setStatus("loading_model");
        else if (e.payload === "transcrevendo") setStatus("transcribing");
      })
    );
    track(
      listen<string>("transcription-complete", (e) => {
        setRawTranscript(e.payload);
      })
    );
    track(listen("formatting-started", () => setStatus("formatting")));
    track(
      listen<string>("format-complete", (e) => {
        setStatus("complete");
        setFormattedText(e.payload);
      })
    );
    track(
      listen<string>("insert-error", (e) => {
        setErrorMsg(`Falha ao colar no app ativo: ${e.payload}`);
      })
    );
    track(
      listen<string>("recording-error", (e) => {
        setStatus("error");
        setErrorMsg(e.payload);
      })
    );
    track(
      listen<string>("transcription-error", (e) => {
        setStatus("error");
        setErrorMsg(e.payload);
      })
    );
    track(
      listen<string>("format-error", (e) => {
        setStatus("error");
        setErrorMsg(e.payload);
      })
    );

    // Feedback sonoro — o Rust só emite se `sound_feedback` estiver ligado
    // no config, então aqui basta tocar quando o evento chega.
    track(
      listen<SoundKind>("play-sound", (e) => {
        playBeep(e.payload);
      })
    );

    return () => unlistenFns.forEach((fn) => fn());
  }, []);

  // Intercepta atalhos de gravação localmente quando o app ou a área de teste está em foco
  useEffect(() => {
    if (view !== "main") return;

    const onKeyDown = (e: KeyboardEvent) => {
      const mod = MOD_CODE_TO_NAME[e.code];
      if (mod) heldModsRef.current.add(mod);

      // 1. Hands-free toggle
      if (handsFreeHotkey && isHotkeyMatch(e, handsFreeHotkey, heldModsRef.current)) {
        e.preventDefault();
        e.stopPropagation();
        invoke("toggle_recording").catch(console.error);
        return;
      }

      // 2. Push-to-talk (segurar para gravar)
      if (isHotkeyMatch(e, hotkey, heldModsRef.current)) {
        e.preventDefault();
        e.stopPropagation();
        if (!isPttActiveLocalRef.current && statusRef.current !== "recording") {
          isPttActiveLocalRef.current = true;
          invoke("start_recording").catch(console.error);
        }
      }
    };

    const onKeyUp = (e: KeyboardEvent) => {
      const mod = MOD_CODE_TO_NAME[e.code];

      // Se soltar tecla do PTT enquanto ativo localmente, finaliza gravação
      if (isPttActiveLocalRef.current) {
        isPttActiveLocalRef.current = false;
        invoke("stop_recording").catch(console.error);
      }

      if (mod) heldModsRef.current.delete(mod);
    };

    const onBlur = () => {
      if (isPttActiveLocalRef.current) {
        isPttActiveLocalRef.current = false;
        invoke("stop_recording").catch(console.error);
      }
      heldModsRef.current.clear();
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", onBlur);

    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [view, hotkey, handsFreeHotkey]);

  const statusLabel: Record<Status, string> = {
    idle: "Aguardando atalho",
    recording: "Gravando…",
    loading_model: "Carregando modelo (primeira vez, ~10s)…",
    transcribing: "Transcrevendo…",
    formatting: "Reformatando via LLM…",
    complete: "Pronto",
    error: "Erro",
  };

  const showRawSeparately =
    rawTranscript && formattedText && rawTranscript !== formattedText;

  async function copyResult() {
    if (!formattedText) return;
    try {
      await navigator.clipboard.writeText(formattedText);
      setCopyMsg("Copiado");
    } catch (err) {
      setCopyMsg(`Falha ao copiar: ${err}`);
    } finally {
      setTimeout(() => setCopyMsg(null), 2000);
    }
  }

  async function copyTestText() {
    if (!testText) return;
    try {
      await navigator.clipboard.writeText(testText);
      setTestCopyMsg("Copiado");
    } catch (err) {
      setTestCopyMsg(`Falha ao copiar: ${err}`);
    } finally {
      setTimeout(() => setTestCopyMsg(null), 1800);
    }
  }

  if (view === "settings") {
    return (
      <main className="container">
        <Settings
          onBack={() => setView("main")}
          updater={updater}
          initialTab={settingsTab}
        />
      </main>
    );
  }

  if (view === "history") {
    return (
      <main className="container">
        <History onBack={() => setView("main")} />
      </main>
    );
  }

  return (
    <main className="container">
      <header className="app-header">
        <div>
          <h1>
            Whisper App
            {appVersion && <span className="app-version">v{appVersion}</span>}
          </h1>
          <p className="tagline">Ditado por voz com IA</p>
        </div>
        <div className="app-header-actions">
          <button
            className="icon-btn"
            onClick={() => setView("history")}
            title="Histórico"
            aria-label="Histórico"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="10" />
              <polyline points="12 6 12 12 16 14" />
            </svg>
          </button>
          <button
            className="icon-btn"
            onClick={() => {
              setSettingsTab("audio");
              setView("settings");
            }}
            title="Configurações"
            aria-label="Configurações"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
              <circle cx="12" cy="12" r="3" />
            </svg>
          </button>
        </div>
      </header>

      {(updater.info.status === "available" ||
        updater.info.status === "downloading" ||
        updater.info.status === "installing") && (
        <section className="update-banner">
          {updater.info.status === "available" && (
            <>
              <span className="update-banner-text">
                Nova versão disponível: v{updater.info.version}
              </span>
              <button
                className="btn-primary btn-small"
                onClick={updater.installNow}
              >
                Atualizar
              </button>
            </>
          )}
          {updater.info.status === "downloading" && (
            <>
              <span className="update-banner-text">
                Baixando atualização
                {updater.info.total
                  ? ` (${Math.round(
                      (updater.info.downloaded / updater.info.total) * 100,
                    )}%)`
                  : "…"}
              </span>
              <div className="model-progress-bar update-banner-progress">
                <div
                  className="model-progress-fill"
                  style={{
                    width: updater.info.total
                      ? `${Math.min(100, (updater.info.downloaded / updater.info.total) * 100)}%`
                      : "8%",
                  }}
                />
              </div>
            </>
          )}
          {updater.info.status === "installing" && (
            <span className="update-banner-text">
              Instalando e reiniciando…
            </span>
          )}
        </section>
      )}

      <section className={`status status-${status}`}>
        <div className="status-dot" />
        <span>{statusLabel[status]}</span>
      </section>

      {formattedText && (
        <section className="transcript">
          <div className="transcript-header">
            <div style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
              <p className="label">Resultado:</p>
              {copyMsg && <span className="copy-badge">{copyMsg}</span>}
            </div>
            <button
              className="icon-btn"
              onClick={copyResult}
              title="Copiar texto"
              aria-label="Copiar texto"
            >
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
                <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
              </svg>
            </button>
          </div>
          <p className="transcript-text">{formattedText}</p>
        </section>
      )}

      {showRawSeparately && (
        <section className="transcript transcript-raw">
          <p className="label">Transcrição bruta:</p>
          <p className="transcript-text">{rawTranscript}</p>
        </section>
      )}

      {errorMsg && (
        <section className="error-box">
          <p className="label">Erro:</p>
          <pre>{errorMsg}</pre>
        </section>
      )}

      <section className="test-scratchpad-card">
        <div className="test-scratchpad-header">
          <div className="test-scratchpad-title-group">
            <span className="test-scratchpad-title">Área de Teste de Digitação</span>
            {testCopyMsg && <span className="copy-badge">{testCopyMsg}</span>}
          </div>
          <div className="test-scratchpad-actions">
            <button
              type="button"
              className={`btn-small ${status === "recording" ? "btn-danger" : "btn-secondary"}`}
              onClick={() => {
                textareaRef.current?.focus();
                if (status === "recording") {
                  invoke("stop_recording").catch(console.error);
                } else {
                  invoke("start_recording").catch(console.error);
                }
              }}
              title={status === "recording" ? "Parar gravação" : "Gravar voz para testar"}
            >
              {status === "recording" ? (
                <>
                  <span className="rec-dot-pulsing" /> Parar
                </>
              ) : (
                <>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" style={{ verticalAlign: "-2px", marginRight: "5px" }}>
                    <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
                    <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                    <line x1="12" y1="19" x2="12" y2="22" />
                  </svg>
                  Gravar voz
                </>
              )}
            </button>
            {testText && (
              <button
                type="button"
                className="btn-link btn-xs"
                onClick={() => setTestText("")}
              >
                Limpar
              </button>
            )}
            <button
              type="button"
              className="btn-secondary btn-xs"
              onClick={copyTestText}
              disabled={!testText}
            >
              Copiar
            </button>
          </div>
        </div>
        <textarea
          ref={textareaRef}
          className="test-scratchpad-textarea"
          rows={4}
          value={testText}
          onChange={(e) => setTestText(e.target.value)}
          placeholder="Clique aqui e use seu atalho de voz para ditar ou digite livremente para testar..."
        />
        <div className="test-scratchpad-footer">
          <span>
            {(() => {
              const trimmed = testText.trim();
              if (!trimmed) {
                return "Clique e fale usando seu atalho para ditar neste campo";
              }
              const words = trimmed.split(/\s+/).filter(Boolean).length;
              const chars = testText.length;
              const tokens = Math.max(words, Math.ceil(chars / 3.8));
              return `${words} ${words === 1 ? "palavra" : "palavras"} • ${chars} ${chars === 1 ? "caractere" : "caracteres"} • ~${tokens} tokens`;
            })()}
          </span>
        </div>
      </section>

      <section className="hint">
        <p>
          Segure <kbd>{displayHotkey(hotkey)}</kbd>, fale, e solte para
          transcrever.
        </p>
        <p className="hint-secondary">
          Fechar esta janela minimiza o app para a bandeja do sistema.
          Para sair de verdade, use o menu do ícone na bandeja.
        </p>
      </section>
    </main>
  );
}

export default App;
