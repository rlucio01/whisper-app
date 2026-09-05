//! Detecção de GPU e recursos de hardware para otimização de inferência local.
//!
//! No Windows, utiliza a API nativa DXGI para enumerar adaptadores gráficos,
//! identificar placas de vídeo dedicadas (NVIDIA, AMD, Intel Arc), quantificar
//! a VRAM dedicada e recomendar automaticamente o modo ideal (GPU vs CPU).

use serde::{Deserialize, Serialize};

/// Informações de um adaptador gráfico detectado no sistema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    /// Nome amigável da GPU (ex: "NVIDIA GeForce RTX 4070 Laptop GPU").
    pub name: String,
    /// Fabricante identificado (NVIDIA, AMD, Intel, etc.).
    pub vendor: String,
    /// Memória de vídeo dedicada em Megabytes (VRAM).
    pub vram_mb: u64,
    /// Memória de sistema compartilhada com a GPU em Megabytes.
    pub shared_ram_mb: u64,
    /// Indica se é uma GPU dedicada com VRAM própria substancial.
    pub is_discrete: bool,
    /// Mensagem amigável com a recomendação para este adaptador.
    pub recommendation: String,
}

/// Relatório consolidado do hardware do computador.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareReport {
    /// Lista de todas as GPUs físicas detectadas.
    pub gpus: Vec<GpuInfo>,
    /// GPU principal recomendada para aceleração (se houver).
    pub primary_gpu: Option<GpuInfo>,
    /// Dispositivo recomendado automaticamente ("gpu" ou "cpu").
    pub recommended_device: String,
    /// Quantidade de núcleos lógicos de CPU detectados.
    pub cpu_cores: usize,
    /// Indica se a CPU suporta instruções AVX para o Whisper local.
    pub has_avx: bool,
}

/// Detecta as GPUs e capacidades do computador atual.
pub fn detect_hardware() -> HardwareReport {
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    #[cfg(target_arch = "x86_64")]
    let has_avx = std::is_x86_feature_detected!("avx");
    #[cfg(not(target_arch = "x86_64"))]
    let has_avx = true;

    let gpus = detect_gpus_dxgi();

    // Seleciona como primária a GPU dedicada com maior VRAM, ou a primeira encontrada
    let primary_gpu = gpus
        .iter()
        .filter(|g| g.is_discrete)
        .max_by_key(|g| g.vram_mb)
        .cloned()
        .or_else(|| gpus.first().cloned());

    let recommended_device = if let Some(ref gpu) = primary_gpu {
        if gpu.is_discrete && gpu.vram_mb >= 1024 {
            "gpu".to_string()
        } else {
            "cpu".to_string()
        }
    } else {
        "cpu".to_string()
    };

    HardwareReport {
        gpus,
        primary_gpu,
        recommended_device,
        cpu_cores,
        has_avx,
    }
}

#[cfg(target_os = "windows")]
fn detect_gpus_dxgi() -> Vec<GpuInfo> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

    let mut result = Vec::new();

    let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[hardware] Falha ao criar DXGIFactory1: {}", e);
            return result;
        }
    };

    let mut adapter_index = 0u32;
    loop {
        let adapter = match unsafe { factory.EnumAdapters1(adapter_index) } {
            Ok(a) => a,
            Err(_) => break, // Fim da enumeração
        };
        adapter_index += 1;

        let desc = match unsafe { adapter.GetDesc1() } {
            Ok(d) => d,
            Err(_) => continue,
        };

        // Ignora adaptadores de software como o Microsoft Basic Render Driver
        // DXGI_ADAPTER_FLAG_SOFTWARE tem valor 2
        let flags = desc.Flags;
        if (flags & 2) != 0 {
            continue;
        }

        let name_len = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..name_len])
            .trim()
            .to_string();

        if name.is_empty() || name.contains("Basic Render") {
            continue;
        }

        let vendor = match desc.VendorId {
            0x10DE => "NVIDIA",
            0x1002 | 0x1022 => "AMD",
            0x8086 => "Intel",
            0x1414 => "Microsoft",
            _ => "Outro",
        }
        .to_string();

        let vram_mb = (desc.DedicatedVideoMemory as u64) / (1024 * 1024);
        let shared_ram_mb = (desc.SharedSystemMemory as u64) / (1024 * 1024);

        // GPU dedicada: VRAM dedicada >= 512MB e não é adaptador Microsoft
        let is_discrete = vram_mb >= 512 && vendor != "Microsoft";

        let recommendation = if is_discrete && vram_mb >= 2048 {
            "GPU Dedicada de Alta Performance — Ideal para transcrição local ultrarrápida.".to_string()
        } else if is_discrete {
            "GPU Dedicada — Recomendada para modelos leves (Tiny / Base).".to_string()
        } else {
            "Gráficos Integrados — O processador (CPU) pode oferecer desempenho mais estável.".to_string()
        };

        result.push(GpuInfo {
            name,
            vendor,
            vram_mb,
            shared_ram_mb,
            is_discrete,
            recommendation,
        });
    }

    result
}

#[cfg(not(target_os = "windows"))]
fn detect_gpus_dxgi() -> Vec<GpuInfo> {
    Vec::new()
}
