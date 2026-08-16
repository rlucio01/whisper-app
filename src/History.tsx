import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface HistoryEntry {
  id: number; // timestamp em ms — também serve de chave única
  raw_text: string;
  formatted_text: string;
}

interface HistoryProps {
  onBack: () => void;
}

export default function History({ onBack }: HistoryProps) {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState("");
  const [message, setMessage] = useState<string | null>(null);

  const refresh = () => {
    invoke<HistoryEntry[]>("get_history")
      .then((list) => {
        setEntries(list);
        setLoading(false);
      })
      .catch((e) => {
        setMessage(`Erro ao carregar histórico: ${e}`);
        setLoading(false);
      });
  };

  useEffect(refresh, []);

  const wordCount = useMemo(
    () =>
      entries.reduce(
        (sum, e) =>
          sum + e.formatted_text.trim().split(/\s+/).filter(Boolean).length,
        0,
      ),
    [entries],
  );

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return entries;
    return entries.filter(
      (e) =>
        e.formatted_text.toLowerCase().includes(q) ||
        e.raw_text.toLowerCase().includes(q),
    );
  }, [entries, search]);

  const groups = useMemo(() => groupByDay(filtered), [filtered]);

  async function copyEntry(e: HistoryEntry) {
    try {
      await navigator.clipboard.writeText(e.formatted_text);
      flash("Copiado.");
    } catch (err) {
      flash(`Falha ao copiar: ${err}`);
    }
  }

  async function repaste(e: HistoryEntry) {
    try {
      await invoke("repaste_text", { text: e.formatted_text });
      flash("Colado no app ativo.");
    } catch (err) {
      flash(`Falha ao colar: ${err}`);
    }
  }

  async function deleteEntry(e: HistoryEntry) {
    // Optimistic — remove da lista antes da confirmação do backend.
    setEntries((prev) => prev.filter((x) => x.id !== e.id));
    try {
      await invoke("delete_history_entry", { id: e.id });
    } catch (err) {
      flash(`Falha ao apagar: ${err}`);
      refresh();
    }
  }

  async function clearAll() {
    if (entries.length === 0) return;
    if (!confirm("Apagar todo o histórico de ditados? Essa ação não pode ser desfeita.")) {
      return;
    }
    setEntries([]);
    try {
      await invoke("clear_history");
    } catch (err) {
      flash(`Falha ao limpar histórico: ${err}`);
      refresh();
    }
  }

  const flashTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  function flash(msg: string) {
    setMessage(msg);
    clearTimeout(flashTimer.current);
    flashTimer.current = setTimeout(() => setMessage(null), 2500);
  }

  return (
    <div className="settings history">
      <header className="settings-header">
        <button className="btn-link" onClick={onBack}>
          ← Voltar
        </button>
        <h2>Histórico</h2>
      </header>

      <section className="history-stat">
        <span className="history-stat-label">Palavras ditadas</span>
        <span className="history-stat-value">{wordCount.toLocaleString("pt-BR")}</span>
      </section>

      <section className="field">
        <input
          type="text"
          className="text-input"
          placeholder="Buscar transcrições…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </section>

      {message && <p className="settings-msg">{message}</p>}

      {loading && <p className="field-hint">Carregando…</p>}

      {!loading && filtered.length === 0 && (
        <p className="field-hint">
          {entries.length === 0
            ? "Nenhum ditado ainda. O histórico aparece aqui depois da primeira transcrição."
            : "Nenhum resultado para essa busca."}
        </p>
      )}

      {!loading &&
        groups.map(([day, dayEntries]) => (
          <section key={day} className="history-group">
            <p className="history-day-label">{day}</p>
            <div className="history-list">
              {dayEntries.map((e) => (
                <div key={e.id} className="history-entry">
                  <div className="history-entry-main">
                    <span className="history-entry-time">{formatTime(e.id)}</span>
                    <p className="history-entry-text">{e.formatted_text}</p>
                  </div>
                  <div className="history-entry-actions">
                    <button
                      className="icon-btn"
                      title="Copiar"
                      aria-label="Copiar"
                      onClick={() => copyEntry(e)}
                    >
                      ⧉
                    </button>
                    <button
                      className="icon-btn"
                      title="Colar de novo no app ativo"
                      aria-label="Colar de novo"
                      onClick={() => repaste(e)}
                    >
                      ↪
                    </button>
                    <button
                      className="icon-btn"
                      title="Apagar"
                      aria-label="Apagar"
                      onClick={() => deleteEntry(e)}
                    >
                      🗑
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </section>
        ))}

      {entries.length > 0 && (
        <section className="actions">
          <button className="btn-secondary" onClick={clearAll}>
            Limpar tudo
          </button>
        </section>
      )}
    </div>
  );
}

function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString("pt-BR", {
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Agrupa entradas por dia (mais recente primeiro), rotulando hoje/ontem. */
function groupByDay(entries: HistoryEntry[]): [string, HistoryEntry[]][] {
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);

  const map = new Map<string, HistoryEntry[]>();
  for (const e of entries) {
    const d = new Date(e.id);
    let label: string;
    if (isSameDay(d, today)) {
      label = "Hoje";
    } else if (isSameDay(d, yesterday)) {
      label = "Ontem";
    } else {
      label = d.toLocaleDateString("pt-BR", {
        day: "2-digit",
        month: "long",
        year: d.getFullYear() !== today.getFullYear() ? "numeric" : undefined,
      });
    }
    if (!map.has(label)) map.set(label, []);
    map.get(label)!.push(e);
  }
  return Array.from(map.entries());
}

function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}
