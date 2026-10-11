import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface TranscriptSegment {
  start_sec: number;
  end_sec: number;
  text: string;
}

export interface TranscriptionFileResult {
  text: string;
  segments: TranscriptSegment[];
  duration_sec: number;
}

interface AudioFileTranscribeProps {
  onBack: () => void;
  onSendToMeeting?: (filePath: string) => void;
}

function formatDuration(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export default function AudioFileTranscribe({
  onBack,
  onSendToMeeting,
}: AudioFileTranscribeProps) {
  const [filePath, setFilePath] = useState("");
  const [isProcessing, setIsProcessing] = useState(false);
  const [result, setResult] = useState<TranscriptionFileResult | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [copyMsg, setCopyMsg] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"text" | "segments">("text");

  async function handlePickFile() {
    try {
      const selected = await invoke<string | null>("pick_audio_file");
      if (selected) {
        setFilePath(selected);
        setErrorMsg(null);
      }
    } catch (err) {
      setErrorMsg(`Erro ao selecionar arquivo: ${err}`);
    }
  }

  async function handleTranscribe() {
    if (!filePath.trim()) return;
    setIsProcessing(true);
    setErrorMsg(null);
    setResult(null);

    try {
      const res = await invoke<TranscriptionFileResult>("transcribe_audio_file", {
        filePath: filePath.trim(),
      });
      setResult(res);
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setIsProcessing(false);
    }
  }

  async function handleCopy() {
    if (!result?.text) return;
    try {
      await navigator.clipboard.writeText(result.text);
      setCopyMsg("Copiado!");
      setTimeout(() => setCopyMsg(null), 2000);
    } catch (err) {
      setCopyMsg(`Falha ao copiar: ${err}`);
      setTimeout(() => setCopyMsg(null), 2000);
    }
  }

  function handleDownloadTxt() {
    if (!result?.text) return;
    const blob = new Blob([result.text], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    const baseName = filePath.split(/[\\/]/).pop()?.replace(/\.[^/.]+$/, "") || "transcricao";
    a.download = `${baseName}_transcricao.txt`;
    a.click();
    URL.revokeObjectURL(url);
  }

  const fileName = filePath.split(/[\\/]/).pop() || "";
  const wordCount = result
    ? result.text.trim().split(/\s+/).filter(Boolean).length
    : 0;

  return (
    <div className="audio-file-transcribe-view">
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
        <h2>Transcrever Arquivo de Audio</h2>
      </header>

      <div className="settings-card" style={{ marginTop: "1rem" }}>
        <h3 className="card-title">Selecionar Arquivo</h3>
        <p className="field-hint" style={{ marginTop: "0.25rem", marginBottom: "1rem" }}>
          Transcreva gravações existentes nos formatos <strong>OGG, WAV, MP3, M4A, FLAC, AAC</strong> com segmentação temporal e alta fidelidade.
        </p>

        <div className="file-selection-box">
          <div className="file-path-row">
            <input
              type="text"
              className="text-input"
              placeholder="Caminho do arquivo ou use o botão Selecionar..."
              value={filePath}
              onChange={(e) => setFilePath(e.target.value)}
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
          </div>

          {fileName && (
            <div className="file-chip">
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
                <path d="M19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07" />
              </svg>
              <span className="file-chip-name">{fileName}</span>
              <button
                type="button"
                className="file-chip-clear"
                onClick={() => {
                  setFilePath("");
                  setResult(null);
                }}
                title="Remover"
              >
                ✕
              </button>
            </div>
          )}

          <div style={{ marginTop: "1rem", display: "flex", gap: "0.75rem", alignItems: "center" }}>
            <button
              type="button"
              className="btn-primary"
              disabled={!filePath.trim() || isProcessing}
              onClick={handleTranscribe}
            >
              {isProcessing ? (
                <>
                  <span className="rec-dot-pulsing" style={{ background: "#fff" }} />
                  Transcrevendo audio...
                </>
              ) : (
                <>
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
                    <polygon points="5 3 19 12 5 21 5 3" />
                  </svg>
                  Iniciar Transcrição
                </>
              )}
            </button>

            {onSendToMeeting && filePath.trim() && (
              <button
                type="button"
                className="btn-secondary"
                disabled={isProcessing}
                onClick={() => onSendToMeeting(filePath.trim())}
                title="Processar este arquivo na aba de Reuniões para diarização por orador e ata"
              >
                Gerar Ata de Reunião
              </button>
            )}
          </div>
        </div>

        {errorMsg && (
          <div className="error-box" style={{ marginTop: "1rem" }}>
            <p className="label">Erro ao transcrever arquivo:</p>
            <pre>{errorMsg}</pre>
          </div>
        )}
      </div>

      {result && (
        <div className="settings-card" style={{ marginTop: "1rem" }}>
          <div className="meeting-header-row">
            <div>
              <h3 className="card-title">Resultado da Transcrição</h3>
              <p className="field-hint">
                Duração estimada: <strong>{formatDuration(result.duration_sec)}</strong> • {wordCount} palavras • {result.segments.length} segmentos
              </p>
            </div>

            <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
              {copyMsg && <span className="copy-badge">{copyMsg}</span>}
              <button
                type="button"
                className="btn-secondary btn-small"
                onClick={handleCopy}
              >
                Copiar Texto
              </button>
              <button
                type="button"
                className="btn-secondary btn-small"
                onClick={handleDownloadTxt}
              >
                Baixar .txt
              </button>
            </div>
          </div>

          <div className="tab-buttons" style={{ marginTop: "1rem", marginBottom: "0.75rem" }}>
            <button
              type="button"
              className={`tab-btn ${activeTab === "text" ? "active" : ""}`}
              onClick={() => setActiveTab("text")}
            >
              Texto Completo
            </button>
            <button
              type="button"
              className={`tab-btn ${activeTab === "segments" ? "active" : ""}`}
              onClick={() => setActiveTab("segments")}
            >
              Segmentos com Timestamps ({result.segments.length})
            </button>
          </div>

          {activeTab === "text" && (
            <div className="transcription-result-box">
              <p style={{ whiteSpace: "pre-wrap", lineHeight: 1.6 }}>{result.text}</p>
            </div>
          )}

          {activeTab === "segments" && (
            <div className="segments-list">
              {result.segments.map((seg, idx) => (
                <div key={idx} className="segment-row">
                  <span className="segment-timestamp">
                    {formatDuration(seg.start_sec)} - {formatDuration(seg.end_sec)}
                  </span>
                  <span className="segment-text">{seg.text}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
