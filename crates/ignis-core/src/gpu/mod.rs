//! Detecção da placa de vídeo: junta várias fontes e devolve uma lista limpa.
//!
//! Fontes (cada uma é opcional):
//! - Linux: /sys (IDs, VRAM e driver na AMD), KFD (alvo gfx), vulkaninfo, rocminfo, nvidia-smi
//! - Windows: DXGI (IDs, nome, VRAM total e livre, versão do driver), nvidia-smi
//! - Depois que um motor é instalado: `llama-server --list-devices` (nome e VRAM livre vistos pelo motor)

pub mod parse;
pub mod pci;
pub mod sysfs;
pub mod windows;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use parse::{LlamaDevice, NvidiaGpu, RocmAgent, VkDevice};
use pci::{VENDOR_AMD, VENDOR_INTEL, VENDOR_NVIDIA};
use sysfs::{KfdNode, SysfsCard};
use windows::DxgiAdapter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Amd,
    Nvidia,
    Intel,
    Outro,
}

impl Vendor {
    pub fn from_pci(id: u32) -> Self {
        match id as u16 {
            _ if id > 0xffff => Vendor::Outro,
            VENDOR_AMD | 0x1022 => Vendor::Amd,
            VENDOR_NVIDIA => Vendor::Nvidia,
            VENDOR_INTEL => Vendor::Intel,
            _ => Vendor::Outro,
        }
    }
    pub fn nome(&self) -> &'static str {
        match self {
            Vendor::Amd => "AMD",
            Vendor::Nvidia => "NVIDIA",
            Vendor::Intel => "Intel",
            Vendor::Outro => "Outro",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gpu {
    pub fabricante: Vendor,
    pub nome: String,
    pub pci_dispositivo: Option<u16>,
    pub vram_total_mb: Option<u64>,
    pub vram_livre_mb: Option<u64>,
    /// Driver em uso (ex.: "amdgpu", "radv", "nvidia").
    pub driver: Option<String>,
    /// Versão do driver (Mesa, NVIDIA, Adrenalin...).
    pub versao_driver: Option<String>,
    /// Alvo gfx da AMD (ex.: gfx1032).
    pub alvo_gfx: Option<String>,
    pub integrada: bool,
    /// De onde veio cada informação (para o relatório).
    pub fontes: Vec<String>,
}

impl Gpu {
    fn new(fabricante: Vendor, nome: String, fonte: &str) -> Self {
        Self {
            fabricante,
            nome,
            pci_dispositivo: None,
            vram_total_mb: None,
            vram_livre_mb: None,
            driver: None,
            versao_driver: None,
            alvo_gfx: None,
            integrada: false,
            fontes: vec![fonte.to_string()],
        }
    }
    fn add_source(&mut self, s: &str) {
        if !self.fontes.iter().any(|f| f == s) {
            self.fontes.push(s.to_string());
        }
    }
    /// Texto curto para o relatório e para a impressão digital.
    pub fn resumo(&self) -> String {
        format!(
            "{} | {} | {} MB | gfx={} | driver={}",
            self.fabricante.nome(),
            self.nome,
            self.vram_total_mb.unwrap_or(0),
            self.alvo_gfx.as_deref().unwrap_or("-"),
            self.versao_driver.as_deref().unwrap_or("-")
        )
    }
}

/// Saídas brutas coletadas do sistema. Separado da coleta para poder testar
/// a junção com saídas salvas (fixtures).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawSources {
    pub sysfs: Vec<SysfsCard>,
    pub kfd: Vec<KfdNode>,
    pub dxgi: Vec<DxgiAdapter>,
    pub vulkaninfo: Option<String>,
    pub rocminfo: Option<String>,
    pub nvidia_smi: Option<String>,
    /// Versão do kernel (Linux), usada como "versão do driver" do amdgpu.
    pub kernel: Option<String>,
    /// /sys/module/nvidia/version (driver NVIDIA sem nvidia-smi, ex.: dentro do Flatpak).
    #[serde(default)]
    pub nvidia_modulo: Option<String>,
}

/// Junta as fontes numa lista de placas.
pub fn merge(raw: &RawSources) -> Vec<Gpu> {
    let mut gpus: Vec<Gpu> = Vec::new();

    // 1) Base: sysfs (Linux) ou DXGI (Windows) — trazem IDs PCI.
    for c in &raw.sysfs {
        let v = Vendor::from_pci(c.vendor as u32);
        let mut g = Gpu::new(v, String::new(), "sysfs");
        g.pci_dispositivo = Some(c.device);
        g.driver = c.driver.clone();
        g.vram_total_mb = c.vram_total_mb;
        if let (Some(t), Some(u)) = (c.vram_total_mb, c.vram_usada_mb) {
            g.vram_livre_mb = Some(t.saturating_sub(u));
        }
        if v == Vendor::Amd && c.driver.as_deref() == Some("amdgpu") {
            g.versao_driver = raw.kernel.as_ref().map(|k| format!("amdgpu (kernel {k})"));
        }
        gpus.push(g);
    }
    for a in raw.dxgi.iter().filter(|a| a.vendor != 0x1414) {
        // 0x1414 = "Microsoft Basic Render Driver" (placa de vídeo por software)
        let v = Vendor::from_pci(a.vendor as u32);
        let mut g = Gpu::new(v, a.nome.trim().to_string(), "dxgi");
        g.pci_dispositivo = Some(a.device);
        g.vram_total_mb = Some(a.vram_total_mb);
        g.vram_livre_mb = a.vram_livre_mb;
        g.versao_driver = a.driver.clone();
        gpus.push(g);
    }

    // 2) KFD: alvo gfx por ID de dispositivo.
    for n in &raw.kfd {
        if let Some(g) = gpus.iter_mut().find(|g| g.fabricante == Vendor::Amd && g.pci_dispositivo == Some(n.device)) {
            g.alvo_gfx = n.gfx.clone();
            g.add_source("kfd");
        }
    }

    // 3) vulkaninfo: nome bonito e driver (Mesa/RADV etc.).
    if let Some(text) = &raw.vulkaninfo {
        for d in parse::parse_vulkaninfo_summary(text).into_iter().filter(|d| !d.is_cpu()) {
            merge_vulkan(&mut gpus, &d);
        }
    }

    // 4) nvidia-smi: VRAM total/livre e versão do driver.
    if let Some(text) = &raw.nvidia_smi {
        for (i, n) in parse::parse_nvidia_smi(text).into_iter().enumerate() {
            merge_nvidia(&mut gpus, &n, i);
        }
    }

    // 4b) versão do driver NVIDIA pelo módulo do kernel, se o nvidia-smi não existir
    if let Some(v) = raw.nvidia_modulo.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        for g in gpus.iter_mut().filter(|g| g.fabricante == Vendor::Nvidia && g.versao_driver.is_none()) {
            g.versao_driver = Some(v.to_string());
            g.add_source("modulo-nvidia");
        }
    }

    // 5) rocminfo: alvo gfx e nome comercial.
    if let Some(text) = &raw.rocminfo {
        for a in parse::parse_rocminfo(text) {
            merge_rocm(&mut gpus, &a);
        }
    }

    // 6) Tabela PCI: preenche o que faltar.
    for g in gpus.iter_mut() {
        let Some(dev) = g.pci_dispositivo else { continue };
        match g.fabricante {
            Vendor::Amd => {
                if let Some(k) = pci::lookup_amd(dev) {
                    if g.nome.is_empty() {
                        g.nome = k.nome.to_string();
                    }
                    if g.alvo_gfx.is_none() {
                        g.alvo_gfx = Some(k.gfx.to_string());
                        g.add_source("tabela-pci");
                    }
                    g.integrada |= k.integrada;
                }
            }
            Vendor::Intel => g.integrada |= pci::intel_is_integrated(dev),
            _ => {}
        }
        if g.nome.is_empty() {
            g.nome = format!("{} (dispositivo 0x{dev:04x})", g.fabricante.nome());
        }
    }
    // Integrada AMD com pouca "VRAM" (memória reservada) também conta.
    for g in gpus.iter_mut() {
        if g.fabricante == Vendor::Amd
            && g.vram_total_mb.map(|v| v <= 2048).unwrap_or(false)
            && g.alvo_gfx.as_deref().map(is_apu_gfx).unwrap_or(false)
        {
            g.integrada = true;
        }
        g.nome = parse::clean_name(&g.nome);
    }
    gpus
}

fn is_apu_gfx(g: &str) -> bool {
    matches!(g, "gfx1035" | "gfx1036" | "gfx1103" | "gfx1150" | "gfx1151" | "gfx1152" | "gfx1153" | "gfx90c" | "gfx902")
}

fn merge_vulkan(gpus: &mut Vec<Gpu>, d: &VkDevice) {
    let v = Vendor::from_pci(d.vendor_id);
    let pos = gpus.iter().position(|g| g.fabricante == v && g.pci_dispositivo == Some(d.device_id as u16)).or_else(|| {
        // sem ID PCI (ex.: só vulkaninfo): casa por fabricante se houver só uma
        let same: Vec<usize> =
            gpus.iter().enumerate().filter(|(_, g)| g.fabricante == v && g.pci_dispositivo.is_none()).map(|(i, _)| i).collect();
        (same.len() == 1).then(|| same[0])
    });
    let g = match pos {
        Some(i) => &mut gpus[i],
        None => {
            let mut g = Gpu::new(v, String::new(), "vulkaninfo");
            g.pci_dispositivo = Some(d.device_id as u16);
            gpus.push(g);
            gpus.last_mut().unwrap()
        }
    };
    g.add_source("vulkaninfo");
    if !d.nome.is_empty() {
        g.nome = d.nome.clone();
    }
    g.integrada |= d.is_integrated();
    if let Some(info) = &d.driver_info {
        // RADV/Mesa: "Mesa 25.0.4" é a versão que importa; NVIDIA: driverInfo = "580.95.05"
        if g.fabricante != Vendor::Nvidia || g.versao_driver.is_none() {
            let nome = d.driver_nome.clone().unwrap_or_default();
            g.versao_driver = Some(if nome.is_empty() || info.contains(&nome) { info.clone() } else { format!("{info} ({nome})") });
        }
    }
    if g.driver.is_none() {
        g.driver = d.driver_nome.clone();
    }
}

fn merge_nvidia(gpus: &mut Vec<Gpu>, n: &NvidiaGpu, index: usize) {
    let pos = gpus
        .iter()
        .position(|g| g.fabricante == Vendor::Nvidia && n.device_id.is_some() && g.pci_dispositivo == n.device_id)
        .or_else(|| gpus.iter().enumerate().filter(|(_, g)| g.fabricante == Vendor::Nvidia).nth(index).map(|(i, _)| i));
    let g = match pos {
        Some(i) => &mut gpus[i],
        None => {
            gpus.push(Gpu::new(Vendor::Nvidia, String::new(), "nvidia-smi"));
            gpus.last_mut().unwrap()
        }
    };
    g.add_source("nvidia-smi");
    g.nome = n.nome.clone();
    g.vram_total_mb = Some(n.vram_total_mb);
    g.vram_livre_mb = Some(n.vram_livre_mb);
    g.versao_driver = Some(n.driver.clone());
    if g.pci_dispositivo.is_none() {
        g.pci_dispositivo = n.device_id;
    }
    g.driver.get_or_insert_with(|| "nvidia".into());
}

fn merge_rocm(gpus: &mut Vec<Gpu>, a: &RocmAgent) {
    let pos = gpus
        .iter()
        .position(|g| g.fabricante == Vendor::Amd && a.chip_id.is_some() && g.pci_dispositivo.map(|d| d as u32) == a.chip_id)
        .or_else(|| gpus.iter().position(|g| g.fabricante == Vendor::Amd && g.alvo_gfx.as_deref() == Some(a.gfx.as_str())))
        .or_else(|| {
            let amd: Vec<usize> = gpus.iter().enumerate().filter(|(_, g)| g.fabricante == Vendor::Amd).map(|(i, _)| i).collect();
            (amd.len() == 1).then(|| amd[0])
        });
    let g = match pos {
        Some(i) => &mut gpus[i],
        None => {
            let mut g = Gpu::new(Vendor::Amd, String::new(), "rocminfo");
            g.pci_dispositivo = a.chip_id.map(|c| c as u16);
            gpus.push(g);
            gpus.last_mut().unwrap()
        }
    };
    g.add_source("rocminfo");
    g.alvo_gfx = Some(a.gfx.clone());
    if g.nome.is_empty() && !a.nome.is_empty() {
        g.nome = a.nome.clone();
    }
    if g.vram_total_mb.is_none() {
        g.vram_total_mb = a.vram_mb;
    }
}

/// Atualiza nome e VRAM livre com o que o motor (`--list-devices`) enxerga.
pub fn apply_llama_devices(gpus: &mut [Gpu], devices: &[LlamaDevice]) {
    for d in devices {
        let lower = d.nome.to_lowercase();
        let target = gpus.iter_mut().find(|g| match g.fabricante {
            Vendor::Amd => lower.contains("amd") || lower.contains("radeon"),
            Vendor::Nvidia => lower.contains("nvidia") || lower.contains("geforce") || lower.contains("rtx"),
            Vendor::Intel => lower.contains("intel"),
            Vendor::Outro => false,
        });
        if let Some(g) = target {
            g.add_source("llama-server");
            if let Some(l) = d.livre_mb {
                g.vram_livre_mb = Some(l);
            }
            if g.vram_total_mb.is_none() {
                g.vram_total_mb = d.total_mb;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub sistema: crate::distro::Sistema,
    pub arq: String,
    pub placas: Vec<Gpu>,
    pub ram_total_mb: u64,
    pub ram_livre_mb: u64,
    /// Bibliotecas do sistema procuradas → encontradas?
    pub libs: BTreeMap<String, bool>,
    /// /dev/kfd acessível (ROCm no Linux).
    pub kfd_acessivel: bool,
    pub ferramentas: Vec<String>,
    pub fontes_brutas: RawSources,
}

impl Detection {
    /// Placa principal: dedicada com mais VRAM; senão a integrada com mais VRAM.
    pub fn principal(&self) -> Option<&Gpu> {
        self.placas
            .iter()
            .filter(|g| g.fabricante != Vendor::Outro || g.vram_total_mb.is_some())
            .max_by_key(|g| (!g.integrada, g.vram_total_mb.unwrap_or(0)))
    }

    pub fn lib(&self, name: &str) -> bool {
        self.libs.get(name).copied().unwrap_or(false)
    }

    /// Impressão digital: muda quando a placa, o driver ou o sistema mudam.
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(self.arq.as_bytes());
        h.update(format!("{:?}", self.sistema.familia).as_bytes());
        h.update(self.sistema.flatpak.to_string().as_bytes());
        let mut placas: Vec<String> = self
            .placas
            .iter()
            .map(|g| {
                format!("{}|{:?}|{}|{}", g.nome, g.pci_dispositivo, g.versao_driver.as_deref().unwrap_or(""), g.vram_total_mb.unwrap_or(0))
            })
            .collect();
        placas.sort();
        for p in placas {
            h.update(p.as_bytes());
        }
        for (k, v) in &self.libs {
            h.update(format!("{k}={v}").as_bytes());
        }
        hex::encode(&h.finalize()[..12])
    }
}

/// Roda um programa com tempo-limite e devolve a saída padrão, se deu certo.
pub async fn run_tool(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    crate::util::hide_console(&mut cmd);
    let child = cmd.spawn().ok()?;
    let out = tokio::time::timeout(timeout, child.wait_with_output()).await.ok()?.ok()?;
    if !out.status.success() && out.stdout.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Como `run_tool`, mas junta a saída padrão e a de erros.
pub async fn run_tool_all(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = Path::new(program).parent() {
        cmd.current_dir(dir);
        if cfg!(target_os = "linux") {
            cmd.env("LD_LIBRARY_PATH", dir);
        }
    }
    crate::util::hide_console(&mut cmd);
    let child = cmd.spawn().ok()?;
    let out = tokio::time::timeout(timeout, child.wait_with_output()).await.ok()?.ok()?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push('\n');
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(s)
}

/// Procura uma biblioteca do sistema.
pub fn find_system_lib(name: &str, ldconfig_cache: Option<&str>) -> bool {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        return Path::new(&root).join("System32").join(name).exists();
    }
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(ld) = std::env::var_os("LD_LIBRARY_PATH") {
        dirs.extend(std::env::split_paths(&ld));
    }
    for d in [
        "/usr/lib64",
        "/usr/lib",
        "/usr/lib/x86_64-linux-gnu",
        "/lib64",
        "/lib/x86_64-linux-gnu",
        "/usr/local/lib",
        "/opt/rocm/lib",
        "/opt/rocm/lib64",
        // Flatpak: extensões GL do runtime
        "/usr/lib/x86_64-linux-gnu/GL/default/lib",
    ] {
        dirs.push(d.into());
    }
    if dirs.iter().any(|d| d.join(name).exists()) {
        return true;
    }
    ldconfig_cache.map(|c| c.lines().any(|l| l.trim_start().starts_with(name))).unwrap_or(false)
}

/// Pastas extras de bibliotecas que o motor precisa enxergar (ROCm em /opt).
pub fn extra_lib_dirs() -> Vec<std::path::PathBuf> {
    ["/opt/rocm/lib", "/opt/rocm/lib64"].iter().map(std::path::PathBuf::from).filter(|p| p.exists()).collect()
}

/// Coleta tudo do sistema real.
pub async fn detect(libs_to_probe: &[String]) -> Detection {
    let t = Duration::from_secs(6);
    let mut raw = RawSources::default();
    let mut ferramentas = Vec::new();
    if cfg!(target_os = "linux") {
        let sys = Path::new("/sys");
        raw.sysfs = sysfs::read_drm_cards(sys);
        raw.kfd = sysfs::read_kfd_nodes(sys);
        raw.kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok().map(|s| s.trim().to_string());
        raw.nvidia_modulo = std::fs::read_to_string("/sys/module/nvidia/version").ok().map(|s| s.trim().to_string());
    }
    if cfg!(windows) {
        raw.dxgi = tokio::task::spawn_blocking(windows::enumerate).await.unwrap_or_default();
    }
    let (vk, rocm, nv) = tokio::join!(
        run_tool("vulkaninfo", &["--summary"], t),
        async {
            if cfg!(target_os = "linux") {
                run_tool("rocminfo", &[], t).await
            } else {
                None
            }
        },
        run_tool("nvidia-smi", parse::NVIDIA_SMI_ARGS, t),
    );
    if vk.is_some() {
        ferramentas.push("vulkaninfo".to_string());
    }
    if rocm.is_some() {
        ferramentas.push("rocminfo".to_string());
    }
    if nv.is_some() {
        ferramentas.push("nvidia-smi".to_string());
    }
    raw.vulkaninfo = vk;
    raw.rocminfo = rocm;
    raw.nvidia_smi = nv;

    let ldcache = if cfg!(target_os = "linux") {
        run_tool("ldconfig", &["-p"], t).await.or(run_tool("/sbin/ldconfig", &["-p"], t).await)
    } else {
        None
    };
    let libs = libs_to_probe.iter().map(|l| (l.clone(), find_system_lib(l, ldcache.as_deref()))).collect();
    let kfd_acessivel = cfg!(target_os = "linux") && std::fs::OpenOptions::new().read(true).write(true).open("/dev/kfd").is_ok();
    let (ram_total_mb, ram_livre_mb) = crate::util::system_memory_mb();
    Detection {
        sistema: crate::distro::Sistema::detect(),
        arq: std::env::consts::ARCH.to_string(),
        placas: merge(&raw),
        ram_total_mb,
        ram_livre_mb,
        libs,
        kfd_acessivel,
        ferramentas,
        fontes_brutas: raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_sem_nvidia_smi_usa_modulo_do_kernel() {
        let raw = RawSources {
            sysfs: vec![SysfsCard { vendor: 0x10de, device: 0x2504, driver: Some("nvidia".into()), ..Default::default() }],
            nvidia_modulo: Some("580.95.05\n".into()),
            ..Default::default()
        };
        let g = merge(&raw);
        assert_eq!(g[0].versao_driver.as_deref(), Some("580.95.05"));
        assert_eq!(g[0].nome, "NVIDIA (dispositivo 0x2504)");
    }

    #[test]
    fn list_devices_completa_vram() {
        let raw = RawSources {
            sysfs: vec![SysfsCard { vendor: 0x10de, device: 0x2504, driver: Some("nvidia".into()), ..Default::default() }],
            ..Default::default()
        };
        let mut g = merge(&raw);
        apply_llama_devices(
            &mut g,
            &parse::parse_list_devices("Available devices:\n  Vulkan0: NVIDIA GeForce RTX 3060 (12288 MiB, 11500 MiB free)\n"),
        );
        assert_eq!(g[0].vram_total_mb, Some(12288));
        assert_eq!(g[0].vram_livre_mb, Some(11500));
    }

    #[test]
    fn vendor_from_pci() {
        assert_eq!(Vendor::from_pci(0x1002), Vendor::Amd);
        assert_eq!(Vendor::from_pci(0x10de), Vendor::Nvidia);
        assert_eq!(Vendor::from_pci(0x8086), Vendor::Intel);
        assert_eq!(Vendor::from_pci(0x10005), Vendor::Outro, "Mesa llvmpipe");
    }
}
