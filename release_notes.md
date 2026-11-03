Esta versão introduz o suporte completo ao modelo de última geração NVIDIA Parakeet-TDT 0.6B v3 para diarização acústica avançada de oradores, processamento direto de reuniões a partir de links da web (YouTube, Teams, Zoom, Drive, Loom), refinamento inteligente de atas com IA sem necessidade de regravação e melhorias substanciais na extração de mídias online.

---

### O que há de novo na v1.3.0:

- **Motor NVIDIA Parakeet-TDT 0.6B v3 (Diarização de Oradores Offline)**:
  - Integração do modelo FastConformer da NVIDIA com arquitetura Token-and-Duration Transducer (TDT) e runtime ONNX (`sherpa-onnx`).
  - Identificação de mudanças acústicas de orador via token especial (`<|spkchange|>`) e alinhamento temporal de alta precisão em 25 idiomas.
  - Seletor do motor de reconhecimento (Whisper vs NVIDIA Parakeet) disponível nas telas de Transcrição e Reuniões Inteligentes.
  - Gerenciamento de download transparente e monitoramento de progresso diretamente nas Configurações.

- **Processamento de Reuniões a partir de Links da Web**:
  - Nova aba "Link da Web" na tela de Reuniões para colar URLs do YouTube, Google Drive, Microsoft Teams, Zoom ou Loom.
  - Extração imediata apenas do fluxo de áudio diretamente da nuvem, gerando a ata estruturada com resumo executivo, decisões e plano de ações em segundos.
  - Botão de atalho "Gerar Ata de Reunião" também na tela de transcrição rápida de mídias online.

- **Refinamento de Oradores com IA (Sem Regravar)**:
  - Novo botão "Refinar Oradores" na visualização de atas salvas para reanalisar dinâmicas de conversas e alternâncias de falas com novos contextos.
  - Chips de contexto de 1 clique ("Palco com 3 oradores", "Entrevista", "Reunião de equipe") para guiar a IA na identificação precisa de perguntas, respostas e intervenções.
  - Segmentação fina de áudio baseada em pontuações interrogativas/exclamativas e pausas acústicas.

- **Resiliência e Correção no Download de Mídias Web**:
  - Simulação de cabeçalho de navegador Desktop moderno (Chrome) e retentativas automáticas no `yt-dlp` para evitar bloqueios antibot no TikTok e plataformas similares.
  - Preservação estrita de caminhos de arquivos contendo acentos, caracteres estendidos e emojis via codificação UTF-8 hexadecimal.
