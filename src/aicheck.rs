//! KI-Check: schätzt ohne Download ab, was dieser Rechner lokal schafft.

use windows::core::w;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::i18n::t;

pub struct Hardware {
    pub cpu: String,
    pub threads: usize,
    pub avx2: bool,
    /// Arbeitsspeicher in GB.
    pub ram_gb: f64,
    /// Grafikkarte mit dem meisten eigenen Speicher: Name und Speicher in GB.
    pub gpu: Option<(String, f64)>,
}

/// Wie gut etwas lokal läuft.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Rating {
    Good,
    Slow,
    NotRecommended,
}

pub fn detect() -> Hardware {
    let mut name = [0u16; 128];
    let mut size = (name.len() * 2) as u32;
    let cpu = unsafe {
        let status = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"),
            w!("ProcessorNameString"),
            RRF_RT_REG_SZ,
            None,
            Some(name.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        if status.is_ok() {
            let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            String::from_utf16_lossy(&name[..len]).trim().to_string()
        } else {
            String::new()
        }
    };
    let mut memory = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    let ram_gb = unsafe { GlobalMemoryStatusEx(&mut memory) }.map_or(0.0, |()| memory.ullTotalPhys as f64 / 1_073_741_824.0);
    Hardware {
        cpu,
        threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
        avx2: is_x86_feature_detected!("avx2"),
        ram_gb,
        gpu: gpu(),
    }
}

fn gpu() -> Option<(String, f64)> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut best: Option<(String, f64)> = None;
        for index in 0.. {
            let Ok(adapter) = factory.EnumAdapters1(index) else { break };
            let Ok(desc) = adapter.GetDesc1() else { continue };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let vram = desc.DedicatedVideoMemory as f64 / 1_073_741_824.0;
            let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..len]);
            if best.as_ref().is_none_or(|(_, v)| vram > *v) {
                best = Some((name, vram));
            }
        }
        best
    }
}

impl Hardware {
    /// Ein lokales Sprachmodell (Zusammenfassen, Umformulieren).
    pub fn local_llm(&self) -> Rating {
        if self.gpu.as_ref().is_some_and(|(_, vram)| *vram >= 6.0) && self.ram_gb >= 15.0 {
            Rating::Good
        } else if self.ram_gb >= 15.0 && self.avx2 && self.threads >= 8 {
            Rating::Slow
        } else {
            Rating::NotRecommended
        }
    }

    /// Lokale Spracherkennung (Whisper) fürs Diktieren.
    pub fn dictation(&self) -> Rating {
        if self.avx2 && self.ram_gb >= 7.5 && self.threads >= 4 {
            Rating::Good
        } else if self.ram_gb >= 3.5 {
            Rating::Slow
        } else {
            Rating::NotRecommended
        }
    }

    /// Bericht in der Sprache der Oberfläche, für den KI-Check.
    pub fn report(&self) -> String {
        let rating = |r: Rating| match r {
            Rating::Good => t("check.good"),
            Rating::Slow => t("check.slow"),
            Rating::NotRecommended => t("check.not_recommended"),
        };
        let gpu = match &self.gpu {
            Some((name, vram)) if *vram >= 1.0 => format!("{name} ({vram:.0} GB)"),
            Some((name, _)) => format!("{name} ({})", t("check.integrated")),
            None => t("check.none").to_string(),
        };
        let advice = match self.local_llm() {
            Rating::Good => t("check.advice_local"),
            Rating::Slow => t("check.advice_slow"),
            Rating::NotRecommended => t("check.advice_server"),
        };
        format!(
            "{}: {} ({} {})\n{}: {:.0} GB   ·   {}: {}\n{}: {}   ·   {}: {}\n{}",
            t("check.cpu"),
            if self.cpu.is_empty() { "?" } else { &self.cpu },
            self.threads,
            t("check.threads"),
            t("check.ram"),
            self.ram_gb,
            t("check.gpu"),
            gpu,
            t("check.local_ai"),
            rating(self.local_llm()),
            t("check.dictation"),
            rating(self.dictation()),
            advice
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_this_computer() {
        let hardware = detect();
        println!("{} | {} Threads | AVX2 {} | {:.1} GB | {:?}", hardware.cpu, hardware.threads, hardware.avx2, hardware.ram_gb, hardware.gpu);
        assert!(hardware.ram_gb > 1.0 && hardware.threads >= 1);
    }

    #[test]
    fn rates_typical_office_pc() {
        let office = Hardware { cpu: "i7".into(), threads: 8, avx2: true, ram_gb: 15.8, gpu: Some(("Intel UHD".into(), 0.1)) };
        assert_eq!(office.local_llm(), Rating::Slow);
        assert_eq!(office.dictation(), Rating::Good);
        let gaming = Hardware { gpu: Some(("RTX".into(), 8.0)), ram_gb: 32.0, ..office };
        assert_eq!(gaming.local_llm(), Rating::Good);
    }
}
