---
name: release
description: Guia e automação do ciclo de release e versionamento semântico (SemVer) multi-plataforma (Windows, Linux, macOS) para o Whisper App.
---

# Guia de Release e Versionamento Multiplataforma (Whisper App)

Este skill define o padrão oficial para versionamento, testes de pré-lançamento, compilação multiplataforma e publicação de releases do **Whisper App**.

---

## 1. Princípios de Versionamento Semântico (SemVer)

O projeto segue estritamente o formato `MAJOR.MINOR.PATCH`:

1. **PATCH (`x.y.Z`)**:
   - **Quando usar**: Correções de bugs (hotfixes/bugfixes), ajustes de compatibilidade, correções de estilos sem adição de novas telas, correções de CI.
   - **Exemplo**: `1.0.1` -> `1.0.2` (ex: fix no codec de áudio WhatsApp, correção de bug de digitação).
2. **MINOR (`x.Y.0`)**:
   - **Quando usar**: Novas funcionalidades ou telas adicionadas de forma retrocompatível (ex: novo Onboarding/Tutorial, suporte a novo provedor de IA, novo sistema de atas ou atalhos).
   - **Regra**: O campo `PATCH` deve ser reiniciado para `0`.
   - **Exemplo**: `1.0.1` -> `1.1.0`.
3. **MAJOR (`X.0.0`)**:
   - **Quando usar**: Mudanças incompatíveis (breaking changes), reescrita do formato de persistência de dados ou quebra de retrocompatibilidade com modelos legados.
   - **Exemplo**: `1.1.0` -> `2.0.0`.

---

## 2. Arquivos de Versão Obrigatórios

Ao realizar um release, a versão **deve ser idêntica** nos seguintes 3 arquivos:

1. [`package.json`](file:///package.json):
   ```json
   "version": "1.1.0"
   ```
2. [`src-tauri/tauri.conf.json`](file:///src-tauri/tauri.conf.json):
   ```json
   "version": "1.1.0"
   ```
3. [`src-tauri/Cargo.toml`](file:///src-tauri/Cargo.toml):
   ```toml
   [package]
   version = "1.1.0"
   ```

Após alterar o `Cargo.toml`, atualize o `Cargo.lock` executando:
```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

---

## 3. Validação Obrigatória Pré-Release

Antes de criar qualquer tag ou commit de release, execute localmente a validação completa:

```powershell
# 1. Testes unitários do Frontend
npm test

# 2. Verificação de tipagem TypeScript e empacotamento Vite
npm run build

# 3. Testes do Backend Rust
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Todos os testes e o build do frontend **devem passar com 100% de sucesso**.

---

## 4. Política: Build Local vs. Build Multiplataforma (CI/CD)

### Por que NÃO subir binários compilados localmente para a Release?
- **Ambiente de Desenvolvimento**: O comando local `.\scripts\dev.ps1` roda em modo Debug (`unoptimized`), apontando para o servidor local do Vite, sem gerar instaladores NSIS/MSI.
- **Multiplataforma Real**: O Whisper App é distribuído para **Windows** (NSIS `.exe` e `.msi`), **Linux** (`.deb` e `.AppImage`) e **macOS** (`.dmg` e `.app.tar.gz`). Uma compilação local gera apenas o binário da máquina do desenvolvedor.
- **Assinatura do Auto-Updater e Integridade**: O GitHub Actions possui o secret `TAURI_SIGNING_PRIVATE_KEY` configurado, garantindo que os instaladores e o manifesto `latest.json` sejam assinados digitalmente com par de chaves oficial em um ambiente limpo e seguro.

> **Regra**: O build local serve exclusivamente para desenvolvimento e validação. A geração oficial dos binários de produção para todas as plataformas deve ser delegada ao workflow `.github/workflows/release.yml`.

---

## 5. Fluxo de Publicação Passo a Passo

### Passo 1: Atualizar os arquivos de versão
Edite `package.json`, `src-tauri/tauri.conf.json` e `src-tauri/Cargo.toml` com a nova versão semântica.

### Passo 2: Registrar o commit
Use mensagens semânticas (Conventional Commits):
```powershell
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock <arquivos alterados>
git commit -m "feat: implementa tutorial de boas-vindas e bump v1.1.0"
```

### Passo 3: Criar a Tag Git correspondente
O padrão de tag deve iniciar obrigatoriamente com `v`:
```powershell
git tag -a v1.1.0 -m "whisper_app v1.1.0"
```

### Passo 4: Enviar commit e tag ao GitHub
```powershell
git push origin main
git push origin v1.1.0
```

### Passo 5: Acompanhar o Workflow de Release
O envio da tag dispara automaticamente o workflow `.github/workflows/release.yml`. Você pode acompanhar via CLI:
```powershell
gh run list --workflow=release.yml
gh run watch
```

Quando o workflow terminar:
- Os instaladores de Windows (`.exe`, `.msi`), Linux (`.deb`, `.AppImage`) e macOS (`.dmg`, `.app.tar.gz`) estarão anexados na Release.
- O arquivo `latest.json` assinado estará disponível, ativando imediatamente o auto-updater para todos os clientes em produção.

---

## 6. Padrão Oficial de Nomenclatura e Release Notes (Anti-Cabeçalho Duplo)

Para manter o histórico e a página de Releases limpa, profissional e uniforme, siga obrigatoriamente este padrão:

### 1. Título da Release (Release Title)
- **Formato obrigatório**: `whisper_app v<versão> - <Título Descritivo>`
- **Exemplo correto**: `whisper_app v1.1.0 - Assistente de Primeiro Acesso e Tutorial de IA`
- **Incorreto**: `v1.1.0` (sem o nome do app e sem descrição) ou apenas a tag crua.

### 2. Corpo do Release (Release Notes) — REGRA CRÍTICA ANTI-CABEÇALHO DUPLO
- **ATENÇÃO**: O GitHub já renderiza o **Título da Release** como um cabeçalho `<h1>` automático no topo da página.
- **PROIBIDO**: **NUNCA** inicie o texto do corpo das notas com `# whisper_app...` ou `# v...`, pois isso gera um **cabeçalho duplo** (o título renderiza repetido em sequência no topo).
- **Estrutura correta do corpo**:
  1. Parágrafo introdutório direto em texto comum (sem `#`).
  2. Linha divisória (`---`).
  3. Seção com `### O que há de novo na vX.Y.Z:`.
  4. Lista estruturada de itens (`- **Recurso**: descrição...`).

### 3. Como Criar ou Atualizar o Título e as Notas via GitHub CLI (`gh`)
Após o término do workflow do GitHub Actions (ou na criação):
```powershell
gh release edit vX.Y.Z --title "whisper_app vX.Y.Z - <Título Amigável>" --notes-file release_notes.md
```

---

## 7. (Opcional) Publicação no WinGet

Após a release ser gerada no GitHub:
```powershell
.\scripts\submit-winget.ps1 -Version "1.1.0"
```
Isso baixa os instaladores recém-lançados, gera os manifestos do Windows Package Manager e abre o Pull Request no repositório `microsoft/winget-pkgs`.
