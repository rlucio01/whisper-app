//! Decodificador universal de arquivos de áudio (MP3, OGG/Opus/Vorbis, WAV, FLAC, M4A/AAC).
//!
//! Utiliza `symphonia` com suporte a `libopus` (mensagens de voz do WhatsApp)
//! para decodificação em Rust, downmixa para mono e resampleia linearmente
//! para 16 kHz (padrão exigido pelo Whisper).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecRegistry, DecoderOptions};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia_adapter_libopus::OpusDecoder;

pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

static CODEC_REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();

fn get_codec_registry() -> &'static CodecRegistry {
    CODEC_REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_all::<OpusDecoder>();
        registry
    })
}

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
    let mut sample_rate = track
        .codec_params
        .sample_rate
        .unwrap_or(48_000); // 48 kHz padrão no caso do Opus/contêiner sem taxa explícita
    let mut channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(1);

    let dec_opts = DecoderOptions::default();
    let mut decoder = get_codec_registry()
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
                let spec = *decoded.spec();
                sample_rate = spec.rate;
                channels = spec.channels.count() as u16;

                let needed_capacity = decoded.capacity() as u64;
                if sample_buf
                    .as_ref()
                    .map_or(true, |sb| (sb.capacity() as u64) < needed_capacity)
                {
                    sample_buf = Some(SampleBuffer::<f32>::new(needed_capacity, spec));
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

#[cfg(test)]
mod tests {
    use super::*;
    use symphonia::core::codecs::CODEC_TYPE_OPUS;

    #[test]
    fn test_codec_registry_has_opus() {
        let registry = get_codec_registry();
        let dec_opts = DecoderOptions::default();
        let mut params = symphonia::core::codecs::CodecParameters::new();
        params.for_codec(CODEC_TYPE_OPUS);
        params.with_channels(symphonia::core::audio::Channels::FRONT_LEFT);
        let res = registry.make(&params, &dec_opts);
        match res {
            Ok(_) => (),
            Err(symphonia::core::errors::Error::Unsupported(err)) => {
                if err.contains("unsupported codec") {
                    panic!("Opus decoder deve estar registrado no CodecRegistry, mas o codec não foi suportado: {}", err);
                }
            }
            Err(_) => (),
        }
    }
}
