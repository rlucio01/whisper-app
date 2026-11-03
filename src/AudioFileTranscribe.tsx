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

const VIDEO_EXTENSIONS = ["mp4", "mkv", "webm", "mov", "avi", "flv", "m4v"];

function isVideoFile(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() || "";
  return VIDEO_EXTENSIONS.includes(ext);
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
  if (lower.includes("twitter.com") || lower.includes("x.com")) {
    return {
      name: "X / Twitter",
      badgeBg: "rgba(56, 189, 248, 0.15)",
      badgeColor: "#38bdf8",
      badgeBorder: "rgba(56, 189, 248, 0.3)",
    };
  }
  if (lower.startsWith("http://") || lower.startsWith("https://")) {
    return {
      name: "Link de Mídia",
      badgeBg: "rgba(59, 130, 246, 0.15)",
      badgeColor: "#60a5fa",
      badgeBorder: "rgba(59, 130, 246, 0.3)",
    };
  }
  return null;
}

export default function AudioFileTranscribe({
  onBack,
  onSendToMeeting,
}: AudioFileTranscribeProps) {
  const [mode, setMode] = useState<"file" | "url">("file");
  const [engine, setEngine] = useState<"whisper" | "parakeet">("whisper");
  const [filePath, setFilePath] = useState("");
  const [url, setUrl] = useState("");
  const [isProcessing, setIsProcessing] = useState(false);
  const [processingMsg, setProcessingMsg] = useState("");
  const [result, setResult] = useState<TranscriptionFileResult | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [copyMsg, setCopyMsg] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"text" | "segments">("text");

  const platform = detectPlatform(url);

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

  async function handleTranscribeFile() {
    if (!filePath.trim()) return;
    setIsProcessing(true);
    setProcessingMsg("Transcrevendo arquivo...");
    setErrorMsg(null);
    setResult(null);

    try {
      const res = await invoke<TranscriptionFileResult>("transcribe_audio_file", {
        filePath: filePath.trim(),
        engine,
      });
      setResult(res);
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setIsProcessing(false);
      setProcessingMsg("");
    }
  }

  async function handleTranscribeUrl() {
    const cleanUrl = url.trim();
    if (!cleanUrl) return;
    setIsProcessing(true);
    setProcessingMsg("Extraindo áudio do link e transcrevendo...");
    setErrorMsg(null);
    setResult(null);

    try {
      const res = await invoke<TranscriptionFileResult>("transcribe_media_url", {
        url: cleanUrl,
        engine,
      });
      setResult(res);
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setIsProcessing(false);
      setProcessingMsg("");
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
    const blobUrl = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = blobUrl;

    let baseName = "transcricao";
    if (mode === "file" && filePath) {
      baseName = filePath.split(/[\\/]/).pop()?.replace(/\.[^/.]+$/, "") || "transcricao";
    } else if (mode === "url" && url) {
      baseName = platform?.name ? `${platform.name.toLowerCase()}_transcricao` : "web_transcricao";
    }

    a.download = `${baseName}.txt`;
    a.click();
    URL.revokeObjectURL(blobUrl);
  }

  const fileName = filePath.split(/[\\/]/).pop() || "";
  const isVideo = isVideoFile(filePath);
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
        <h2>Transcrever Áudio ou Vídeo</h2>
      </header>

      <div className="settings-card" style={{ marginTop: "1rem" }}>
        {/* Seletor de Modo: Arquivo Local vs Link da Web */}
        <div className="transcribe-mode-tabs">
          <button
            type="button"
            className={`transcribe-mode-btn ${mode === "file" ? "active" : ""}`}
            onClick={() => {
              setMode("file");
              setErrorMsg(null);
            }}
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
            className={`transcribe-mode-btn ${mode === "url" ? "active" : ""}`}
            onClick={() => {
              setMode("url");
              setErrorMsg(null);
            }}
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
            Link da Web (YouTube, TikTok, Instagram)
          </button>
        </div>

        {/* Seletor do Motor: Whisper vs NVIDIA Parakeet */}
        <div style={{ marginBottom: "1.25rem" }}>
          <label className="field-label" style={{ marginBottom: "0.4rem", display: "block", fontSize: "0.82rem" }}>
            Motor de Transcrição:
          </label>
          <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
            <button
              type="button"
              className={`toggle-btn ${engine === "whisper" ? "active" : ""}`}
              onClick={() => setEngine("whisper")}
              style={{ padding: "0.4rem 0.8rem", borderRadius: "6px" }}
            >
              Whisper (Padrão)
            </button>
            <button
              type="button"
              className={`toggle-btn ${engine === "parakeet" ? "active" : ""}`}
              onClick={() => setEngine("parakeet")}
              style={{
                display: "flex",
                alignItems: "center",
                gap: "6px",
                padding: "0.4rem 0.8rem",
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
                Oradores & Timestamps
              </span>
            </button>
          </div>
        </div>

        {/* Modo 1: Arquivo Local */}
        {mode === "file" && (
          <>
            <h3 className="card-title">Selecionar Arquivo de Áudio ou Vídeo</h3>
            <p className="field-hint" style={{ marginTop: "0.25rem", marginBottom: "1rem" }}>
              Suporte nativo a vídeos (<strong>MP4, MKV, WebM, MOV, AVI</strong>) e gravações de áudio (<strong>MP3, WAV, OGG, OPUS/WhatsApp, M4A, FLAC, AAC</strong>).
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
                  {isVideo ? (
                    <span title="Arquivo de Vídeo" style={{ display: "inline-flex", alignItems: "center" }}>
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
                        <polygon points="23 7 16 12 23 17 23 7" />
                        <rect x="1" y="5" width="15" height="14" rx="2" ry="2" />
                      </svg>
                    </span>
                  ) : (
                    <span title="Arquivo de Áudio" style={{ display: "inline-flex", alignItems: "center" }}>
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
                    </span>
                  )}
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
                  onClick={handleTranscribeFile}
                >
                  {isProcessing ? (
                    <>
                      <span className="rec-dot-pulsing" style={{ background: "#fff" }} />
                      {processingMsg || "Transcrevendo..."}
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
          </>
        )}

        {/* Modo 2: Link da Web (YouTube, TikTok, Instagram) */}
        {mode === "url" && (
          <>
            <h3 className="card-title">Transcrever Link de Vídeo ou Áudio</h3>
            <p className="field-hint" style={{ marginTop: "0.25rem", marginBottom: "1rem" }}>
              Transcreva direto de links do <strong>YouTube, Shorts, TikTok, Instagram (Reels), Twitter/X</strong> sem precisar baixar o vídeo previamente.
            </p>

            <div className="file-selection-box">
              <div className="file-path-row">
                <input
                  type="url"
                  className="text-input"
                  placeholder="Cole o link aqui (ex: https://www.youtube.com/watch?v=... ou https://www.instagram.com/reel/...)"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  disabled={isProcessing}
                />
                {url && (
                  <button
                    type="button"
                    className="btn-secondary"
                    onClick={() => {
                      setUrl("");
                      setResult(null);
                    }}
                    disabled={isProcessing}
                    title="Limpar link"
                  >
                    ✕
                  </button>
                )}
              </div>

              {platform && (
                <div style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
                  <span
                    className="platform-badge"
                    style={{
                      background: platform.badgeBg,
                      color: platform.badgeColor,
                      border: `1px solid ${platform.badgeBorder}`,
                    }}
                  >
                    <svg
                      width="12"
                      height="12"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2.5"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    >
                      <polygon points="5 3 19 12 5 21 5 3" />
                    </svg>
                    {platform.name}
                  </span>
                  <span style={{ fontSize: "0.8rem", color: "rgba(255, 255, 255, 0.5)" }}>
                    O app extrai apenas a faixa de áudio em segundos, poupando espaço e internet.
                  </span>
                </div>
              )}

              <div style={{ marginTop: "1rem", display: "flex", gap: "0.75rem", alignItems: "center" }}>
                <button
                  type="button"
                  className="btn-primary"
                  disabled={!url.trim() || isProcessing}
                  onClick={handleTranscribeUrl}
                >
                  {isProcessing ? (
                    <>
                      <span className="rec-dot-pulsing" style={{ background: "#fff" }} />
                      {processingMsg || "Processando link..."}
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
                      Extrair e Transcrever
                    </>
                  )}
                </button>

                {onSendToMeeting && url.trim() && (
                  <button
                    type="button"
                    className="btn-secondary"
                    disabled={isProcessing}
                    onClick={() => onSendToMeeting(url.trim())}
                    title="Processar este link na aba de Reuniões para diarização por orador e ata executiva"
                  >
                    Gerar Ata de Reunião
                  </button>
                )}
              </div>
            </div>
          </>
        )}

        {errorMsg && (
          <div className="error-box" style={{ marginTop: "1.25rem" }}>
            <p className="label">Erro ao processar transcrição:</p>
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
                Duração estimada: <strong>{formatDuration(result.duration_sec)}</strong> • {wordCount} palavras • {(result.segments || []).length} segmentos
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
              Segmentos com Timestamps ({(result.segments || []).length})
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
