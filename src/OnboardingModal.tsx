import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { displayHotkey } from "./hotkeyFormat";

interface OnboardingModalProps {
  isOpen: boolean;
  onClose: () => void;
  onComplete: () => void;
  currentHotkey: string;
}

type Step = "welcome" | "choose_provider" | "setup_key";
type ChosenProvider = "groq" | "openai" | "offline";

export default function OnboardingModal({
  isOpen,
  onClose,
  onComplete,
  currentHotkey,
}: OnboardingModalProps) {
  const [step, setStep] = useState<Step>("welcome");
  const [chosenProvider, setChosenProvider] = useState<ChosenProvider>("groq");
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [isTesting, setIsTesting] = useState(false);
  const [testResult, setTestResult] = useState<{
    success: boolean;
    message: string;
  } | null>(null);
  const [isSaving, setIsSaving] = useState(false);

  if (!isOpen) return null;

  async function handleOpenWebsite(url: string) {
    try {
      await openUrl(url);
    } catch {
      window.open(url, "_blank");
    }
  }

  async function handlePasteClipboard() {
    try {
      const text = await navigator.clipboard.readText();
      if (text) {
        setApiKeyInput(text.trim());
        setTestResult(null);
      }
    } catch (err) {
      console.error("Falha ao ler área de transferência:", err);
    }
  }

  async function handleTestKey() {
    if (!apiKeyInput.trim()) {
      setTestResult({
        success: false,
        message: "Por favor, digite ou cole sua chave antes de testar.",
      });
      return;
    }

    setIsTesting(true);
    setTestResult(null);

    try {
      const msg = await invoke<string>("test_api_key", {
        provider: chosenProvider,
        apiKey: apiKeyInput.trim(),
      });
      setTestResult({
        success: true,
        message: msg,
      });
    } catch (err) {
      setTestResult({
        success: false,
        message: String(err),
      });
    } finally {
      setIsTesting(false);
    }
  }

  async function handleSaveAndFinish() {
    setIsSaving(true);
    try {
      // Lê o config atual e atualiza com as opções escolhidas
      const currentConfig = await invoke<Record<string, unknown>>("get_config");

      const updated: Record<string, unknown> = {
        ...currentConfig,
        onboarding_completed: true,
      };

      if (chosenProvider === "groq") {
        updated.provider = "groq";
        if (apiKeyInput.trim()) {
          updated.groq_api_key = apiKeyInput.trim();
        }
      } else if (chosenProvider === "openai") {
        updated.provider = "openai";
        if (apiKeyInput.trim()) {
          updated.openai_api_key = apiKeyInput.trim();
        }
      } else if (chosenProvider === "offline") {
        // No modo offline, não requer IA em nuvem
        updated.skip_llm_formatting = true;
      }

      await invoke("save_config", { config: updated });
      onComplete();
    } catch (err) {
      console.error("Erro ao salvar configuração inicial:", err);
      onComplete();
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <div className="onboarding-overlay" role="dialog" aria-modal="true">
      <div className="onboarding-modal">
        {/* Cabeçalho do modal */}
        <header className="onboarding-header">
          <div className="onboarding-steps-indicator">
            <span className={`step-dot ${step === "welcome" ? "active" : "done"}`}>
              1
            </span>
            <span className="step-line" />
            <span
              className={`step-dot ${
                step === "choose_provider"
                  ? "active"
                  : step === "setup_key"
                  ? "done"
                  : ""
              }`}
            >
              2
            </span>
            <span className="step-line" />
            <span className={`step-dot ${step === "setup_key" ? "active" : ""}`}>
              3
            </span>
          </div>

          <button
            className="icon-btn onboarding-close"
            onClick={onClose}
            title="Fechar tutorial"
            aria-label="Fechar"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </header>

        {/* Passo 1: Como Funciona */}
        {step === "welcome" && (
          <div className="onboarding-body">
            <div className="onboarding-badge-top">Guia Rápido de Início</div>
            <h2 className="onboarding-title">Como usar o Whisper App</h2>
            <p className="onboarding-subtitle">
              Você fala no microfone e o aplicativo digita o texto formatado e pontuado
              onde quer que seu cursor esteja piscando.
            </p>

            <div className="onboarding-features-list">
              <div className="onboarding-feature-item">
                <div className="feature-icon-wrapper">
                  <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
                    <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                    <line x1="12" y1="19" x2="12" y2="22" />
                  </svg>
                </div>
                <div className="feature-text">
                  <strong>1. Segure seu atalho</strong>
                  <span>
                    Pressione e mantenha pressionada a tecla de atalho{" "}
                    <kbd className="onboarding-kbd">{displayHotkey(currentHotkey)}</kbd>{" "}
                    (você pode alterá-la quando quiser).
                  </span>
                </div>
              </div>

              <div className="onboarding-feature-item">
                <div className="feature-icon-wrapper">
                  <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
                  </svg>
                </div>
                <div className="feature-text">
                  <strong>2. Fale naturalmente</strong>
                  <span>
                    Fale em qualquer programa aberto (WhatsApp, Microsoft Word,
                    Google Docs, e-mail, bloco de notas, etc.).
                  </span>
                </div>
              </div>

              <div className="onboarding-feature-item">
                <div className="feature-icon-wrapper">
                  <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M12 20h9" />
                    <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z" />
                  </svg>
                </div>
                <div className="feature-text">
                  <strong>3. Solte para digitar</strong>
                  <span>
                    Ao soltar a tecla, a IA ajusta a pontuação, remove repetições e
                    digita a mensagem automaticamente para você.
                  </span>
                </div>
              </div>
            </div>

            <div className="onboarding-actions">
              <button
                type="button"
                className="btn-link"
                onClick={onClose}
              >
                Pular por enquanto
              </button>
              <button
                type="button"
                className="btn-primary onboarding-btn-next"
                onClick={() => setStep("choose_provider")}
              >
                Avançar: Configurar a IA
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="9 18 15 12 9 6" />
                </svg>
              </button>
            </div>
          </div>
        )}

        {/* Passo 2: Escolha de IA */}
        {step === "choose_provider" && (
          <div className="onboarding-body">
            <h2 className="onboarding-title">Como deseja conectar a IA?</h2>
            <p className="onboarding-subtitle">
              A inteligência artificial é quem coloca pontos, vírgulas, parágrafos e
              corrige gaguejos. Escolha a opção ideal para você:
            </p>

            <div className="onboarding-options-grid">
              {/* Opção Groq */}
              <div
                className={`onboarding-card-option ${
                  chosenProvider === "groq" ? "selected" : ""
                }`}
                onClick={() => setChosenProvider("groq")}
              >
                <div className="option-header">
                  <div className="option-title-group">
                    <span className="option-name">Groq</span>
                    <span className="option-badge-free">100% Gratuito • Recomendado</span>
                  </div>
                  <input
                    type="radio"
                    name="provider_choice"
                    checked={chosenProvider === "groq"}
                    onChange={() => setChosenProvider("groq")}
                  />
                </div>
                <p className="option-desc">
                  Melhor opção para começar. Rápida (menos de 1 segundo), gratuita e
                  <strong> sem pedir cartão de crédito</strong>. Permite até 14.400
                  requisições por dia de graça.
                </p>
              </div>

              {/* Opção OpenAI */}
              <div
                className={`onboarding-card-option ${
                  chosenProvider === "openai" ? "selected" : ""
                }`}
                onClick={() => setChosenProvider("openai")}
              >
                <div className="option-header">
                  <div className="option-title-group">
                    <span className="option-name">OpenAI (ChatGPT)</span>
                    <span className="option-badge-neutral">Conta OpenAI</span>
                  </div>
                  <input
                    type="radio"
                    name="provider_choice"
                    checked={chosenProvider === "openai"}
                    onChange={() => setChosenProvider("openai")}
                  />
                </div>
                <p className="option-desc">
                  Ideal se você já possui conta com créditos pagos na OpenAI e deseja
                  usar modelos como o <code>gpt-4o-mini</code>.
                </p>
              </div>

              {/* Opção Modo Offline */}
              <div
                className={`onboarding-card-option ${
                  chosenProvider === "offline" ? "selected" : ""
                }`}
                onClick={() => setChosenProvider("offline")}
              >
                <div className="option-header">
                  <div className="option-title-group">
                    <span className="option-name">Modo 100% Offline (Sem Nuvem)</span>
                    <span className="option-badge-neutral">Privacidade Total</span>
                  </div>
                  <input
                    type="radio"
                    name="provider_choice"
                    checked={chosenProvider === "offline"}
                    onChange={() => setChosenProvider("offline")}
                  />
                </div>
                <p className="option-desc">
                  Roda tudo localmente no seu computador. Não precisa de cadastro,
                  chaves nem internet, transcrevendo o que você fala diretamente.
                </p>
              </div>
            </div>

            <div className="onboarding-actions">
              <button
                type="button"
                className="btn-secondary"
                onClick={() => setStep("welcome")}
              >
                Voltar
              </button>
              <button
                type="button"
                className="btn-primary onboarding-btn-next"
                onClick={() => setStep("setup_key")}
              >
                Continuar
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="9 18 15 12 9 6" />
                </svg>
              </button>
            </div>
          </div>
        )}

        {/* Passo 3: Configurar Chave & Testar */}
        {step === "setup_key" && (
          <div className="onboarding-body">
            {chosenProvider === "offline" ? (
              <div className="offline-setup-box">
                <div className="offline-icon-circle">
                  <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="#22c55e" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                </div>
                <h3 className="onboarding-title">Tudo Pronto para Começar!</h3>
                <p className="onboarding-subtitle" style={{ maxWidth: "480px", margin: "0 auto 1.5rem" }}>
                  O Whisper App funcionará de forma independente no seu computador.
                  Você pode falar a qualquer momento segurando seu atalho.
                </p>
                <div className="offline-hint-card">
                  💡 <strong>Dica:</strong> Se no futuro você quiser ativar a IA gratuita
                  do Groq para pontuações e parágrafos perfeitos, basta clicar no botão
                  <strong> "Primeiros Passos"</strong> no topo da tela inicial.
                </div>
              </div>
            ) : (
              <>
                <h2 className="onboarding-title">
                  {chosenProvider === "groq"
                    ? "Como pegar sua chave gratuita do Groq"
                    : "Conectar sua chave da OpenAI"}
                </h2>
                <p className="onboarding-subtitle">
                  Siga os 3 passos simples abaixo. Leva menos de 1 minuto:
                </p>

                <div className="wizard-instructions-card">
                  <div className="wizard-step-row">
                    <span className="wizard-step-num">1</span>
                    <div className="wizard-step-content">
                      <span>Clique no botão para abrir o site oficial:</span>
                      <button
                        type="button"
                        className="btn-secondary btn-small"
                        style={{ marginTop: "0.4rem", display: "inline-flex", gap: "0.4rem" }}
                        onClick={() =>
                          handleOpenWebsite(
                            chosenProvider === "groq"
                              ? "https://console.groq.com/keys"
                              : "https://platform.openai.com/api-keys"
                          )
                        }
                      >
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                          <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
                          <polyline points="15 3 21 3 21 9" />
                          <line x1="10" y1="14" x2="21" y2="3" />
                        </svg>
                        {chosenProvider === "groq"
                          ? "Abrir página do Groq (console.groq.com)"
                          : "Abrir página da OpenAI (platform.openai.com)"}
                      </button>
                    </div>
                  </div>

                  <div className="wizard-step-row">
                    <span className="wizard-step-num">2</span>
                    <div className="wizard-step-content">
                      {chosenProvider === "groq" ? (
                        <span>
                          Faça login com seu Google (botão <em>"Continue with Google"</em>),
                          depois clique no botão <strong>"Create API Key"</strong>.
                        </span>
                      ) : (
                        <span>
                          Faça login na sua conta e clique no botão{" "}
                          <strong>"Create new secret key"</strong>.
                        </span>
                      )}
                    </div>
                  </div>

                  <div className="wizard-step-row">
                    <span className="wizard-step-num">3</span>
                    <div className="wizard-step-content">
                      <span>
                        Dê qualquer nome (ex: <code>Whisper</code>), copie o código que
                        começa com{" "}
                        <code>{chosenProvider === "groq" ? "gsk_" : "sk-"}</code> e cole
                        no campo abaixo:
                      </span>
                    </div>
                  </div>
                </div>

                <div className="wizard-key-input-box">
                  <div className="file-path-row">
                    <input
                      type="password"
                      className="text-input"
                      placeholder={
                        chosenProvider === "groq"
                          ? "Cole sua chave gsk_... aqui"
                          : "Cole sua chave sk-... aqui"
                      }
                      value={apiKeyInput}
                      onChange={(e) => {
                        setApiKeyInput(e.target.value);
                        setTestResult(null);
                      }}
                    />
                    <button
                      type="button"
                      className="btn-secondary"
                      onClick={handlePasteClipboard}
                      title="Colar texto copiado"
                    >
                      Colar
                    </button>
                    <button
                      type="button"
                      className="btn-secondary"
                      onClick={handleTestKey}
                      disabled={isTesting || !apiKeyInput.trim()}
                    >
                      {isTesting ? "Testando…" : "Testar Chave"}
                    </button>
                  </div>

                  {testResult && (
                    <div
                      className={`wizard-test-badge ${
                        testResult.success ? "success" : "error"
                      }`}
                    >
                      {testResult.success ? (
                        <>
                          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                            <polyline points="20 6 9 17 4 12" />
                          </svg>
                          <span>{testResult.message}</span>
                        </>
                      ) : (
                        <>
                          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                            <circle cx="12" cy="12" r="10" />
                            <line x1="12" y1="8" x2="12" y2="12" />
                            <line x1="12" y1="16" x2="12.01" y2="16" />
                          </svg>
                          <span>{testResult.message}</span>
                        </>
                      )}
                    </div>
                  )}
                </div>
              </>
            )}

            <div className="onboarding-actions">
              <button
                type="button"
                className="btn-secondary"
                onClick={() => setStep("choose_provider")}
                disabled={isSaving}
              >
                Voltar
              </button>
              <button
                type="button"
                className="btn-primary"
                onClick={handleSaveAndFinish}
                disabled={isSaving}
              >
                {isSaving ? "Salvando…" : "Salvar e Concluir"}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
