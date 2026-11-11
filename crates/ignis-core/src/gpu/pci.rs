//! Tabela curta de placas conhecidas (ID PCI → nome e alvo gfx da AMD).
//! Serve de reserva quando o sistema não tem vulkaninfo/rocminfo (comum no
//! Windows e no Flatpak). Não precisa ser completa: o nome real vem do
//! driver sempre que possível.

pub const VENDOR_AMD: u16 = 0x1002;
pub const VENDOR_NVIDIA: u16 = 0x10de;
pub const VENDOR_INTEL: u16 = 0x8086;

pub struct KnownGpu {
    pub device: u16,
    pub nome: &'static str,
    pub gfx: &'static str,
    pub integrada: bool,
}

const AMD: &[KnownGpu] = &[
    // Polaris (gfx803)
    KnownGpu { device: 0x67df, nome: "AMD Radeon RX 470/480/570/580", gfx: "gfx803", integrada: false },
    KnownGpu { device: 0x6fdf, nome: "AMD Radeon RX 580 2048SP", gfx: "gfx803", integrada: false },
    KnownGpu { device: 0x67ef, nome: "AMD Radeon RX 460/560", gfx: "gfx803", integrada: false },
    KnownGpu { device: 0x67ff, nome: "AMD Radeon RX 550/560", gfx: "gfx803", integrada: false },
    KnownGpu { device: 0x699f, nome: "AMD Radeon RX 550", gfx: "gfx803", integrada: false },
    // Vega
    KnownGpu { device: 0x687f, nome: "AMD Radeon RX Vega 56/64", gfx: "gfx900", integrada: false },
    KnownGpu { device: 0x66af, nome: "AMD Radeon VII", gfx: "gfx906", integrada: false },
    // RDNA1
    KnownGpu { device: 0x731f, nome: "AMD Radeon RX 5600/5700", gfx: "gfx1010", integrada: false },
    KnownGpu { device: 0x7340, nome: "AMD Radeon RX 5500", gfx: "gfx1012", integrada: false },
    // RDNA2
    KnownGpu { device: 0x73bf, nome: "AMD Radeon RX 6800/6900", gfx: "gfx1030", integrada: false },
    KnownGpu { device: 0x73a5, nome: "AMD Radeon RX 6950 XT", gfx: "gfx1030", integrada: false },
    KnownGpu { device: 0x73df, nome: "AMD Radeon RX 6700/6750 XT", gfx: "gfx1031", integrada: false },
    KnownGpu { device: 0x73ff, nome: "AMD Radeon RX 6600/6600 XT", gfx: "gfx1032", integrada: false },
    KnownGpu { device: 0x73ef, nome: "AMD Radeon RX 6650 XT", gfx: "gfx1032", integrada: false },
    KnownGpu { device: 0x743f, nome: "AMD Radeon RX 6400/6500 XT", gfx: "gfx1034", integrada: false },
    // RDNA3
    KnownGpu { device: 0x744c, nome: "AMD Radeon RX 7900 XT/XTX", gfx: "gfx1100", integrada: false },
    KnownGpu { device: 0x747e, nome: "AMD Radeon RX 7700/7800 XT", gfx: "gfx1101", integrada: false },
    KnownGpu { device: 0x7480, nome: "AMD Radeon RX 7600/7600 XT", gfx: "gfx1102", integrada: false },
    KnownGpu { device: 0x7483, nome: "AMD Radeon RX 7600M", gfx: "gfx1102", integrada: false },
    // RDNA4
    KnownGpu { device: 0x7590, nome: "AMD Radeon RX 9060 XT", gfx: "gfx1200", integrada: false },
    KnownGpu { device: 0x7550, nome: "AMD Radeon RX 9070/9070 XT", gfx: "gfx1201", integrada: false },
    // Integradas (APUs)
    KnownGpu { device: 0x164e, nome: "AMD Radeon (Ryzen 7000, integrada)", gfx: "gfx1036", integrada: true },
    KnownGpu { device: 0x1681, nome: "AMD Radeon 680M (integrada)", gfx: "gfx1035", integrada: true },
    KnownGpu { device: 0x15bf, nome: "AMD Radeon 780M (integrada)", gfx: "gfx1103", integrada: true },
    KnownGpu { device: 0x1900, nome: "AMD Radeon 780M (integrada)", gfx: "gfx1103", integrada: true },
    KnownGpu { device: 0x150e, nome: "AMD Radeon 890M (integrada)", gfx: "gfx1150", integrada: true },
    KnownGpu { device: 0x1586, nome: "AMD Radeon 8060S (integrada)", gfx: "gfx1151", integrada: true },
    KnownGpu { device: 0x1638, nome: "AMD Radeon Vega (Ryzen 5000, integrada)", gfx: "gfx90c", integrada: true },
    KnownGpu { device: 0x15d8, nome: "AMD Radeon Vega (Ryzen 3000, integrada)", gfx: "gfx90c", integrada: true },
];

pub fn lookup_amd(device: u16) -> Option<&'static KnownGpu> {
    AMD.iter().find(|g| g.device == device)
}

/// Intel: quase tudo é integrada, exceto Arc (dispositivos 0x56xx e 0xe20x).
pub fn intel_is_integrated(device: u16) -> bool {
    !((0x5690..=0x56ff).contains(&device) || (0xe200..=0xe2ff).contains(&device))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placas_do_briefing() {
        assert_eq!(lookup_amd(0x67df).unwrap().gfx, "gfx803"); // RX 580
        assert_eq!(lookup_amd(0x73ff).unwrap().gfx, "gfx1032"); // RX 6600
        assert_eq!(lookup_amd(0x7480).unwrap().gfx, "gfx1102"); // RX 7600
        assert_eq!(lookup_amd(0x7590).unwrap().gfx, "gfx1200"); // RX 9060 XT
        assert!(lookup_amd(0x15bf).unwrap().integrada);
        assert!(lookup_amd(0x1234).is_none());
    }

    #[test]
    fn intel_arc_nao_e_integrada() {
        assert!(!intel_is_integrated(0x56a0));
        assert!(intel_is_integrated(0x46a6));
    }
}
