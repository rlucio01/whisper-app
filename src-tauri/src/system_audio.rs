//! Controle do volume/mute do áudio do sistema (Windows).

#[cfg(target_os = "windows")]
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
#[cfg(target_os = "windows")]
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};

#[cfg(target_os = "windows")]
pub struct WindowsAudioMuter {
    endpoint_volume: IAudioEndpointVolume,
    original_mute: bool,
    active: bool,
}

#[cfg(target_os = "windows")]
impl WindowsAudioMuter {
    /// Obtém a interface de volume do endpoint padrão de reprodução e muta o áudio,
    /// lembrando o estado anterior de mute para restaurar no drop ou método restore().
    pub fn mute() -> Option<Self> {
        unsafe {
            // Inicializa COM para a thread atual caso ainda não esteja inicializado.
            // RPC_E_CHANGED_MODE é aceitável caso a thread já tenha inicializado como STA.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;

            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;

            let endpoint_volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).ok()?;

            let original_mute = endpoint_volume.GetMute().ok()?.as_bool();

            // Se já não estiver mudo, muta agora
            if !original_mute {
                let _ = endpoint_volume.SetMute(true, std::ptr::null());
            }

            Some(Self {
                endpoint_volume,
                original_mute,
                active: true,
            })
        }
    }

    /// Restaura o estado anterior de mute.
    pub fn restore(&mut self) {
        if self.active {
            unsafe {
                // Restaura o estado original: se o usuário já estava mudo antes, continua mudo.
                // Se não estava mudo, desmuta.
                let _ = self.endpoint_volume.SetMute(self.original_mute, std::ptr::null());
            }
            self.active = false;
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsAudioMuter {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(not(target_os = "windows"))]
pub struct WindowsAudioMuter;

#[cfg(not(target_os = "windows"))]
impl WindowsAudioMuter {
    pub fn mute() -> Option<Self> {
        None
    }
    pub fn restore(&mut self) {}
}
