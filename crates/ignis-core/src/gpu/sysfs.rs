//! Leitura do /sys no Linux (funciona também dentro do Flatpak).
//! As funções recebem a raiz (normalmente "/sys") para poderem ser testadas
//! com uma árvore falsa.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SysfsCard {
    pub vendor: u16,
    pub device: u16,
    pub driver: Option<String>,
    pub vram_total_mb: Option<u64>,
    pub vram_usada_mb: Option<u64>,
    /// Endereço PCI (ex.: 0000:03:00.0), para não contar a mesma placa duas vezes.
    pub pci_slot: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct KfdNode {
    pub vendor: u16,
    pub device: u16,
    pub gfx: Option<String>,
}

fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn read_hex_u16(p: &Path) -> Option<u16> {
    let s = read_trim(p)?;
    u16::from_str_radix(s.trim_start_matches("0x"), 16).ok()
}

/// Placas de vídeo em /sys/class/drm/cardN/device.
pub fn read_drm_cards(sys_root: &Path) -> Vec<SysfsCard> {
    let drm = sys_root.join("class/drm");
    let Ok(entries) = std::fs::read_dir(&drm) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("card") && n[4..].chars().all(|c| c.is_ascii_digit()) && n.len() > 4)
        .collect();
    names.sort();
    let mut out: Vec<SysfsCard> = Vec::new();
    for n in names {
        let dev = drm.join(&n).join("device");
        let Some(vendor) = read_hex_u16(&dev.join("vendor")) else { continue };
        let Some(device) = read_hex_u16(&dev.join("device")) else { continue };
        // Só controladoras de vídeo (classe 0x03xxxx)
        if let Some(class) = read_trim(&dev.join("class")) {
            if !class.trim_start_matches("0x").starts_with("03") {
                continue;
            }
        }
        let driver = std::fs::read_link(dev.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
            .or_else(|| read_trim(&dev.join("driver_name")));
        let mb = |f: &str| read_trim(&dev.join(f)).and_then(|s| s.parse::<u64>().ok()).map(|b| b / (1024 * 1024));
        let pci_slot = std::fs::canonicalize(&dev)
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
            .filter(|s| s.contains(':'))
            .or_else(|| {
                read_trim(&dev.join("uevent")).and_then(|u| u.lines().find_map(|l| l.strip_prefix("PCI_SLOT_NAME=").map(String::from)))
            });
        let card = SysfsCard {
            vendor,
            device,
            driver,
            vram_total_mb: mb("mem_info_vram_total"),
            vram_usada_mb: mb("mem_info_vram_used"),
            pci_slot,
        };
        if card.pci_slot.is_some() && out.iter().any(|c| c.pci_slot == card.pci_slot) {
            continue;
        }
        out.push(card);
    }
    out
}

/// Nós do KFD (driver de computação da AMD), que dizem o alvo gfx.
pub fn read_kfd_nodes(sys_root: &Path) -> Vec<KfdNode> {
    let nodes = sys_root.join("class/kfd/kfd/topology/nodes");
    let Ok(entries) = std::fs::read_dir(&nodes) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.filter_map(|e| e.ok()) {
        let Some(props) = read_trim(&e.path().join("properties")) else { continue };
        let mut node = KfdNode::default();
        let mut gfx_ver = 0u32;
        for line in props.lines() {
            let mut it = line.split_whitespace();
            let (Some(k), Some(v)) = (it.next(), it.next()) else { continue };
            match k {
                "gfx_target_version" => gfx_ver = v.parse().unwrap_or(0),
                "vendor_id" => node.vendor = v.parse().unwrap_or(0),
                "device_id" => node.device = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        node.gfx = super::parse::gfx_from_kfd_version(gfx_ver);
        if node.gfx.is_some() {
            out.push(node);
        }
    }
    out
}

/// Ferramenta de teste: cria uma árvore /sys falsa.
pub fn write_fake_sys(root: &Path, cards: &[SysfsCard], kfd: &[KfdNode]) -> std::io::Result<()> {
    for (i, c) in cards.iter().enumerate() {
        let dev = root.join(format!("class/drm/card{i}/device"));
        std::fs::create_dir_all(&dev)?;
        std::fs::write(dev.join("vendor"), format!("0x{:04x}\n", c.vendor))?;
        std::fs::write(dev.join("device"), format!("0x{:04x}\n", c.device))?;
        std::fs::write(dev.join("class"), "0x030000\n")?;
        if let Some(d) = &c.driver {
            std::fs::write(dev.join("driver_name"), d)?;
        }
        if let Some(s) = &c.pci_slot {
            std::fs::write(dev.join("uevent"), format!("DRIVER=x\nPCI_SLOT_NAME={s}\n"))?;
        }
        if let Some(t) = c.vram_total_mb {
            std::fs::write(dev.join("mem_info_vram_total"), format!("{}\n", t * 1024 * 1024))?;
        }
        if let Some(u) = c.vram_usada_mb {
            std::fs::write(dev.join("mem_info_vram_used"), format!("{}\n", u * 1024 * 1024))?;
        }
        // conector (deve ser ignorado)
        std::fs::create_dir_all(root.join(format!("class/drm/card{i}-DP-1")))?;
    }
    for (i, n) in kfd.iter().enumerate() {
        let d = root.join(format!("class/kfd/kfd/topology/nodes/{}", i + 1));
        std::fs::create_dir_all(&d)?;
        let ver = n
            .gfx
            .as_deref()
            .map(|g| {
                // inverso simples de gfx_from_kfd_version para os casos de teste
                let h = &g[3..];
                let (maj, rest) = h.split_at(h.len() - 2);
                let minor = u32::from_str_radix(&rest[..1], 16).unwrap();
                let step = u32::from_str_radix(&rest[1..], 16).unwrap();
                maj.parse::<u32>().unwrap() * 10000 + minor * 100 + step
            })
            .unwrap_or(0);
        std::fs::write(
            d.join("properties"),
            format!("cpu_cores_count 0\nsimd_count 128\ngfx_target_version {ver}\nvendor_id {}\ndevice_id {}\n", n.vendor, n.device),
        )?;
    }
    // nó de CPU (sem gfx)
    let cpu = root.join("class/kfd/kfd/topology/nodes/0");
    std::fs::create_dir_all(&cpu)?;
    std::fs::write(cpu.join("properties"), "cpu_cores_count 8\nsimd_count 0\ngfx_target_version 0\nvendor_id 0\ndevice_id 0\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_arvore_falsa() {
        let dir = tempfile::tempdir().unwrap();
        let cards = vec![
            SysfsCard {
                vendor: 0x1002,
                device: 0x73ff,
                driver: Some("amdgpu".into()),
                vram_total_mb: Some(8176),
                vram_usada_mb: Some(400),
                pci_slot: Some("0000:03:00.0".into()),
            },
            SysfsCard {
                vendor: 0x8086,
                device: 0x4680,
                driver: Some("i915".into()),
                pci_slot: Some("0000:00:02.0".into()),
                ..Default::default()
            },
        ];
        let kfd = vec![KfdNode { vendor: 4098, device: 0x73ff, gfx: Some("gfx1032".into()) }];
        write_fake_sys(dir.path(), &cards, &kfd).unwrap();
        let got = read_drm_cards(dir.path());
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].vendor, 0x1002);
        assert_eq!(got[0].vram_total_mb, Some(8176));
        assert_eq!(got[0].driver.as_deref(), Some("amdgpu"));
        let nodes = read_kfd_nodes(dir.path());
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].gfx.as_deref(), Some("gfx1032"));
        assert_eq!(nodes[0].device, 0x73ff);
    }

    #[test]
    fn sem_sys_nao_quebra() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_drm_cards(dir.path()).is_empty());
        assert!(read_kfd_nodes(dir.path()).is_empty());
    }
}
