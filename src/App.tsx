import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import Settings, { type SettingsTab } from "./Settings";
import History from "./History";
import AudioFileTranscribe from "./AudioFileTranscribe";
import MeetingsView from "./MeetingsView";
import {
  DEFAULT_HOTKEY,
  displayHotkey,
  isHotkeyMatch,
  MOD_CODE_TO_NAME,
} from "./hotkeyFormat";
import { playBeep, type SoundKind } from "./sound";
import { useUpdater } from "./useUpdater";
import OnboardingModal from "./OnboardingModal";
import "./App.css";

type Status =
  | "idle"
  | "recording"
  | "loading_model"
  | "transcribing"
  | "formatting"
  | "complete"
  | "error";

type View = "main" | "file_transcribe" | "meetings" | "settings" | "history";

function App() {
  const [view, setView] = useState<View>("main");
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("audio");
  const [status, setStatus] = useState<Status>("idle");
  const [rawTranscript, setRawTranscript] = useState<string | null>(null);
  const [formattedText, setFormattedText] = useState<string | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [hotkey, setHotkey] = useState(DEFAULT_HOTKEY);
  const [secondaryHotkey, setSecondaryHotkey] = useState("");
  const [translateToggleHotkey, setTranslateToggleHotkey] = useState("F10");
  const [translateEnabled, setTranslateEnabled] = useState(false);
  const [meetingInitialAudio, setMeetingInitialAudio] = useState<string | null>(null);
  const [handsFreeHotkey, setHandsFreeHotkey] = useState("");
  const [copyMsg, setCopyMsg] = useState<string | null>(null);
  const [testText, setTestText] = useState("");
  const [testCopyMsg, setTestCopyMsg] = useState<string | null>(null);
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [hasNoAiKey, setHasNoAiKey] = useState(false);
  const [dismissedPromo, setDismissedPromo] = useState(false);
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
    invoke<{
      hotkey: string;
      secondary_hotkey?: string;
      translate_toggle_hotkey?: string;
      hands_free_hotkey?: string;
      translate?: { enabled: boolean; target_language: string };
      onboarding_completed?: boolean;
      openai_api_key?: string;
      groq_api_key?: string;
      anthropic_api_key?: string;
      gemini_api_key?: string;
      openrouter_api_key?: string;
      xai_api_key?: string;
    }>("get_config")
      .then((cfg) => {
        setHotkey(cfg.hotkey.trim() || DEFAULT_HOTKEY);
        setSecondaryHotkey(cfg.secondary_hotkey ? cfg.secondary_hotkey.trim() : "");
        setTranslateToggleHotkey(cfg.translate_toggle_hotkey ? cfg.translate_toggle_hotkey.trim() : "F10");
        setHandsFreeHotkey(
          cfg.hands_free_hotkey ? cfg.hands_free_hotkey.trim() : "",
        );
        if (cfg.translate) {
          setTranslateEnabled(cfg.translate.enabled);
        }

        const hasAnyKey = Boolean(
          cfg.groq_api_key?.trim() ||
          cfg.openai_api_key?.trim() ||
          cfg.anthropic_api_key?.trim() ||
          cfg.gemini_api_key?.trim() ||
          cfg.openrouter_api_key?.trim() ||
          cfg.xai_api_key?.trim()
        );
        setHasNoAiKey(!hasAnyKey);

        // Se for primeiro acesso sem chave configurada nem onboarding concluído, abre o tutorial automaticamente
        if (!cfg.onboarding_completed && !hasAnyKey) {
          setShowOnboarding(true);
        }
      })
      .catch(() => {});
  }, [view]);

  useEffect(() => {
    const unlistenFns: UnlistenFn[] = [];
    const track = (p: Promise<UnlistenFn>) =>
      p.then((fn) => unlistenFns.push(fn));

    track(
      listen<boolean>("translation-toggled", (e) => {
        setTranslateEnabled(e.payload);
      })
    );

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

      // 0. Alternar tradução para inglês (ex: F10)
      if (isHotkeyMatch(e, translateToggleHotkey || "F10", heldModsRef.current)) {
        e.preventDefault();
        e.stopPropagation();
        invoke("toggle_translation").catch(console.error);
        return;
      }

      // 1. Hands-free toggle
      if (handsFreeHotkey && isHotkeyMatch(e, handsFreeHotkey, heldModsRef.current)) {
        e.preventDefault();
        e.stopPropagation();
        invoke("toggle_recording").catch(console.error);
        return;
      }

      // 2. Push-to-talk principal ou secundário (teclado)
      const matchesMainPtt = isHotkeyMatch(e, hotkey, heldModsRef.current);
      const matchesSecPtt =
        secondaryHotkey && isHotkeyMatch(e, secondaryHotkey, heldModsRef.current);

      if (matchesMainPtt || matchesSecPtt) {
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
  }, [view, hotkey, secondaryHotkey, handsFreeHotkey, translateToggleHotkey]);

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

  if (view === "file_transcribe") {
    return (
      <main className="container">
        <AudioFileTranscribe
          onBack={() => setView("main")}
          onSendToMeeting={(path) => {
            setMeetingInitialAudio(path);
            setView("meetings");
          }}
        />
      </main>
    );
  }

  if (view === "meetings") {
    return (
      <main className="container">
        <MeetingsView
          onBack={() => {
            setMeetingInitialAudio(null);
            setView("main");
          }}
          initialAudioPath={meetingInitialAudio}
        />
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
            className={`header-pill-btn ${translateEnabled ? "active" : ""}`}
            onClick={() => invoke("toggle_translation").catch(console.error)}
            title={`Alternar tradução para inglês (${displayHotkey(translateToggleHotkey || "F10")})`}
          >
            <span className="pill-dot" />
            <span className="pill-label">Tradução: {translateEnabled ? "EN" : "Off"}</span>
            <kbd className="pill-kbd">{translateToggleHotkey || "F10"}</kbd>
          </button>

          <button
            className="icon-btn"
            onClick={() => setView("file_transcribe")}
            title="Transcrever Arquivo de Áudio (MP3, OGG, WAV...)"
            aria-label="Transcrever Arquivo"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
              <polyline points="14 2 14 8 20 8" />
              <path d="M12 18v-6" />
              <path d="M9 15l3-3 3 3" />
            </svg>
          </button>

          <button
            className="icon-btn"
            onClick={() => setView("meetings")}
            title="Reuniões e Atas com Diarização por Orador"
            aria-label="Reuniões"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
              <circle cx="9" cy="7" r="4" />
              <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
              <path d="M16 3.13a4 4 0 0 1 0 7.75" />
            </svg>
          </button>

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
            onClick={() => setShowOnboarding(true)}
            title="Primeiros Passos / Tutorial de Configuração"
            aria-label="Primeiros Passos"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="10" />
              <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
              <line x1="12" y1="17" x2="12.01" y2="17" />
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

      {hasNoAiKey && !dismissedPromo && !showOnboarding && (
        <section className="onboarding-promo-banner" style={{ margin: "0.5rem 0" }}>
          <div className="promo-banner-text">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#60a5fa" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
              <path d="M12 2a10 10 0 1 0 10 10H12V2Z" />
              <path d="M12 12L2.05 12" />
            </svg>
            <span>
              <strong>Dica:</strong> Ative a pontuação automática com IA usando o plano <strong>100% gratuito do Groq</strong>.
            </span>
          </div>
          <div className="promo-banner-actions">
            <button
              type="button"
              className="btn-primary btn-small"
              onClick={() => setShowOnboarding(true)}
            >
              Configurar em 1 minuto
            </button>
            <button
              type="button"
              className="icon-btn"
              onClick={() => setDismissedPromo(true)}
              title="Dispensar aviso"
              aria-label="Dispensar"
              style={{ padding: "0.25rem" }}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>
        </section>
      )}

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
          Segure <kbd>{displayHotkey(hotkey)}</kbd>
          {secondaryHotkey ? (
            <> ou <kbd>{displayHotkey(secondaryHotkey)}</kbd></>
          ) : null}
          , fale, e solte para transcrever.
        </p>
        <p className="hint-secondary">
          Pressione <kbd>{displayHotkey(translateToggleHotkey || "F10")}</kbd> para ligar/desligar a tradução para inglês. Fechar esta janela minimiza o app para a bandeja do sistema.
        </p>
      </section>

      {showOnboarding && (
        <OnboardingModal
          isOpen={showOnboarding}
          onClose={() => setShowOnboarding(false)}
          onComplete={() => {
            setShowOnboarding(false);
            setDismissedPromo(true);
            setHasNoAiKey(false);
          }}
          currentHotkey={hotkey}
        />
      )}
    </main>
  );
}

export default App;
