//! Leitura das saídas de ferramentas do sistema (vulkaninfo, rocminfo,
//! nvidia-smi) e do próprio llama-server (`--list-devices`).
//! Funções puras: testadas com saídas salvas em `tests/fixtures`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct VkDevice {
    pub nome: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub tipo: String,
    pub driver_nome: Option<String>,
    pub driver_info: Option<String>,
    pub driver_versao: Option<String>,
}

impl VkDevice {
    pub fn is_cpu(&self) -> bool {
        self.tipo.contains("CPU") || self.driver_nome.as_deref() == Some("llvmpipe")
    }
    pub fn is_integrated(&self) -> bool {
        self.tipo.contains("INTEGRATED")
    }
}

fn parse_hex_u32(s: &str) -> Option<u32> {
    let s = s.trim();
    let s = s.split_whitespace().next().unwrap_or(s);
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

/// `vulkaninfo --summary`
pub fn parse_vulkaninfo_summary(text: &str) -> Vec<VkDevice> {
    let mut out = Vec::new();
    let mut cur: Option<VkDevice> = None;
    let mut in_devices = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("Devices:") {
            in_devices = true;
            continue;
        }
        if !in_devices {
            continue;
        }
        if t.starts_with("GPU") && t.ends_with(':') && t[3..t.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            if let Some(d) = cur.take() {
                out.push(d);
            }
            cur = Some(VkDevice::default());
            continue;
        }
        let Some(d) = cur.as_mut() else { continue };
        let Some((k, v)) = t.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim().to_string());
        match k {
            "deviceName" => d.nome = v,
            "vendorID" => d.vendor_id = parse_hex_u32(&v).unwrap_or(0),
            "deviceID" => d.device_id = parse_hex_u32(&v).unwrap_or(0),
            "deviceType" => d.tipo = v,
            "driverName" => d.driver_nome = Some(v),
            "driverInfo" => d.driver_info = Some(v),
            "driverVersion" => d.driver_versao = Some(v),
            _ => {}
        }
    }
    if let Some(d) = cur.take() {
        out.push(d);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RocmAgent {
    pub gfx: String,
    pub nome: String,
    pub chip_id: Option<u32>,
    pub vram_mb: Option<u64>,
}

/// `rocminfo` — devolve só os agentes do tipo GPU.
pub fn parse_rocminfo(text: &str) -> Vec<RocmAgent> {
    let mut out = Vec::new();
    // Divide em blocos "Agent N"
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("Agent ") && t[6..].trim().chars().all(|c| c.is_ascii_digit()) && !t[6..].trim().is_empty() {
            blocks.push(Vec::new());
            continue;
        }
        if let Some(b) = blocks.last_mut() {
            b.push(line);
        }
    }
    for b in blocks {
        let mut a = RocmAgent::default();
        let mut is_gpu = false;
        let mut in_global_pool = false;
        for line in b {
            let t = line.trim();
            let Some((k, v)) = t.split_once(':') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "Name" if a.gfx.is_empty() && v.starts_with("gfx") => a.gfx = v.to_string(),
                "Marketing Name" if a.nome.is_empty() => a.nome = v.to_string(),
                "Device Type" => is_gpu = v == "GPU",
                "Chip ID" => {
                    // "29695(0x73ff)"
                    a.chip_id = v.split('(').next().and_then(|n| n.trim().parse().ok());
                }
                "Segment" => in_global_pool = v.starts_with("GLOBAL") && v.contains("COARSE"),
                "Size" if in_global_pool && a.vram_mb.is_none() => {
                    // "8372224(0x7fc000) KB"
                    let n: Option<u64> = v.split('(').next().and_then(|n| n.trim().parse().ok());
                    let kb = v.trim_end().ends_with("KB");
                    if let Some(n) = n {
                        a.vram_mb = Some(if kb { n / 1024 } else { n / (1024 * 1024) });
                    }
                    in_global_pool = false;
                }
                _ => {}
            }
        }
        if is_gpu && !a.gfx.is_empty() {
            out.push(a);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct NvidiaGpu {
    pub nome: String,
    pub vram_total_mb: u64,
    pub vram_livre_mb: u64,
    pub driver: String,
    pub device_id: Option<u16>,
}

/// Comando usado para a consulta (mantido junto do parser para não desencontrar).
pub const NVIDIA_SMI_ARGS: &[&str] = &[
    "--query-gpu=name,memory.total,memory.free,driver_version,pci.device_id",
    "--format=csv,noheader,nounits",
];

/// `nvidia-smi --query-gpu=name,memory.total,memory.free,driver_version,pci.device_id --format=csv,noheader,nounits`
pub fn parse_nvidia_smi(text: &str) -> Vec<NvidiaGpu> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let parts: Vec<&str> = t.rsplitn(5, ',').map(|s| s.trim()).collect();
        if parts.len() != 5 {
            continue;
        }
        // rsplitn devolve ao contrário
        let (pci, driver, free, total, name) = (parts[0], parts[1], parts[2], parts[3], parts[4]);
        let Ok(total) = total.parse::<u64>() else { continue };
        // pci.device_id = 0xDDDDVVVV (dispositivo nos 16 bits altos)
        let device_id = parse_hex_u32(pci).map(|v| (v >> 16) as u16);
        out.push(NvidiaGpu {
            nome: name.to_string(),
            vram_total_mb: total,
            vram_livre_mb: free.parse().unwrap_or(0),
            driver: driver.to_string(),
            device_id,
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LlamaDevice {
    pub id: String,
    pub nome: String,
    pub total_mb: Option<u64>,
    pub livre_mb: Option<u64>,
}

/// `llama-server --list-devices`
/// Linha típica: `  Vulkan0: AMD Radeon RX 580 Series (RADV POLARIS10) (8192 MiB, 7934 MiB free)`
pub fn parse_list_devices(text: &str) -> Vec<LlamaDevice> {
    let mut out = Vec::new();
    let mut started = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("Available devices") {
            started = true;
            continue;
        }
        if !started || t.is_empty() || t == "(none)" {
            continue;
        }
        let Some((id, rest)) = t.split_once(": ") else { continue };
        if id.contains(' ') {
            continue;
        }
        let mut dev = LlamaDevice { id: id.to_string(), nome: rest.trim().to_string(), ..Default::default() };
        if let Some(idx) = rest.rfind(" (") {
            let inside = rest[idx + 2..].trim_end_matches(')');
            if inside.contains("MiB") {
                dev.nome = rest[..idx].trim().to_string();
                for part in inside.split(',') {
                    let p = part.trim();
                    let n: Option<u64> = p.split_whitespace().next().and_then(|n| n.parse().ok());
                    if p.ends_with("free") {
                        dev.livre_mb = n;
                    } else {
                        dev.total_mb = n;
                    }
                }
            }
        }
        out.push(dev);
    }
    out
}

/// Converte `gfx_target_version` do KFD (ex.: 100302) em "gfx1032".
pub fn gfx_from_kfd_version(v: u32) -> Option<String> {
    if v == 0 {
        return None;
    }
    let major = v / 10000;
    let minor = (v / 100) % 100;
    let step = v % 100;
    Some(format!("gfx{major}{minor:x}{step:x}"))
}

/// Remove sufixos de driver do nome ("AMD Radeon RX 580 Series (RADV POLARIS10)" → "AMD Radeon RX 580 Series").
pub fn clean_name(name: &str) -> String {
    let mut n = name.trim().to_string();
    for marker in [" (RADV", " (AMDVLK", " (NVIDIA", " (Intel", " (LLVM"] {
        if let Some(i) = n.find(marker) {
            n.truncate(i);
        }
    }
    n.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gfx_do_kfd() {
        assert_eq!(gfx_from_kfd_version(80003).as_deref(), Some("gfx803"));
        assert_eq!(gfx_from_kfd_version(100302).as_deref(), Some("gfx1032"));
        assert_eq!(gfx_from_kfd_version(110002).as_deref(), Some("gfx1102"));
        assert_eq!(gfx_from_kfd_version(120000).as_deref(), Some("gfx1200"));
        assert_eq!(gfx_from_kfd_version(90010).as_deref(), Some("gfx90a"));
        assert_eq!(gfx_from_kfd_version(0), None);
    }

    #[test]
    fn limpa_nome() {
        assert_eq!(clean_name("AMD Radeon RX 580 Series (RADV POLARIS10)"), "AMD Radeon RX 580 Series");
        assert_eq!(clean_name("NVIDIA GeForce RTX 3060"), "NVIDIA GeForce RTX 3060");
    }

    #[test]
    fn list_devices_sem_placa() {
        assert!(parse_list_devices("Available devices:\n  (none)\n").is_empty());
    }

    #[test]
    fn list_devices_com_placa() {
        let d = parse_list_devices(
            "ggml_vulkan: Found 1 Vulkan devices:\nAvailable devices:\n  Vulkan0: AMD Radeon RX 580 Series (RADV POLARIS10) (8192 MiB, 7934 MiB free)\n",
        );
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].id, "Vulkan0");
        assert_eq!(d[0].nome, "AMD Radeon RX 580 Series (RADV POLARIS10)");
        assert_eq!(d[0].total_mb, Some(8192));
        assert_eq!(d[0].livre_mb, Some(7934));
    }
}
