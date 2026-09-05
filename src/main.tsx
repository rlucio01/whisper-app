import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Overlay from "./Overlay";

// Rotea entre a UI principal e a barra flutuante de indicação com base num
// query param. O Tauri abre a janela "overlay" com `index.html?w=overlay`
// (ver tauri.conf.json), então aqui só decidimos qual componente montar.
const isOverlay =
  new URLSearchParams(window.location.search).get("w") === "overlay";

// Marca o <html> quando estamos no modo overlay. O `overlay.css` usa esse
// atributo pra escopar as regras globais (background transparente, overflow
// hidden, altura 100%) apenas à janela overlay — caso contrário elas vazariam
// pra janela principal (Vite bundla CSS imports globalmente) e quebrariam o
// scroll da tela de configurações.
if (isOverlay) {
  document.documentElement.setAttribute("data-overlay-mode", "");
}

interface ErrorBoundaryProps {
  children: React.ReactNode;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error("[ErrorBoundary]", error, errorInfo);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div style={{ padding: "2rem", color: "#f87171", fontFamily: "sans-serif" }}>
          <h2>Ops! Ocorreu um erro na interface:</h2>
          <pre style={{ background: "#2a2a2a", padding: "1rem", borderRadius: "6px", color: "#fca5a5", overflow: "auto" }}>
            {this.state.error?.message || String(this.state.error)}
          </pre>
          <button
            onClick={() => window.location.reload()}
            style={{ marginTop: "1rem", padding: "0.5rem 1rem", background: "#3b82f6", color: "#fff", border: "none", borderRadius: "4px", cursor: "pointer" }}
          >
            Recarregar Aplicativo
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      {isOverlay ? <Overlay /> : <App />}
    </ErrorBoundary>
  </React.StrictMode>,
);
