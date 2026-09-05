import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import Settings, { type SettingsTab } from "./Settings";
import History from "./History";
import { DEFAULT_HOTKEY, displayHotkey } from "./hotkeyFormat";
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
  const [copyMsg, setCopyMsg] = useState<string | null>(null);
  const updater = useUpdater();

  useEffect(() => {
    invoke<string>("get_app_version").then(setAppVersion).catch(() => {});
  }, []);

  // Checagem silenciosa de atualização no boot — se houver versão nova, o
  // banner abaixo do header aparece sozinho; se não houver (ou der erro,
  // ex: sem internet), nada é mostrado. O botão "Verificar agora" em
  // Configurações usa o mesmo hook para uma checagem explícita.
  useEffect(() => {
    updater.checkNow({ silent: true });
  }, []);

  // Relê o config sempre que a tela principal volta a ficar visível — cobre
  // tanto o boot quanto o retorno da tela de Configurações depois de salvar
  // um atalho novo.
  useEffect(() => {
    if (view !== "main") return;
    invoke<{ hotkey: string }>("get_config")
      .then((cfg) => setHotkey(cfg.hotkey.trim() || DEFAULT_HOTKEY))
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
      setCopyMsg("Copiado.");
    } catch (err) {
      setCopyMsg(`Falha ao copiar: ${err}`);
    } finally {
      setTimeout(() => setCopyMsg(null), 2000);
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
            🕓
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
            ⚙
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
              ⧉
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
