//! Detecção com saídas salvas das placas do briefing.

mod comum;

use comum::fixture;
use ignis_core::gpu::{merge, Vendor};

#[test]
fn rx580_linux() {
    let g = merge(&fixture("rx580"));
    assert_eq!(g.len(), 1);
    let p = &g[0];
    assert_eq!(p.fabricante, Vendor::Amd);
    assert_eq!(p.nome, "AMD Radeon RX 580 Series");
    assert_eq!(p.alvo_gfx.as_deref(), Some("gfx803"));
    assert_eq!(p.vram_total_mb, Some(8192));
    assert_eq!(p.vram_livre_mb, Some(8192 - 310));
    assert_eq!(p.versao_driver.as_deref(), Some("Mesa 25.2.4 (radv)"));
    assert!(!p.integrada);
}

#[test]
fn rx6600_linux_com_rocminfo() {
    let g = merge(&fixture("rx6600"));
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].nome, "AMD Radeon RX 6600");
    assert_eq!(g[0].alvo_gfx.as_deref(), Some("gfx1032"));
    assert!(g[0].fontes.contains(&"rocminfo".to_string()));
    assert_eq!(g[0].vram_total_mb, Some(8176));
}

#[test]
fn rx7600_linux() {
    let g = merge(&fixture("rx7600"));
    assert_eq!(g[0].alvo_gfx.as_deref(), Some("gfx1102"));
    assert_eq!(g[0].nome, "AMD Radeon RX 7600");
}

#[test]
fn rx9060xt_linux_com_integrada() {
    let d = comum::detection("rx9060xt");
    assert_eq!(d.placas.len(), 2);
    let p = d.principal().unwrap();
    assert_eq!(p.nome, "AMD Radeon RX 9060 XT");
    assert_eq!(p.alvo_gfx.as_deref(), Some("gfx1200"));
    assert_eq!(p.vram_total_mb, Some(16304));
    let igpu = d.placas.iter().find(|g| g.integrada).expect("integrada detectada");
    assert_eq!(igpu.alvo_gfx.as_deref(), Some("gfx1036"));
}

#[test]
fn rx9060xt_windows_dxgi() {
    let g = merge(&fixture("rx9060xt-windows"));
    assert_eq!(g.len(), 1, "ignora o Microsoft Basic Render Driver");
    assert_eq!(g[0].nome, "AMD Radeon RX 9060 XT");
    assert_eq!(g[0].alvo_gfx.as_deref(), Some("gfx1200"), "gfx vem da tabela PCI");
    assert_eq!(g[0].vram_livre_mb, Some(15200));
    assert_eq!(g[0].versao_driver.as_deref(), Some("32.0.21025.1024"));
}

#[test]
fn rx580_windows_dxgi() {
    let g = merge(&fixture("rx580-windows"));
    assert_eq!(g[0].alvo_gfx.as_deref(), Some("gfx803"));
    assert_eq!(g[0].vram_total_mb, Some(8192));
}

#[test]
fn nvidia_rtx3060() {
    let g = merge(&fixture("rtx3060"));
    assert_eq!(g.len(), 1);
    let p = &g[0];
    assert_eq!(p.fabricante, Vendor::Nvidia);
    assert_eq!(p.nome, "NVIDIA GeForce RTX 3060");
    assert_eq!(p.vram_total_mb, Some(12288));
    assert_eq!(p.vram_livre_mb, Some(11653));
    assert_eq!(p.versao_driver.as_deref(), Some("580.95.05"));
    assert_eq!(p.pci_dispositivo, Some(0x2504));
    assert!(p.alvo_gfx.is_none());
}

#[test]
fn sem_placa() {
    let d = comum::detection("sem-placa");
    assert!(d.principal().is_none(), "VM sem placa: só processador");
}

#[test]
fn impressao_digital_muda_com_driver() {
    let a = comum::detection("rx6600");
    let mut b = comum::detection("rx6600");
    assert_eq!(a.fingerprint(), b.fingerprint());
    b.placas[0].versao_driver = Some("Mesa 25.3.0 (radv)".into());
    assert_ne!(a.fingerprint(), b.fingerprint());
}
