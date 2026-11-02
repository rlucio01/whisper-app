//! Decodificador universal de arquivos de áudio e vídeo (MP3, OGG/Opus/Vorbis, WAV, FLAC, M4A/AAC, MP4, MKV/WebM).
//!
//! Utiliza `symphonia` com suporte a `libopus`, containers ISO MP4 e MKV/WebM
//! para decodificação em Rust, selecionando a primeira faixa de áudio e ignorando faixas de vídeo.
//! Caso o container ou codec seja exótico (ex: AVI, FLV), há fallback transparente via `ffmpeg` se presente.

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

/// Tenta localizar um executável do ffmpeg no sistema como fallback para formatos exóticos.
pub fn find_ffmpeg() -> Option<PathBuf> {
    let mut cmd = std::process::Command::new("ffmpeg");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    if cmd.arg("-version").output().is_ok() {
        return Some(PathBuf::from("ffmpeg"));
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            let candidates = [
                PathBuf::from(&localappdata)
                    .join("Programs")
                    .join("ffmpeg")
                    .join("bin")
                    .join("ffmpeg.exe"),
                PathBuf::from(&localappdata).join("meetily").join("ffmpeg.exe"),
                PathBuf::from(&localappdata)
                    .join("Programs")
                    .join("Stremio")
                    .join("ffmpeg.exe"),
            ];
            for c in &candidates {
                if c.exists() {
                    return Some(c.clone());
                }
            }
        }
    }

    None
}

/// Fallback de decodificação via ffmpeg para 16 kHz mono WAV.
fn decode_with_ffmpeg(path: &Path, ffmpeg_path: &Path) -> Result<DecodedAudio> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let wav_16k_path = std::env::temp_dir().join(format!("whisper_ffmpeg_{}.wav", ts));

    let mut cmd = std::process::Command::new(ffmpeg_path);
    cmd.args([
        "-y",
        "-v",
        "error",
        "-i",
        &path.to_string_lossy(),
        "-vn",
        "-ac",
        "1",
        "-ar",
        "16000",
        "-f",
        "wav",
        &wav_16k_path.to_string_lossy(),
    ]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let status = cmd.status().context("falha ao invocar ffmpeg")?;
    if !status.success() {
        return Err(anyhow!("ffmpeg finalizou com erro ({})", status));
    }

    let mut reader = hound::WavReader::open(&wav_16k_path)
        .with_context(|| format!("falha ao ler WAV gerado pelo ffmpeg em {:?}", wav_16k_path))?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let max_val = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / max_val)
                .collect()
        }
    };

    if samples.is_empty() {
        return Err(anyhow!("Nenhuma amostra de áudio extraída pelo ffmpeg"));
    }

    let duration_sec = samples.len() as f32 / WHISPER_SAMPLE_RATE as f32;
    Ok(DecodedAudio {
        samples,
        duration_sec,
        wav_16k_path,
    })
}

/// Decodificação nativa via Symphonia (puro Rust, sem processos externos).
fn decode_audio_file_symphonia(path: &Path) -> Result<DecodedAudio> {
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
                "Formato de áudio/vídeo não suportado ou arquivo corrompido: {}",
                path.display()
            )
        })?;

    let mut format = probed.format;

    let dec_opts = DecoderOptions::default();

    // Em arquivos de vídeo (MP4, MKV, MOV, WebM), procuramos a primeira faixa
    // compatível com o registro de codecs de áudio (ignorando vídeo, legendas, etc.):
    let (track_id, mut sample_rate, mut channels, mut decoder) = {
        let mut selected = None;
        for track in format.tracks() {
            if let Ok(dec) = get_codec_registry().make(&track.codec_params, &dec_opts) {
                let s_rate = track.codec_params.sample_rate.unwrap_or(48_000);
                let ch = track
                    .codec_params
                    .channels
                    .map(|c| c.count() as u16)
                    .unwrap_or(1);
                selected = Some((track.id, s_rate, ch, dec));
                break;
            }
        }
        selected.ok_or_else(|| {
            anyhow!(
                "Nenhuma faixa de áudio compatível encontrada no arquivo {}",
                path.display()
            )
        })?
    };

    let mut all_interleaved = Vec::<f32>::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            // Ignora faixas de vídeo ou legendas
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

/// Decodifica qualquer arquivo de áudio ou vídeo suportado para mono 16 kHz e gera um WAV temporário.
pub fn decode_audio_file(path: &Path) -> Result<DecodedAudio> {
    if !path.exists() {
        return Err(anyhow!("Arquivo não encontrado: {}", path.display()));
    }

    // Tenta primeiro via Symphonia (puro Rust, in-process, ultrarrápido)
    match decode_audio_file_symphonia(path) {
        Ok(decoded) => Ok(decoded),
        Err(symphonia_err) => {
            // Se falhar e houver ffmpeg disponível, tenta fallback via ffmpeg
            if let Some(ffmpeg) = find_ffmpeg() {
                if let Ok(decoded) = decode_with_ffmpeg(path, &ffmpeg) {
                    return Ok(decoded);
                }
            }
            Err(symphonia_err)
        }
    }
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
