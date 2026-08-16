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

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isOverlay ? <Overlay /> : <App />}</React.StrictMode>,
);
