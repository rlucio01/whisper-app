//! Decodificador universal de arquivos de áudio (MP3, OGG, WAV, FLAC, M4A/AAC).
//!
//! Utiliza `symphonia` para decodificação pura em Rust, downmixa para mono
//! e resampleia linearmente para 16 kHz (padrão exigido pelo Whisper).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub duration_sec: f32,
    pub wav_16k_path: PathBuf,
}

/// Decodifica qualquer arquivo de áudio suportado para mono 16 kHz e gera um WAV temporário.
pub fn decode_audio_file(path: &Path) -> Result<DecodedAudio> {
    if !path.exists() {
        return Err(anyhow!("Arquivo não encontrado: {}", path.display()));
    }

    let file = File::open(path)
        .with_context(|| format!("falha ao abrir arquivo de áudio em {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let meta_opts = MetadataOptions::default();
    let fmt_opts = FormatOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &fmt_opts, &meta_opts)
        .with_context(|| {
            format!(
                "Formato de áudio não suportado ou arquivo corrompido: {}",
                path.display()
            )
        })?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| anyhow!("Nenhuma faixa de áudio encontrada no arquivo {}", path.display()))?;

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("Taxa de amostragem desconhecida no arquivo de áudio"))?;
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(1);

    let dec_opts = DecoderOptions::default();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .with_context(|| "Codec de áudio não suportado ou inválido")?;

    let mut all_interleaved = Vec::<f32>::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                if sample_buf.is_none() {
                    let spec = *decoded.spec();
                    let duration = decoded.capacity() as u64;
                    sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
                }

                if let Some(sb) = sample_buf.as_mut() {
                    sb.copy_interleaved_ref(decoded);
                    all_interleaved.extend_from_slice(sb.samples());
                }
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => {
                // Quadros corrompidos isolados são ignorados para resiliência
                continue;
            }
            Err(symphonia::core::errors::Error::IoError(_)) => {
                break;
            }
            Err(e) => {
                return Err(anyhow!("Erro ao decodificar amostras de áudio: {}", e));
            }
        }
    }

    if all_interleaved.is_empty() {
        return Err(anyhow!(
            "Nenhuma amostra de áudio válida pôde ser extraída do arquivo."
        ));
    }

    // Downmix para mono
    let mono_samples = crate::transcription::downmix_to_mono(&all_interleaved, channels);

    // Resample linear para 16 kHz se necessário
    let resampled = if sample_rate == WHISPER_SAMPLE_RATE {
        mono_samples
    } else {
        crate::transcription::linear_resample(&mono_samples, sample_rate, WHISPER_SAMPLE_RATE)
    };

    let duration_sec = resampled.len() as f32 / WHISPER_SAMPLE_RATE as f32;

    // Salva arquivo temporário WAV 16kHz mono para alimentação dos motores Whisper
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let wav_16k_path = std::env::temp_dir().join(format!("whisper_decoded_{}.wav", ts));

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: WHISPER_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&wav_16k_path, spec)
        .with_context(|| format!("falha ao criar WAV em {:?}", wav_16k_path))?;

    for &f in &resampled {
        let clamped = f.clamp(-1.0, 1.0);
        let s = (clamped * i16::MAX as f32) as i16;
        writer.write_sample(s)?;
    }
    writer.finalize().context("falha ao finalizar WAV decodificado")?;

    Ok(DecodedAudio {
        samples: resampled,
        duration_sec,
        wav_16k_path,
    })
}
