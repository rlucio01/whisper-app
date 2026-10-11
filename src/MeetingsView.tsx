import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface SpeakerSegment {
  speaker: String;
  start_sec: number;
  end_sec: number;
  text: string;
}

export interface ActionItem {
  participant: string;
  task: string;
  deadline?: string | null;
  done: boolean;
}

export interface MeetingMinutes {
  id: string;
  title: string;
  created_at: string;
  duration_sec: number;
  summary: string;
  key_decisions: string[];
  action_items: ActionItem[];
  transcript: string;
  diarized_segments: SpeakerSegment[];
}

interface MeetingsViewProps {
  onBack: () => void;
  initialAudioPath?: string | null;
}

function formatDuration(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

function formatDate(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString("pt-BR", {
      day: "2-digit",
      month: "2-digit",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

export default function MeetingsView({
  onBack,
  initialAudioPath,
}: MeetingsViewProps) {
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
  const [uploadFilePath, setUploadFilePath] = useState(initialAudioPath || "");
  const [activeTab, setActiveTab] = useState<"summary" | "actions" | "diarization">(
    "summary",
  );
  const [copyMsg, setCopyMsg] = useState<string | null>(null);

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

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, []);

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
        action_items: selectedMeeting.action_items.map((item, idx) =>
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
    let md = `# ${selectedMeeting.title}\n\n`;
    md += `**Data:** ${formatDate(selectedMeeting.created_at)}\n`;
    md += `**Duração:** ${formatDuration(selectedMeeting.duration_sec)}\n\n`;
    md += `## Resumo Executivo\n\n${selectedMeeting.summary}\n\n`;

    if (selectedMeeting.key_decisions.length > 0) {
      md += `## Decisões Principais\n\n`;
      selectedMeeting.key_decisions.forEach((dec) => {
        md += `- ${dec}\n`;
      });
      md += "\n";
    }

    if (selectedMeeting.action_items.length > 0) {
      md += `## Plano de Ação por Participante\n\n`;
      selectedMeeting.action_items.forEach((item) => {
        const check = item.done ? "[x]" : "[ ]";
        const deadlineStr = item.deadline ? ` (Prazo: ${item.deadline})` : "";
        md += `- ${check} **${item.participant}**: ${item.task}${deadlineStr}\n`;
      });
      md += "\n";
    }

    if (selectedMeeting.diarized_segments.length > 0) {
      md += `## Transcrição por Orador\n\n`;
      selectedMeeting.diarized_segments.forEach((seg) => {
        md += `**[${formatDuration(seg.start_sec)}] ${seg.speaker}:** ${seg.text}\n\n`;
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
    let text = `${selectedMeeting.title}\n\nRESUMO EXECUTIVO:\n${selectedMeeting.summary}\n\n`;
    if (selectedMeeting.action_items.length > 0) {
      text += `AÇÕES POR PARTICIPANTE:\n`;
      selectedMeeting.action_items.forEach((item) => {
        text += `- [${item.done ? "X" : " "}] ${item.participant}: ${item.task}${item.deadline ? ` (${item.deadline})` : ""}\n`;
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
        </div>

        {/* Painel de Gravação ao Vivo */}
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
          ) : (
            <div className="meeting-actions-row">
              <button
                type="button"
                className="btn-primary"
                onClick={handleStartLive}
                disabled={isProcessing}
              >
                <svg
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  style={{ verticalAlign: "-2px", marginRight: "6px" }}
                >
                  <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
                  <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                  <line x1="12" y1="19" x2="12" y2="22" />
                </svg>
                Iniciar Gravação ao Vivo
              </button>

              <div className="meeting-divider-or">ou processe arquivo gravado:</div>

              <div style={{ display: "flex", gap: "0.5rem", flex: 1 }}>
                <input
                  type="text"
                  className="text-input"
                  placeholder="Caminho do áudio (.mp3, .ogg, .wav, .m4a)..."
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
                  Selecionar
                </button>
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={handleProcessFile}
                  disabled={!uploadFilePath.trim() || isProcessing}
                >
                  Processar
                </button>
              </div>
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
                    {m.action_items.filter((a) => a.done).length}/{m.action_items.length} ações
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
                  Plano de Ações ({selectedMeeting.action_items.length})
                </button>
                <button
                  type="button"
                  className={`tab-btn ${activeTab === "diarization" ? "active" : ""}`}
                  onClick={() => setActiveTab("diarization")}
                >
                  Transcrição por Orador ({selectedMeeting.diarized_segments.length})
                </button>
              </div>

              {/* Aba 1: Resumo e Decisões */}
              {activeTab === "summary" && (
                <div className="meeting-summary-view">
                  <div className="meeting-section">
                    <h4 className="meeting-section-title">Resumo Executivo</h4>
                    <div className="meeting-summary-text">{selectedMeeting.summary}</div>
                  </div>

                  {selectedMeeting.key_decisions.length > 0 && (
                    <div className="meeting-section" style={{ marginTop: "1.25rem" }}>
                      <h4 className="meeting-section-title">Decisões Principais</h4>
                      <ul className="meeting-decisions-list">
                        {selectedMeeting.key_decisions.map((dec, idx) => (
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
                  {selectedMeeting.action_items.length === 0 ? (
                    <p className="field-hint" style={{ padding: "1rem" }}>
                      Nenhuma ação pendente identificada nesta reunião.
                    </p>
                  ) : (
                    <div className="meeting-tasks-list">
                      {selectedMeeting.action_items.map((item, idx) => (
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
                              {item.participant}
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
                  {selectedMeeting.diarized_segments.length === 0 ? (
                    <div className="transcription-result-box">
                      <p style={{ whiteSpace: "pre-wrap", lineHeight: 1.6 }}>
                        {selectedMeeting.transcript}
                      </p>
                    </div>
                  ) : (
                    <div className="meeting-dialog-list">
                      {selectedMeeting.diarized_segments.map((seg, idx) => (
                        <div key={idx} className="meeting-dialog-bubble">
                          <div className="speaker-header">
                            <span className="speaker-name">{seg.speaker}</span>
                            <span className="speaker-time">
                              {formatDuration(seg.start_sec)} - {formatDuration(seg.end_sec)}
                            </span>
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
