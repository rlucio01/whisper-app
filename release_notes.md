Esta versão introduz suporte completo à transcrição de arquivos de vídeo locais (MP4, MKV, WebM, MOV, AVI) e transcrição direta via links da web (YouTube, Shorts, TikTok, Instagram Reels, Twitter/X) sem a necessidade de baixar o vídeo manualmente, além de correções críticas na preservação de acentos e emojis em caminhos de arquivos.

---

### O que há de novo na v1.2.0:

- **Transcrição Direta de Links da Web (YouTube, TikTok, Instagram)**:
  - Nova aba "Link da Web" na tela de transcrição para colar links diretos de plataformas de vídeo.
  - Extração inteligente e ultrarrápida apenas da trilha de áudio via `yt-dlp` integrado, poupando espaço em disco e consumo de internet.
  - Canal oficial atualizado para o branch `nightly` com suporte ao resolvedor nativo de desafios de JS do TikTok e auto-recuperação transparente.

- **Suporte Nativo a Arquivos de Vídeo Locais (MP4, MKV, WebM, MOV, AVI)**:
  - Seleção nativa de arquivos expandida para incluir formatos de vídeo populares além de áudios convencionais.
  - Decodificação pura em Rust com seleção explícita da trilha de áudio e descarte de fluxos de vídeo.
  - Suporte ao container Matroska/WebM no backend Symphonia e fallback inteligente via `ffmpeg` para codecs exóticos.
  - Suporte à importação de reuniões gravadas em vídeo (Zoom, Teams, Google Meet em MP4) na aba de Reuniões Inteligentes.

- **Correção de Codificação de Caminhos (Acentos e Emojis)**:
  - Resolução definitiva de falhas "Arquivo não encontrado" ao selecionar arquivos com caracteres acentuados (`á, é, ã, ç`), emojis (`🛜`) ou símbolos especiais através de transferência UTF-8 via Hexadecimal nativo.
