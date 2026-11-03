import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface MeetingDiarizedTurn {
  speaker: string;
  timestamp: string;
  text: string;
}

export interface MeetingActionItem {
  speaker: string;
  task: string;
  deadline?: string | null;
  done: boolean;
}

export interface MeetingMinutes {
  id: string;
  title: string;
  created_at: number | string;
  duration_sec: number;
  executive_summary: string;
  key_decisions: string[];
  action_items: MeetingActionItem[];
  speakers: string[];
  diarized_transcript: MeetingDiarizedTurn[];
  raw_transcript: string;
}

export interface ParakeetStatus {
  installed: boolean;
  is_downloading: boolean;
  size_mb: number;
  runtime_installed: boolean;
  model_installed: boolean;
  model_dir?: string | null;
}

interface MeetingsViewProps {
  onBack: () => void;
  initialAudioPath?: string | null;
}

interface PlatformInfo {
  name: string;
  badgeBg: string;
  badgeColor: string;
  badgeBorder: string;
}

function detectPlatform(url: string): PlatformInfo | null {
  const lower = url.toLowerCase().trim();
  if (lower.includes("youtube.com") || lower.includes("youtu.be")) {
    return {
      name: "YouTube",
      badgeBg: "rgba(239, 68, 68, 0.15)",
      badgeColor: "#f87171",
      badgeBorder: "rgba(239, 68, 68, 0.3)",
    };
  }
  if (lower.includes("drive.google.com")) {
    return {
      name: "Google Drive",
      badgeBg: "rgba(34, 197, 94, 0.15)",
      badgeColor: "#4ade80",
      badgeBorder: "rgba(34, 197, 94, 0.3)",
    };
  }
  if (lower.includes("teams.microsoft.com")) {
    return {
      name: "Microsoft Teams",
      badgeBg: "rgba(99, 102, 241, 0.15)",
      badgeColor: "#818cf8",
      badgeBorder: "rgba(99, 102, 241, 0.3)",
    };
  }
  if (lower.includes("zoom.us")) {
    return {
      name: "Zoom",
      badgeBg: "rgba(59, 130, 246, 0.15)",
      badgeColor: "#60a5fa",
      badgeBorder: "rgba(59, 130, 246, 0.3)",
    };
  }
  if (lower.includes("loom.com")) {
    return {
      name: "Loom",
      badgeBg: "rgba(234, 88, 12, 0.15)",
      badgeColor: "#fb923c",
      badgeBorder: "rgba(234, 88, 12, 0.3)",
    };
  }
  if (lower.includes("tiktok.com")) {
    return {
      name: "TikTok",
      badgeBg: "rgba(6, 182, 212, 0.15)",
      badgeColor: "#22d3ee",
      badgeBorder: "rgba(6, 182, 212, 0.3)",
    };
  }
  if (lower.includes("instagram.com")) {
    return {
      name: "Instagram",
      badgeBg: "rgba(236, 72, 153, 0.15)",
      badgeColor: "#f472b6",
      badgeBorder: "rgba(236, 72, 153, 0.3)",
    };
  }
  if (lower.startsWith("http://") || lower.startsWith("https://")) {
    return {
      name: "Link de Vídeo/Áudio",
      badgeBg: "rgba(59, 130, 246, 0.15)",
      badgeColor: "#60a5fa",
      badgeBorder: "rgba(59, 130, 246, 0.3)",
    };
  }
  return null;
}

function formatDuration(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

function formatDate(val: number | string): string {
  try {
    const num = typeof val === "number" ? val : Number(val);
    const d = !isNaN(num) && num > 0
      ? new Date(num > 1e11 ? num : num * 1000)
      : new Date(val);
    return d.toLocaleString("pt-BR", {
      day: "2-digit",
      month: "2-digit",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return String(val);
  }
}

export default function MeetingsView({
  onBack,
  initialAudioPath,
}: MeetingsViewProps) {
  const isInitialUrl = Boolean(
    initialAudioPath &&
      (initialAudioPath.startsWith("http://") || initialAudioPath.startsWith("https://")),
  );

  const [sourceMode, setSourceMode] = useState<"live" | "file" | "url">(
    isInitialUrl ? "url" : initialAudioPath ? "file" : "live",
  );
  const [meetingUrl, setMeetingUrl] = useState(isInitialUrl ? initialAudioPath || "" : "");
  const [uploadFilePath, setUploadFilePath] = useState(
    isInitialUrl ? "" : initialAudioPath || "",
  );
  const [meetings, setMeetings] = useState<MeetingMinutes[]>([]);
  const [selectedMeeting, setSelectedMeeting] = useState<MeetingMinutes | null>(
    null,
  );
  const [isRecording, setIsRecording] = useState(false);
  const [recordingSeconds, setRecordingSeconds] = useState(0);
  const [audioLevel, setAudioLevel] = useState(0);
  const [isProcessing, setIsProcessing] = useState(false);
  const [processingStatus, setProcessingStatus] = useState<string>("");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [contextHint, setContextHint] = useState("");
  const [engine, setEngine] = useState<"whisper" | "parakeet">("parakeet");
  const [parakeetStatus, setParakeetStatus] = useState<ParakeetStatus | null>(null);
  const [isDownloadingParakeet, setIsDownloadingParakeet] = useState(false);
  const [parakeetProgress, setParakeetProgress] = useState<{ step: string; percentage: number } | null>(null);
  const [activeTab, setActiveTab] = useState<"summary" | "actions" | "diarization">(
    "summary",
  );
  const [copyMsg, setCopyMsg] = useState<string | null>(null);
  const [showRefineBar, setShowRefineBar] = useState(false);
  const [refineHint, setRefineHint] = useState("");
  const [isReprocessing, setIsReprocessing] = useState(false);

  const platform = detectPlatform(meetingUrl);

  const refreshParakeet = () => {
    invoke<ParakeetStatus>("get_parakeet_status")
      .then(setParakeetStatus)
      .catch((e) => console.error("Falha ao obter status Parakeet:", e));
  };

  // Carrega lista de reuniões salvas
  const refreshMeetings = async () => {
    try {
      const list = await invoke<MeetingMinutes[]>("get_meetings");
      setMeetings(list);
      if (list.length > 0 && !selectedMeeting) {
        setSelectedMeeting(list[0]);
      }
    } catch (e) {
      console.error("Erro ao carregar reuniões:", e);
    }
  };

  useEffect(() => {
    refreshMeetings();
    refreshParakeet();
  }, []);

  // Event listeners para gravação ao vivo e status de processamento
  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    const track = (p: Promise<UnlistenFn>) =>
      p.then((fn) => unlisteners.push(fn));

    track(
      listen<number>("meeting-recording-time", (e) => {
        setRecordingSeconds(e.payload);
      }),
    );

    track(
      listen<number>("meeting-audio-level", (e) => {
        setAudioLevel(e.payload);
      }),
    );

    track(
      listen<string>("meeting-status", (e) => {
        setProcessingStatus(e.payload);
      }),
    );

    track(
      listen<MeetingMinutes>("meeting-completed", (e) => {
        setIsProcessing(false);
        setProcessingStatus("");
        setSelectedMeeting(e.payload);
        refreshMeetings();
      }),
    );

    track(
      listen<{ step: string; percentage: number }>("parakeet-download-progress", (e) => {
        setIsDownloadingParakeet(true);
        setParakeetProgress(e.payload);
      }),
    );

    track(
      listen<void>("parakeet-download-complete", () => {
        setIsDownloadingParakeet(false);
        setParakeetProgress(null);
        refreshParakeet();
      }),
    );

    track(
      listen<{ error: string }>("parakeet-download-error", (e) => {
        setIsDownloadingParakeet(false);
        setParakeetProgress(null);
        setErrorMsg(`Erro ao baixar modelo Parakeet: ${e.payload.error}`);
        refreshParakeet();
      }),
    );

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  const handleDownloadParakeet = () => {
    setIsDownloadingParakeet(true);
    setErrorMsg(null);
    invoke("download_parakeet_model").catch((err) => {
      setIsDownloadingParakeet(false);
      setErrorMsg(String(err));
    });
  };

  const handleReprocessMeeting = async () => {
    if (!selectedMeeting) return;
    setIsReprocessing(true);
    setErrorMsg(null);
    try {
      const hint = refineHint.trim() || "Palco com 3 oradores: Apresentador e 2 convidados";
      const updated = await invoke<MeetingMinutes>("reprocess_meeting", {
        id: selectedMeeting.id,
        contextHint: hint,
      });
      setSelectedMeeting(updated);
      setMeetings((prev) => prev.map((m) => (m.id === updated.id ? updated : m)));
      setShowRefineBar(false);
      setActiveTab("diarization");
    } catch (err) {
      setErrorMsg(`Erro ao refinar oradores: ${err}`);
    } finally {
      setIsReprocessing(false);
    }
  };

  // Iniciar gravação ao vivo
  const handleStartLive = async () => {
    try {
      setErrorMsg(null);
      await invoke("start_meeting_recording");
      setIsRecording(true);
      setRecordingSeconds(0);
    } catch (e) {
      setErrorMsg(`Erro ao iniciar gravação: ${e}`);
    }
  };

  // Parar gravação e processar reunião
  const handleStopAndProcess = async () => {
    try {
      setIsRecording(false);
      setIsProcessing(true);
      setProcessingStatus("Finalizando gravação do áudio...");
      const wavPath = await invoke<string>("stop_meeting_recording");
      setProcessingStatus("Iniciando transcrição e diarização...");
      const minutes = await invoke<MeetingMinutes>("process_meeting", {
        audioPath: wavPath,
        contextHint: contextHint.trim() || null,
        engine,
      });
      setSelectedMeeting(minutes);
      refreshMeetings();
    } catch (e) {
      setErrorMsg(`Erro ao processar reunião: ${e}`);
    } finally {
      setIsProcessing(false);
      setProcessingStatus("");
    }
  };

  // Cancelar gravação ao vivo
  const handleCancelRecording = async () => {
    try {
      await invoke("cancel_meeting_recording");
      setIsRecording(false);
      setRecordingSeconds(0);
    } catch (e) {
      console.error(e);
    }
  };

  // Processar arquivo já existente
  const handlePickFile = async () => {
    try {
      const selected = await invoke<string | null>("pick_audio_file");
      if (selected) {
        setUploadFilePath(selected);
      }
    } catch (err) {
      setErrorMsg(`Erro ao selecionar arquivo: ${err}`);
    }
  };

  const handleProcessFile = async () => {
    if (!uploadFilePath.trim()) return;
    try {
      setErrorMsg(null);
      setIsProcessing(true);
      setProcessingStatus("Processando arquivo de reunião...");
      const minutes = await invoke<MeetingMinutes>("process_meeting", {
        audioPath: uploadFilePath.trim(),
        contextHint: contextHint.trim() || null,
        engine,
      });
      setSelectedMeeting(minutes);
      refreshMeetings();
    } catch (e) {
      setErrorMsg(`Erro ao processar arquivo: ${e}`);
    } finally {
      setIsProcessing(false);
      setProcessingStatus("");
    }
  };

  const handleProcessUrl = async () => {
    const cleanUrl = meetingUrl.trim();
    if (!cleanUrl) return;
    try {
      setErrorMsg(null);
      setIsProcessing(true);
      setProcessingStatus("Extraindo áudio da URL da reunião...");
      const minutes = await invoke<MeetingMinutes>("process_meeting_url", {
        url: cleanUrl,
        contextHint: contextHint.trim() || null,
        engine,
      });
      setSelectedMeeting(minutes);
      setMeetingUrl("");
      refreshMeetings();
      setActiveTab("summary");
    } catch (e) {
      setErrorMsg(`Erro ao processar reunião a partir do link: ${e}`);
    } finally {
      setIsProcessing(false);
      setProcessingStatus("");
    }
  };

  // Toggle checkbox de ação
  const handleToggleAction = async (actionIdx: number, currentDone: boolean) => {
    if (!selectedMeeting) return;
    try {
      await invoke("set_meeting_action_done", {
        meetingId: selectedMeeting.id,
        actionIdx,
        done: !currentDone,
      });
      // Atualiza estado local
      const updated = {
        ...selectedMeeting,
        action_items: (selectedMeeting.action_items || []).map((item, idx) =>
          idx === actionIdx ? { ...item, done: !currentDone } : item,
        ),
      };
      setSelectedMeeting(updated);
      setMeetings((prev) =>
        prev.map((m) => (m.id === updated.id ? updated : m)),
      );
    } catch (e) {
      console.error("Erro ao atualizar item de ação:", e);
    }
  };

  // Deletar reunião
  const handleDeleteMeeting = async (id: string) => {
    if (!confirm("Deseja realmente excluir esta ata de reunião?")) return;
    try {
      await invoke("delete_meeting", { id });
      setMeetings((prev) => prev.filter((m) => m.id !== id));
      if (selectedMeeting?.id === id) {
        setSelectedMeeting(null);
      }
    } catch (e) {
      setErrorMsg(`Erro ao excluir: ${e}`);
    }
  };

  // Exportar Markdown
  const handleExportMarkdown = () => {
    if (!selectedMeeting) return;
    const actions = selectedMeeting.action_items || [];
    const decisions = selectedMeeting.key_decisions || [];
    const turns = selectedMeeting.diarized_transcript || [];

    let md = `# ${selectedMeeting.title}\n\n`;
    md += `**Data:** ${formatDate(selectedMeeting.created_at)}\n`;
    md += `**Duração:** ${formatDuration(selectedMeeting.duration_sec)}\n\n`;
    md += `## Resumo Executivo\n\n${selectedMeeting.executive_summary || selectedMeeting.raw_transcript}\n\n`;

    if (decisions.length > 0) {
      md += `## Decisões Principais\n\n`;
      decisions.forEach((dec) => {
        md += `- ${dec}\n`;
      });
      md += "\n";
    }

    if (actions.length > 0) {
      md += `## Plano de Ação por Participante\n\n`;
      actions.forEach((item) => {
        const check = item.done ? "[x]" : "[ ]";
        const deadlineStr = item.deadline ? ` (Prazo: ${item.deadline})` : "";
        md += `- ${check} **${item.speaker}**: ${item.task}${deadlineStr}\n`;
      });
      md += "\n";
    }

    if (turns.length > 0) {
      md += `## Transcrição por Orador\n\n`;
      turns.forEach((seg) => {
        md += `**[${seg.timestamp}] ${seg.speaker}:** ${seg.text}\n\n`;
      });
    }

    const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${selectedMeeting.title.replace(/[^a-zA-Z0-9_-]/g, "_")}_ata.md`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleCopySummary = async () => {
    if (!selectedMeeting) return;
    const actions = selectedMeeting.action_items || [];
    let text = `${selectedMeeting.title}\n\nRESUMO EXECUTIVO:\n${selectedMeeting.executive_summary || selectedMeeting.raw_transcript}\n\n`;
    if (actions.length > 0) {
      text += `AÇÕES POR PARTICIPANTE:\n`;
      actions.forEach((item) => {
        text += `- [${item.done ? "X" : " "}] ${item.speaker}: ${item.task}${item.deadline ? ` (${item.deadline})` : ""}\n`;
      });
    }
    try {
      await navigator.clipboard.writeText(text);
      setCopyMsg("Ata copiada!");
      setTimeout(() => setCopyMsg(null), 2000);
    } catch {
      setCopyMsg("Erro ao copiar");
      setTimeout(() => setCopyMsg(null), 2000);
    }
  };

  return (
    <div className="meetings-container">
      <header className="page-header">
        <button
          type="button"
          className="btn-back"
          onClick={onBack}
          aria-label="Voltar"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <line x1="19" y1="12" x2="5" y2="12" />
            <polyline points="12 19 5 12 12 5" />
          </svg>
          Voltar
        </button>
        <h2>Reuniões e Atas Inteligentes</h2>
      </header>

      {/* Card de Gravação / Processamento */}
      <div className="settings-card" style={{ marginTop: "1rem" }}>
        <h3 className="card-title">Nova Reunião</h3>
        <p className="field-hint" style={{ marginTop: "0.25rem", marginBottom: "1rem" }}>
          Grave o áudio ao vivo ou importe um arquivo de reunião (Zoom, Teams, Meet). A IA transcreve, separa as falas por orador e gera um resumo executivo com plano de ações e prazos.
        </p>

        <div style={{ marginBottom: "1rem" }}>
          <label className="field-label" htmlFor="meeting-context">
            Contexto e Participantes (opcional, ajuda a IA a associar os nomes corretos):
          </label>
          <input
            id="meeting-context"
            type="text"
            className="text-input"
            placeholder="Ex: Alinhamento semanal de produto - Rafael, Ana e Carlos"
            value={contextHint}
            onChange={(e) => setContextHint(e.target.value)}
            disabled={isRecording || isProcessing}
          />
          <div style={{ display: "flex", gap: "0.4rem", flexWrap: "wrap", marginTop: "0.45rem" }}>
            {[
              "Palco com 3 oradores: Apresentador e 2 convidados",
              "Entrevista (2 pessoas)",
              "Reunião de equipe com múltiplos participantes",
              "Apresentador e plateia",
            ].map((preset) => (
              <button
                key={preset}
                type="button"
                style={{
                  fontSize: "0.72rem",
                  padding: "0.2rem 0.55rem",
                  borderRadius: "10px",
                  background: contextHint === preset ? "rgba(59, 130, 246, 0.25)" : "rgba(255, 255, 255, 0.05)",
                  border: contextHint === preset ? "1px solid #3b82f6" : "1px solid rgba(255, 255, 255, 0.1)",
                  color: contextHint === preset ? "#60a5fa" : "rgba(255, 255, 255, 0.7)",
                  cursor: "pointer",
                }}
                onClick={() => setContextHint(preset)}
                disabled={isRecording || isProcessing}
              >
                {preset}
              </button>
            ))}
          </div>
        </div>

        {/* Escolha do Motor de Transcrição e Diarização */}
        <div style={{ marginBottom: "1rem" }}>
          <label className="field-label" style={{ marginBottom: "0.4rem", display: "block" }}>
            Motor de Reconhecimento & Diarização:
          </label>
          <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
            <button
              type="button"
              className={`toggle-btn ${engine === "parakeet" ? "active" : ""}`}
              onClick={() => setEngine("parakeet")}
              style={{
                display: "flex",
                alignItems: "center",
                gap: "6px",
                padding: "0.45rem 0.85rem",
                borderRadius: "6px",
              }}
            >
              <span>NVIDIA Parakeet-TDT v3</span>
              <span
                style={{
                  fontSize: "0.68rem",
                  padding: "1px 5px",
                  borderRadius: "4px",
                  background: "rgba(16, 185, 129, 0.2)",
                  color: "#10b981",
                  fontWeight: 600,
                }}
              >
                Recomendado • Oradores
              </span>
            </button>
            <button
              type="button"
              className={`toggle-btn ${engine === "whisper" ? "active" : ""}`}
              onClick={() => setEngine("whisper")}
              style={{ padding: "0.45rem 0.85rem", borderRadius: "6px" }}
            >
              Whisper (Padrão)
            </button>
          </div>

          {/* Aviso se Parakeet foi selecionado mas não está instalado */}
          {engine === "parakeet" && parakeetStatus && !parakeetStatus.installed && (
            <div
              style={{
                marginTop: "0.6rem",
                padding: "0.75rem",
                borderRadius: "6px",
                background: "rgba(234, 179, 8, 0.08)",
                border: "1px solid rgba(234, 179, 8, 0.25)",
                display: "flex",
                flexDirection: "column",
                gap: "0.5rem",
              }}
            >
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: "0.5rem" }}>
                <span style={{ fontSize: "0.82rem", color: "#facc15" }}>
                  O modelo NVIDIA Parakeet-TDT v3 ainda não está instalado no computador (~670 MB).
                </span>
                <button
                  type="button"
                  className="btn-primary btn-small"
                  onClick={handleDownloadParakeet}
                  disabled={isDownloadingParakeet}
                >
                  {isDownloadingParakeet ? "Baixando…" : "Baixar Modelo (~670 MB)"}
                </button>
              </div>

              {parakeetProgress && (
                <div>
                  <div className="model-progress-bar">
                    <div
                      className="model-progress-fill"
                      style={{
                        width: `${Math.min(100, Math.max(5, parakeetProgress.percentage))}%`,
                      }}
                    />
                  </div>
                  <div style={{ display: "flex", justifyContent: "space-between", marginTop: "0.2rem" }}>
                    <span className="field-hint" style={{ fontSize: "0.72rem" }}>
                      {parakeetProgress.step}
                    </span>
                    <span className="field-hint" style={{ fontSize: "0.72rem" }}>
                      {parakeetProgress.percentage.toFixed(0)}%
                    </span>
                  </div>
                </div>
              )}
            </div>
          )}
        </div>

        {/* Abas de Origem da Reunião: Ao Vivo vs Arquivo Local vs Link da Web */}
        <div className="transcribe-mode-tabs" style={{ marginBottom: "1rem" }}>
          <button
            type="button"
            className={`transcribe-mode-btn ${sourceMode === "live" ? "active" : ""}`}
            onClick={() => {
              setSourceMode("live");
              setErrorMsg(null);
            }}
            disabled={isRecording || isProcessing}
          >
            <svg
              width="15"
              height="15"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
              <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
              <line x1="12" y1="19" x2="12" y2="22" />
            </svg>
            Gravação ao Vivo
          </button>

          <button
            type="button"
            className={`transcribe-mode-btn ${sourceMode === "file" ? "active" : ""}`}
            onClick={() => {
              setSourceMode("file");
              setErrorMsg(null);
            }}
            disabled={isRecording || isProcessing}
          >
            <svg
              width="15"
              height="15"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
              <polyline points="14 2 14 8 20 8" />
              <line x1="16" y1="13" x2="8" y2="13" />
              <line x1="16" y1="17" x2="8" y2="17" />
              <polyline points="10 9 9 9 8 9" />
            </svg>
            Arquivo Local (Áudio / Vídeo)
          </button>

          <button
            type="button"
            className={`transcribe-mode-btn ${sourceMode === "url" ? "active" : ""}`}
            onClick={() => {
              setSourceMode("url");
              setErrorMsg(null);
            }}
            disabled={isRecording || isProcessing}
          >
            <svg
              width="15"
              height="15"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <circle cx="12" cy="12" r="10" />
              <line x1="2" y1="12" x2="22" y2="12" />
              <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
            </svg>
            Link da Web (YouTube, Teams, Drive, etc.)
          </button>
        </div>

        {/* Painel de Ações conforme a Origem Escolhida */}
        <div className="meeting-recorder-box">
          {isRecording ? (
            <div className="meeting-live-recording">
              <div className="rec-live-indicator">
                <span className="rec-dot-pulsing" />
                <span className="rec-timer">{formatDuration(recordingSeconds)}</span>
                <span className="rec-tag">Gravando Reunião...</span>
              </div>

              {/* Medidor de Áudio */}
              <div className="meeting-audio-meter">
                <div
                  className="meeting-audio-fill"
                  style={{
                    width: `${Math.min(100, Math.max(8, audioLevel * 250))}%`,
                  }}
                />
              </div>

              <div style={{ display: "flex", gap: "0.75rem", marginTop: "1rem" }}>
                <button
                  type="button"
                  className="btn-primary"
                  onClick={handleStopAndProcess}
                >
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    style={{ verticalAlign: "-2px", marginRight: "6px" }}
                  >
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                  Concluir e Gerar Ata
                </button>
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={handleCancelRecording}
                >
                  Cancelar Gravação
                </button>
              </div>
            </div>
          ) : sourceMode === "live" ? (
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: "1rem" }}>
              <div>
                <p style={{ margin: 0, fontSize: "0.9rem", fontWeight: 500, color: "rgba(255, 255, 255, 0.9)" }}>
                  Gravar Reunião Presencial ou Online ao Vivo
                </p>
                <p className="field-hint" style={{ margin: "0.2rem 0 0 0", fontSize: "0.8rem" }}>
                  Captura pelo microfone da máquina com medidor de volume e diarização automática com IA ao finalizar.
                </p>
              </div>
              <button
                type="button"
                className="btn-primary"
                onClick={handleStartLive}
                disabled={isProcessing}
                style={{ display: "inline-flex", alignItems: "center", gap: "6px", padding: "0.55rem 1.1rem" }}
              >
                <svg
                  width="15"
                  height="15"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
                  <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                  <line x1="12" y1="19" x2="12" y2="22" />
                </svg>
                Iniciar Gravação ao Vivo
              </button>
            </div>
          ) : sourceMode === "file" ? (
            <div className="file-selection-box">
              <div className="file-path-row">
                <input
                  type="text"
                  className="text-input"
                  placeholder="Caminho do arquivo ou use Selecionar (.mp4, .mkv, .mp3, .wav, .m4a)..."
                  value={uploadFilePath}
                  onChange={(e) => setUploadFilePath(e.target.value)}
                  disabled={isProcessing}
                />
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={handlePickFile}
                  disabled={isProcessing}
                >
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    style={{ verticalAlign: "-2px", marginRight: "6px" }}
                  >
                    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                  </svg>
                  Selecionar Arquivo
                </button>
                <button
                  type="button"
                  className="btn-primary"
                  onClick={handleProcessFile}
                  disabled={!uploadFilePath.trim() || isProcessing}
                  style={{ display: "inline-flex", alignItems: "center", gap: "6px", whiteSpace: "nowrap" }}
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                    <polygon points="5 3 19 12 5 21 5 3" />
                  </svg>
                  Processar Arquivo
                </button>
              </div>
              <p className="field-hint" style={{ fontSize: "0.8rem", margin: 0 }}>
                Suporte nativo a vídeos (MP4, MKV, WebM, MOV, AVI) e gravações de áudio (MP3, WAV, OGG, OPUS, M4A, FLAC, AAC).
              </p>
            </div>
          ) : (
            <div className="file-selection-box">
              <div className="file-path-row">
                <input
                  type="url"
                  className="text-input"
                  placeholder="Cole o link da reunião (YouTube, Google Drive, Teams, Zoom, Loom, etc.)..."
                  value={meetingUrl}
                  onChange={(e) => setMeetingUrl(e.target.value)}
                  disabled={isProcessing}
                />
                {meetingUrl && (
                  <button
                    type="button"
                    className="btn-secondary"
                    onClick={() => setMeetingUrl("")}
                    disabled={isProcessing}
                    title="Limpar link"
                    style={{ padding: "0.45rem 0.75rem" }}
                  >
                    ✕
                  </button>
                )}
                <button
                  type="button"
                  className="btn-primary"
                  onClick={handleProcessUrl}
                  disabled={!meetingUrl.trim() || isProcessing}
                  style={{ display: "inline-flex", alignItems: "center", gap: "6px", whiteSpace: "nowrap" }}
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                    <polygon points="5 3 19 12 5 21 5 3" />
                  </svg>
                  Extrair e Gerar Ata
                </button>
              </div>
              {platform ? (
                <div style={{ display: "flex", alignItems: "center", gap: "0.75rem", marginTop: "0.25rem" }}>
                  <span
                    className="platform-badge"
                    style={{
                      background: platform.badgeBg,
                      color: platform.badgeColor,
                      border: `1px solid ${platform.badgeBorder}`,
                    }}
                  >
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                      <polygon points="5 3 19 12 5 21 5 3" />
                    </svg>
                    {platform.name}
                  </span>
                  <span className="field-hint" style={{ fontSize: "0.8rem", margin: 0 }}>
                    O app baixa apenas a faixa de áudio otimizada e gera a ata com oradores automaticamente.
                  </span>
                </div>
              ) : (
                <p className="field-hint" style={{ fontSize: "0.8rem", margin: 0 }}>
                  Suporta links de reuniões gravadas no YouTube, Google Drive, Teams, Zoom, Loom ou qualquer link direto de mídia.
                </p>
              )}
            </div>
          )}
        </div>

        {isProcessing && (
          <div className="meeting-processing-box">
            <span className="rec-dot-pulsing" style={{ background: "var(--accent)" }} />
            <span>{processingStatus || "Processando reunião com IA..."}</span>
          </div>
        )}

        {errorMsg && (
          <div className="error-box" style={{ marginTop: "1rem" }}>
            <p className="label">Erro na Reunião:</p>
            <pre>{errorMsg}</pre>
          </div>
        )}
      </div>

      {/* Histórico e Detalhes da Ata */}
      <div className="meeting-layout" style={{ marginTop: "1rem" }}>
        {/* Sidebar com histórico de reuniões */}
        <aside className="meeting-sidebar">
          <div className="meeting-sidebar-header">
            <h4>Atas Salvas ({meetings.length})</h4>
          </div>
          <div className="meeting-sidebar-list">
            {meetings.length === 0 ? (
              <p className="field-hint" style={{ padding: "1rem", textAlign: "center" }}>
                Nenhuma ata gravada ainda. Inicie uma gravação acima para testar.
              </p>
            ) : (
              meetings.map((m) => (
                <div
                  key={m.id}
                  className={`meeting-item-card ${selectedMeeting?.id === m.id ? "active" : ""}`}
                  onClick={() => setSelectedMeeting(m)}
                >
                  <div className="meeting-item-title">{m.title}</div>
                  <div className="meeting-item-meta">
                    <span>{formatDate(m.created_at)}</span>
                    <span>{formatDuration(m.duration_sec)}</span>
                  </div>
                  <div className="meeting-item-actions-badge">
                    {(m.action_items || []).filter((a) => a.done).length}/{(m.action_items || []).length} ações
                  </div>
                </div>
              ))
            )}
          </div>
        </aside>

        {/* Visualizador da Ata Selecionada */}
        <main className="meeting-content-pane">
          {selectedMeeting ? (
            <div className="settings-card" style={{ margin: 0 }}>
              <div className="meeting-header-row">
                <div>
                  <h3 className="card-title">{selectedMeeting.title}</h3>
                  <p className="field-hint">
                    {formatDate(selectedMeeting.created_at)} • Duração: {formatDuration(selectedMeeting.duration_sec)}
                  </p>
                </div>
                <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
                  {copyMsg && <span className="copy-badge">{copyMsg}</span>}
                  <button
                    type="button"
                    className="btn-secondary btn-small"
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: "5px",
                      borderColor: showRefineBar ? "#3b82f6" : undefined,
                      color: showRefineBar ? "#60a5fa" : undefined,
                    }}
                    onClick={() => {
                      if (!showRefineBar && !refineHint) {
                        setRefineHint("Palco com 3 oradores: Apresentador e 2 convidados");
                      }
                      setShowRefineBar(!showRefineBar);
                    }}
                    title="Reanalisa a conversa e separa os oradores com IA"
                  >
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                      <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
                      <circle cx="9" cy="7" r="4" />
                      <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
                      <path d="M16 3.13a4 4 0 0 1 0 7.75" />
                    </svg>
                    Refinar Oradores
                  </button>
                  <button
                    type="button"
                    className="btn-secondary btn-small"
                    onClick={handleCopySummary}
                  >
                    Copiar Ata
                  </button>
                  <button
                    type="button"
                    className="btn-secondary btn-small"
                    onClick={handleExportMarkdown}
                  >
                    Baixar .md
                  </button>
                  <button
                    type="button"
                    className="btn-secondary btn-small"
                    style={{ color: "#ef4444" }}
                    onClick={() => handleDeleteMeeting(selectedMeeting.id)}
                    title="Excluir ata"
                  >
                    Excluir
                  </button>
                </div>
              </div>

              {/* Barra de Refinamento de Oradores com IA */}
              {showRefineBar && (
                <div
                  style={{
                    background: "rgba(30, 41, 59, 0.75)",
                    border: "1px solid rgba(59, 130, 246, 0.35)",
                    borderRadius: "8px",
                    padding: "0.85rem 1rem",
                    marginTop: "0.85rem",
                  }}
                >
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "0.4rem" }}>
                    <span style={{ fontSize: "0.82rem", fontWeight: 600, color: "#93c5fd" }}>
                      Separar Oradores com IA (sem regravar):
                    </span>
                    <button
                      type="button"
                      onClick={() => setShowRefineBar(false)}
                      style={{ background: "none", border: "none", color: "rgba(255,255,255,0.5)", cursor: "pointer", fontSize: "0.85rem" }}
                    >
                      ✕
                    </button>
                  </div>
                  <p className="field-hint" style={{ fontSize: "0.78rem", marginBottom: "0.6rem" }}>
                    Especifique o contexto da reunião ou o número de pessoas para re-diarizar as falas e perguntas:
                  </p>
                  <div style={{ display: "flex", gap: "0.5rem", marginBottom: "0.55rem" }}>
                    <input
                      type="text"
                      className="text-input"
                      style={{ fontSize: "0.82rem", padding: "0.45rem 0.65rem", flex: 1 }}
                      placeholder="Ex: Palco com 3 oradores: Apresentador e 2 convidados"
                      value={refineHint}
                      onChange={(e) => setRefineHint(e.target.value)}
                      disabled={isReprocessing}
                    />
                    <button
                      type="button"
                      className="btn-primary"
                      style={{
                        padding: "0.45rem 0.85rem",
                        fontSize: "0.82rem",
                        whiteSpace: "nowrap",
                        display: "flex",
                        alignItems: "center",
                        gap: "6px",
                      }}
                      onClick={handleReprocessMeeting}
                      disabled={isReprocessing}
                    >
                      {isReprocessing ? (
                        <>
                          <span className="rec-dot-pulsing" style={{ width: 8, height: 8 }} />
                          Analisando com IA...
                        </>
                      ) : (
                        "Reprocessar Ata"
                      )}
                    </button>
                  </div>
                  {/* Chips Rápidos */}
                  <div style={{ display: "flex", gap: "0.35rem", flexWrap: "wrap" }}>
                    {[
                      "Palco com 3 oradores: Apresentador e 2 convidados",
                      "Entrevista (2 pessoas)",
                      "Reunião com múltiplos participantes",
                      "Apresentador e plateia",
                    ].map((chip) => (
                      <button
                        key={chip}
                        type="button"
                        style={{
                          fontSize: "0.72rem",
                          padding: "0.15rem 0.5rem",
                          borderRadius: "10px",
                          background: refineHint === chip ? "rgba(59, 130, 246, 0.3)" : "rgba(255, 255, 255, 0.05)",
                          border: refineHint === chip ? "1px solid #3b82f6" : "1px solid rgba(255, 255, 255, 0.1)",
                          color: refineHint === chip ? "#60a5fa" : "rgba(255, 255, 255, 0.7)",
                          cursor: "pointer",
                        }}
                        onClick={() => setRefineHint(chip)}
                        disabled={isReprocessing}
                      >
                        {chip}
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {/* Abas da Ata */}
              <div className="tab-buttons" style={{ marginTop: "1rem", marginBottom: "0.75rem" }}>
                <button
                  type="button"
                  className={`tab-btn ${activeTab === "summary" ? "active" : ""}`}
                  onClick={() => setActiveTab("summary")}
                >
                  Resumo e Decisões
                </button>
                <button
                  type="button"
                  className={`tab-btn ${activeTab === "actions" ? "active" : ""}`}
                  onClick={() => setActiveTab("actions")}
                >
                  Plano de Ações ({(selectedMeeting.action_items || []).length})
                </button>
                <button
                  type="button"
                  className={`tab-btn ${activeTab === "diarization" ? "active" : ""}`}
                  onClick={() => setActiveTab("diarization")}
                >
                  Transcrição por Orador ({(selectedMeeting.diarized_transcript || []).length})
                </button>
              </div>

              {/* Aba 1: Resumo e Decisões */}
              {activeTab === "summary" && (
                <div className="meeting-summary-view">
                  <div className="meeting-section">
                    <h4 className="meeting-section-title">Resumo Executivo</h4>
                    <div className="meeting-summary-text">
                      {selectedMeeting.executive_summary || selectedMeeting.raw_transcript}
                    </div>
                  </div>

                  {(selectedMeeting.key_decisions || []).length > 0 && (
                    <div className="meeting-section" style={{ marginTop: "1.25rem" }}>
                      <h4 className="meeting-section-title">Decisões Principais</h4>
                      <ul className="meeting-decisions-list">
                        {(selectedMeeting.key_decisions || []).map((dec, idx) => (
                          <li key={idx} className="meeting-decision-item">
                            <span className="bullet-check">✓</span>
                            <span>{dec}</span>
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}
                </div>
              )}

              {/* Aba 2: Plano de Ações por Orador */}
              {activeTab === "actions" && (
                <div className="meeting-actions-view">
                  {(selectedMeeting.action_items || []).length === 0 ? (
                    <p className="field-hint" style={{ padding: "1rem" }}>
                      Nenhuma ação pendente identificada nesta reunião.
                    </p>
                  ) : (
                    <div className="meeting-tasks-list">
                      {(selectedMeeting.action_items || []).map((item, idx) => (
                        <div
                          key={idx}
                          className={`meeting-task-card ${item.done ? "done" : ""}`}
                        >
                          <label className="checkbox-row" style={{ flex: 1, margin: 0 }}>
                            <input
                              type="checkbox"
                              checked={item.done}
                              onChange={() => handleToggleAction(idx, item.done)}
                            />
                            <span className="task-participant-badge">
                              {item.speaker}
                            </span>
                            <span className="task-desc">{item.task}</span>
                          </label>
                          {item.deadline && (
                            <span className="task-deadline-badge">
                              Prazo: {item.deadline}
                            </span>
                          )}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {/* Aba 3: Transcrição Diarizada */}
              {activeTab === "diarization" && (
                <div className="meeting-diarization-view">
                  {(selectedMeeting.diarized_transcript || []).length === 0 ? (
                    <div className="transcription-result-box">
                      <p style={{ whiteSpace: "pre-wrap", lineHeight: 1.6 }}>
                        {selectedMeeting.raw_transcript}
                      </p>
                    </div>
                  ) : (
                    <div className="meeting-dialog-list">
                      {(selectedMeeting.diarized_transcript || []).map((seg, idx) => (
                        <div key={idx} className="meeting-dialog-bubble">
                          <div className="speaker-header">
                            <span className="speaker-name">{seg.speaker}</span>
                            <span className="speaker-time">{seg.timestamp}</span>
                          </div>
                          <div className="speaker-speech">{seg.text}</div>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
          ) : (
            <div className="settings-card empty-state-box">
              <p>Selecione uma reunião ao lado ou inicie uma nova gravação acima.</p>
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
