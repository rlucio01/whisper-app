//! Binding FFI dinâmico para `sherpa-onnx-c-api.dll` (v2.0.1) no Windows.
//!
//! Evita abrir um subprocesso CLI e gravar WAV temporário a cada ditado:
//! o modelo fica residente em memória enquanto em uso e é descarregado
//! automaticamente após um curto período de inatividade (drop + `FreeLibrary`
//! + trim do working set) para manter a RAM em idle abaixo de ~10 MB.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_float, c_int};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};

// ---------- Structs C exatamente compatíveis com sherpa-onnx v2.0.1 c-api.h ----------

#[repr(C)]
#[derive(Default)]
struct SherpaOnnxFeatureConfig {
    sample_rate: i32,
    feature_dim: i32,
}

#[repr(C)]
struct SherpaOnnxHomophoneReplacerConfig {
    dict_dir: *const c_char,
    lexicon: *const c_char,
    rule_fsts: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineCtcFstDecoderConfig {
    graph: *const c_char,
    max_active: i32,
}

#[repr(C)]
struct SherpaOnnxOfflineTransducerModelConfig {
    encoder: *const c_char,
    decoder: *const c_char,
    joiner: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineParaformerModelConfig {
    model: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineNemoEncDecCtcModelConfig {
    model: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineWhisperModelConfig {
    encoder: *const c_char,
    decoder: *const c_char,
    language: *const c_char,
    task: *const c_char,
    tail_paddings: i32,
    enable_token_timestamps: i32,
    enable_segment_timestamps: i32,
}

#[repr(C)]
struct SherpaOnnxOfflineTdnnModelConfig {
    model: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineSenseVoiceModelConfig {
    model: *const c_char,
    language: *const c_char,
    use_itn: i32,
}

#[repr(C)]
struct SherpaOnnxOfflineMoonshineModelConfig {
    preprocessor: *const c_char,
    encoder: *const c_char,
    uncached_decoder: *const c_char,
    cached_decoder: *const c_char,
    merged_decoder: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineFireRedAsrModelConfig {
    encoder: *const c_char,
    decoder: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineDolphinModelConfig {
    model: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineZipformerCtcModelConfig {
    model: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineCanaryModelConfig {
    encoder: *const c_char,
    decoder: *const c_char,
    src_lang: *const c_char,
    tgt_lang: *const c_char,
    use_pnc: i32,
}

#[repr(C)]
struct SherpaOnnxOfflineWenetCtcModelConfig {
    model: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineOmnilingualAsrCtcModelConfig {
    model: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineMedAsrCtcModelConfig {
    model: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineFunASRNanoModelConfig {
    encoder_adaptor: *const c_char,
    llm: *const c_char,
    embedding: *const c_char,
    tokenizer: *const c_char,
    system_prompt: *const c_char,
    user_prompt: *const c_char,
    max_new_tokens: i32,
    temperature: c_float,
    top_p: c_float,
    seed: i32,
    language: *const c_char,
    itn: i32,
    hotwords: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineFireRedAsrCtcModelConfig {
    model: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineQwen3ASRModelConfig {
    conv_frontend: *const c_char,
    encoder: *const c_char,
    decoder: *const c_char,
    tokenizer: *const c_char,
    max_total_len: i32,
    max_new_tokens: i32,
    temperature: c_float,
    top_p: c_float,
    seed: i32,
    hotwords: *const c_char,
}

#[repr(C)]
struct SherpaOnnxOfflineCohereTranscribeModelConfig {
    encoder: *const c_char,
    decoder: *const c_char,
    language: *const c_char,
    use_punct: i32,
    use_itn: i32,
}

#[repr(C)]
struct SherpaOnnxOfflineModelConfig {
    transducer: SherpaOnnxOfflineTransducerModelConfig,
    paraformer: SherpaOnnxOfflineParaformerModelConfig,
    nemo_ctc: SherpaOnnxOfflineNemoEncDecCtcModelConfig,
    whisper: SherpaOnnxOfflineWhisperModelConfig,
    tdnn: SherpaOnnxOfflineTdnnModelConfig,
    tokens: *const c_char,
    num_threads: i32,
    debug: i32,
    provider: *const c_char,
    model_type: *const c_char,
    modeling_unit: *const c_char,
    bpe_vocab: *const c_char,
    telespeech_ctc: *const c_char,
    sense_voice: SherpaOnnxOfflineSenseVoiceModelConfig,
    moonshine: SherpaOnnxOfflineMoonshineModelConfig,
    fire_red_asr: SherpaOnnxOfflineFireRedAsrModelConfig,
    dolphin: SherpaOnnxOfflineDolphinModelConfig,
    zipformer_ctc: SherpaOnnxOfflineZipformerCtcModelConfig,
    canary: SherpaOnnxOfflineCanaryModelConfig,
    wenet_ctc: SherpaOnnxOfflineWenetCtcModelConfig,
    omnilingual: SherpaOnnxOfflineOmnilingualAsrCtcModelConfig,
    medasr: SherpaOnnxOfflineMedAsrCtcModelConfig,
    funasr_nano: SherpaOnnxOfflineFunASRNanoModelConfig,
    fire_red_asr_ctc: SherpaOnnxOfflineFireRedAsrCtcModelConfig,
    qwen3_asr: SherpaOnnxOfflineQwen3ASRModelConfig,
    cohere_transcribe: SherpaOnnxOfflineCohereTranscribeModelConfig,
}

#[repr(C)]
struct SherpaOnnxOfflineLMConfig {
    model: *const c_char,
    scale: c_float,
}

#[repr(C)]
struct SherpaOnnxOfflineRecognizerConfig {
    feat_config: SherpaOnnxFeatureConfig,
    model_config: SherpaOnnxOfflineModelConfig,
    lm_config: SherpaOnnxOfflineLMConfig,
    decoding_method: *const c_char,
    max_active_paths: i32,
    hotwords_file: *const c_char,
    hotwords_score: c_float,
    rule_fsts: *const c_char,
    rule_fars: *const c_char,
    blank_penalty: c_float,
    hr: SherpaOnnxHomophoneReplacerConfig,
    ctc_fst_decoder_config: SherpaOnnxOfflineCtcFstDecoderConfig,
}

#[repr(C)]
struct SherpaOnnxOfflineRecognizer {
    _private: [u8; 0],
}

#[repr(C)]
struct SherpaOnnxOfflineStream {
    _private: [u8; 0],
}

#[repr(C)]
struct SherpaOnnxOfflineRecognizerResult {
    text: *const c_char,
    timestamps: *const c_float,
    count: i32,
    tokens: *const c_char,
    tokens_arr: *const *const c_char,
    json: *const c_char,
    lang: *const c_char,
    emotion: *const c_char,
    event: *const c_char,
    durations: *const c_float,
    ys_log_probs: *const c_float,
    segment_timestamps: *const c_float,
    segment_durations: *const c_float,
    segment_texts: *const c_char,
    segment_texts_arr: *const *const c_char,
    segment_count: i32,
}

type FnCreateOfflineRecognizer =
    unsafe extern "C" fn(*const SherpaOnnxOfflineRecognizerConfig) -> *const SherpaOnnxOfflineRecognizer;
type FnDestroyOfflineRecognizer = unsafe extern "C" fn(*const SherpaOnnxOfflineRecognizer);
type FnCreateOfflineStream =
    unsafe extern "C" fn(*const SherpaOnnxOfflineRecognizer) -> *const SherpaOnnxOfflineStream;
type FnDestroyOfflineStream = unsafe extern "C" fn(*const SherpaOnnxOfflineStream);
type FnAcceptWaveformOffline =
    unsafe extern "C" fn(*const SherpaOnnxOfflineStream, c_int, *const c_float, c_int);
type FnDecodeOfflineStream =
    unsafe extern "C" fn(*const SherpaOnnxOfflineRecognizer, *const SherpaOnnxOfflineStream);
type FnGetOfflineStreamResult =
    unsafe extern "C" fn(*const SherpaOnnxOfflineStream) -> *const SherpaOnnxOfflineRecognizerResult;
type FnDestroyOfflineRecognizerResult =
    unsafe extern "C" fn(*const SherpaOnnxOfflineRecognizerResult);

unsafe fn load_sym<T>(module: HMODULE, name: &[u8]) -> Result<T> {
    let addr = GetProcAddress(module, name.as_ptr());
    if addr.is_none() {
        let name_str = std::str::from_utf8(name).unwrap_or("unknown");
        return Err(anyhow!("símbolo não encontrado em sherpa-onnx-c-api.dll: {}", name_str));
    }
    Ok(std::mem::transmute_copy(&addr))
}

/// Instância carregada em memória do recognizer Parakeet-TDT via `sherpa-onnx-c-api.dll`.
pub struct ParakeetEngine {
    module: HMODULE,
    recognizer: *const SherpaOnnxOfflineRecognizer,
    destroy_recognizer: FnDestroyOfflineRecognizer,
    create_stream: FnCreateOfflineStream,
    destroy_stream: FnDestroyOfflineStream,
    accept_waveform: FnAcceptWaveformOffline,
    decode_stream: FnDecodeOfflineStream,
    get_result: FnGetOfflineStreamResult,
    destroy_result: FnDestroyOfflineRecognizerResult,
}

// O recognizer do sherpa-onnx é usado exclusivamente pela thread dedicada de transcrição.
unsafe impl Send for ParakeetEngine {}

impl ParakeetEngine {
    pub fn load(dll_path: &Path, model_dir: &Path, num_threads: i32) -> Result<Self> {
        let wide: Vec<u16> = dll_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let module = unsafe {
            LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH)
        };
        if module.is_null() {
            return Err(anyhow!(
                "falha ao carregar {} via LoadLibraryExW",
                dll_path.display()
            ));
        }

        let res = (|| -> Result<Self> {
            let create_recognizer: FnCreateOfflineRecognizer =
                unsafe { load_sym(module, b"SherpaOnnxCreateOfflineRecognizer\0")? };
            let destroy_recognizer: FnDestroyOfflineRecognizer =
                unsafe { load_sym(module, b"SherpaOnnxDestroyOfflineRecognizer\0")? };
            let create_stream: FnCreateOfflineStream =
                unsafe { load_sym(module, b"SherpaOnnxCreateOfflineStream\0")? };
            let destroy_stream: FnDestroyOfflineStream =
                unsafe { load_sym(module, b"SherpaOnnxDestroyOfflineStream\0")? };
            let accept_waveform: FnAcceptWaveformOffline =
                unsafe { load_sym(module, b"SherpaOnnxAcceptWaveformOffline\0")? };
            let decode_stream: FnDecodeOfflineStream =
                unsafe { load_sym(module, b"SherpaOnnxDecodeOfflineStream\0")? };
            let get_result: FnGetOfflineStreamResult =
                unsafe { load_sym(module, b"SherpaOnnxGetOfflineStreamResult\0")? };
            let destroy_result: FnDestroyOfflineRecognizerResult =
                unsafe { load_sym(module, b"SherpaOnnxDestroyOfflineRecognizerResult\0")? };

            let c_encoder = CString::new(model_dir.join("encoder.int8.onnx").to_string_lossy().as_ref())
                .context("caminho encoder inválido")?;
            let c_decoder = CString::new(model_dir.join("decoder.int8.onnx").to_string_lossy().as_ref())
                .context("caminho decoder inválido")?;
            let c_joiner = CString::new(model_dir.join("joiner.int8.onnx").to_string_lossy().as_ref())
                .context("caminho joiner inválido")?;
            let c_tokens = CString::new(model_dir.join("tokens.txt").to_string_lossy().as_ref())
                .context("caminho tokens inválido")?;
            let c_provider = CString::new("cpu").unwrap();
            let c_model_type = CString::new("nemo_transducer").unwrap();
            let c_decoding = CString::new("greedy_search").unwrap();
            let c_empty = CString::new("").unwrap();

            let mut config: SherpaOnnxOfflineRecognizerConfig = unsafe { std::mem::zeroed() };
            config.feat_config.sample_rate = 16_000;
            config.feat_config.feature_dim = 80;
            config.model_config.transducer.encoder = c_encoder.as_ptr();
            config.model_config.transducer.decoder = c_decoder.as_ptr();
            config.model_config.transducer.joiner = c_joiner.as_ptr();
            config.model_config.tokens = c_tokens.as_ptr();
            config.model_config.num_threads = num_threads;
            config.model_config.debug = 0;
            config.model_config.provider = c_provider.as_ptr();
            config.model_config.model_type = c_model_type.as_ptr();
            config.decoding_method = c_decoding.as_ptr();
            config.max_active_paths = 4;
            config.hotwords_file = c_empty.as_ptr();
            config.hotwords_score = 1.5;
            config.rule_fsts = c_empty.as_ptr();
            config.rule_fars = c_empty.as_ptr();

            let recognizer = unsafe { create_recognizer(&config) };
            if recognizer.is_null() {
                return Err(anyhow!("SherpaOnnxCreateOfflineRecognizer retornou nulo"));
            }

            Ok(Self {
                module,
                recognizer,
                destroy_recognizer,
                create_stream,
                destroy_stream,
                accept_waveform,
                decode_stream,
                get_result,
                destroy_result,
            })
        })();

        if res.is_err() {
            unsafe {
                FreeLibrary(module);
            }
        }
        res
    }

    pub fn transcribe_samples(&self, sample_rate: i32, samples: &[f32]) -> Result<String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        unsafe {
            let stream = (self.create_stream)(self.recognizer);
            if stream.is_null() {
                return Err(anyhow!("SherpaOnnxCreateOfflineStream retornou nulo"));
            }
            (self.accept_waveform)(stream, sample_rate, samples.as_ptr(), samples.len() as i32);
            (self.decode_stream)(self.recognizer, stream);
            let result = (self.get_result)(stream);
            let text = if !result.is_null() && !(*result).text.is_null() {
                CStr::from_ptr((*result).text).to_string_lossy().into_owned()
            } else {
                String::new()
            };
            if !result.is_null() {
                (self.destroy_result)(result);
            }
            (self.destroy_stream)(stream);
            Ok(text)
        }
    }
}

impl Drop for ParakeetEngine {
    fn drop(&mut self) {
        unsafe {
            if !self.recognizer.is_null() {
                (self.destroy_recognizer)(self.recognizer);
            }
            if !self.module.is_null() {
                FreeLibrary(self.module);
            }
            // Devolve as páginas físicas do working set ao SO para manter RAM em idle baixa.
            SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
        }
    }
}
