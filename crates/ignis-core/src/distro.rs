//! Identifica o sistema (distro Linux, Fedora Atomic, Flatpak, AppImage) para
//! dar dicas com o comando certo de cada um.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Familia {
    Fedora,
    FedoraAtomic,
    Debian,
    Arch,
    Suse,
    OutraLinux,
    Windows,
    Mac,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sistema {
    pub familia: Familia,
    /// Nome bonito ("Fedora Linux 44 (Silverblue)", "Windows").
    pub nome: String,
    pub versao: Option<String>,
    /// Rodando dentro de um Flatpak.
    pub flatpak: bool,
    /// Rodando a partir de um AppImage.
    pub appimage: bool,
}

/// Campos de /etc/os-release que interessam.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct OsRelease {
    pub id: String,
    pub name: Option<String>,
    pub id_like: Vec<String>,
    pub pretty_name: Option<String>,
    pub version_id: Option<String>,
    pub variant_id: Option<String>,
}

pub fn parse_os_release(text: &str) -> OsRelease {
    let mut r = OsRelease::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
        match k.trim() {
            "ID" => r.id = v.to_lowercase(),
            "ID_LIKE" => r.id_like = v.split_whitespace().map(|s| s.to_lowercase()).collect(),
            "NAME" => r.name = Some(v),
            "PRETTY_NAME" => r.pretty_name = Some(v),
            "VERSION_ID" => r.version_id = Some(v),
            "VARIANT_ID" => r.variant_id = Some(v.to_lowercase()),
            _ => {}
        }
    }
    r
}

/// Decide a família a partir do os-release e de sinais do sistema.
/// `ostree` = existe /run/ostree-booted (Fedora Atomic, Bazzite, Bluefin...).
pub fn classify(os: &OsRelease, ostree: bool) -> Familia {
    let is = |name: &str| os.id == name || os.id_like.iter().any(|x| x == name);
    let atomic_variant = matches!(
        os.variant_id.as_deref(),
        Some("silverblue" | "kinoite" | "sericea" | "onyx" | "cosmic-atomic" | "atomic" | "iot" | "coreos")
    );
    if is("fedora") || os.id == "bazzite" || os.id == "bluefin" || os.id == "aurora" || eh_matsu(os) {
        if ostree || atomic_variant {
            return Familia::FedoraAtomic;
        }
        return Familia::Fedora;
    }
    if is("debian") || is("ubuntu") {
        return Familia::Debian;
    }
    if is("arch") {
        return Familia::Arch;
    }
    if is("suse") || os.id.starts_with("opensuse") {
        return Familia::Suse;
    }
    Familia::OutraLinux
}

/// Matsu OS (do criador do Ignis), baseado no Fedora Atomic.
pub fn eh_matsu(os: &OsRelease) -> bool {
    let tem = |s: &str| s.to_lowercase().replace([' ', '-', '_'], "").contains("matsu");
    tem(&os.id) || os.name.as_deref().is_some_and(tem) || os.pretty_name.as_deref().is_some_and(tem)
}

/// Lê o os-release do sistema (no Flatpak, o do sistema de verdade, em /run/host).
pub fn ler_os_release() -> OsRelease {
    let candidates = ["/run/host/os-release", "/run/host/etc/os-release", "/etc/os-release", "/usr/lib/os-release"];
    let text = candidates.iter().find_map(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    parse_os_release(&text)
}

impl Sistema {
    pub fn detect() -> Self {
        let flatpak = std::env::var_os("FLATPAK_ID").is_some() || std::path::Path::new("/.flatpak-info").exists();
        let appimage = std::env::var_os("APPIMAGE").is_some();
        if cfg!(windows) {
            return Self { familia: Familia::Windows, nome: "Windows".into(), versao: None, flatpak, appimage };
        }
        if cfg!(target_os = "macos") {
            return Self { familia: Familia::Mac, nome: "macOS".into(), versao: None, flatpak, appimage };
        }
        // Dentro do Flatpak o /etc/os-release é do runtime; o do sistema fica em /run/host.
        let os = ler_os_release();
        let ostree = std::path::Path::new("/run/ostree-booted").exists();
        Self {
            familia: classify(&os, ostree),
            nome: os.pretty_name.clone().unwrap_or_else(|| "Linux".into()),
            versao: os.version_id.clone(),
            flatpak,
            appimage,
        }
    }

    /// Comando para instalar pacotes do sistema, por família.
    pub fn comando_instalar(&self, pacotes: &Pacotes) -> Option<String> {
        let (cmd, list) = match self.familia {
            Familia::Fedora => ("sudo dnf install", pacotes.fedora),
            Familia::FedoraAtomic => ("sudo rpm-ostree install", pacotes.fedora),
            Familia::Debian => ("sudo apt install", pacotes.debian),
            Familia::Arch => ("sudo pacman -S", pacotes.arch),
            Familia::Suse => ("sudo zypper install", pacotes.suse),
            _ => return None,
        };
        if list.is_empty() {
            return None;
        }
        let mut s = format!("{cmd} {}", list.join(" "));
        if self.familia == Familia::FedoraAtomic {
            s.push_str("  (depois reinicie o computador)");
        }
        Some(s)
    }
}

/// Nomes de pacote equivalentes em cada família.
pub struct Pacotes {
    pub fedora: &'static [&'static str],
    pub debian: &'static [&'static str],
    pub arch: &'static [&'static str],
    pub suse: &'static [&'static str],
}

pub const PACOTES_VULKAN: Pacotes = Pacotes {
    fedora: &["vulkan-loader", "mesa-vulkan-drivers"],
    debian: &["libvulkan1", "mesa-vulkan-drivers"],
    arch: &["vulkan-icd-loader", "vulkan-radeon"],
    suse: &["libvulkan1", "libvulkan_radeon"],
};

pub const PACOTES_ROCM: Pacotes =
    Pacotes { fedora: &["rocm-hip", "hipblas", "rocblas"], debian: &[], arch: &["rocm-hip-runtime", "hipblas", "rocblas"], suse: &[] };

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matsu_os_e_reconhecido_e_vira_fedora_atomic() {
        let os = parse_os_release("NAME=\"Matsu OS\"\nID=matsu\nID_LIKE=\"fedora\"\nPRETTY_NAME=\"Matsu OS 44 (Atomic)\"\n");
        assert!(eh_matsu(&os));
        assert_eq!(classify(&os, true), Familia::FedoraAtomic);
        // mesmo sem ID_LIKE, o nome basta
        let os = parse_os_release("NAME=\"MatsuOS\"\nID=matsuos\n");
        assert!(eh_matsu(&os));
        assert_eq!(classify(&os, true), Familia::FedoraAtomic);
        assert!(!eh_matsu(&parse_os_release(FEDORA_44)));
    }

    const FEDORA_44: &str = r#"NAME="Fedora Linux"
VERSION="44 (Workstation Edition)"
ID=fedora
VERSION_ID=44
PRETTY_NAME="Fedora Linux 44 (Workstation Edition)"
VARIANT_ID=workstation
"#;
    const SILVERBLUE_44: &str = r#"NAME="Fedora Linux"
VERSION="44 (Silverblue)"
ID=fedora
VERSION_ID=44
PRETTY_NAME="Fedora Linux 44 (Silverblue)"
VARIANT="Silverblue"
VARIANT_ID=silverblue
"#;
    const BAZZITE: &str = "ID=bazzite\nID_LIKE=\"fedora\"\nPRETTY_NAME=\"Bazzite 44\"\nVERSION_ID=44\n";
    const UBUNTU: &str = "ID=ubuntu\nID_LIKE=debian\nPRETTY_NAME=\"Ubuntu 24.04.3 LTS\"\nVERSION_ID=\"24.04\"\n";
    const MINT: &str = "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\nPRETTY_NAME=\"Linux Mint 22\"\n";
    const ARCH: &str = "ID=arch\nPRETTY_NAME=\"Arch Linux\"\n";
    const CACHY: &str = "ID=cachyos\nID_LIKE=arch\nPRETTY_NAME=\"CachyOS\"\n";

    #[test]
    fn classifica_distros() {
        assert_eq!(classify(&parse_os_release(FEDORA_44), false), Familia::Fedora);
        assert_eq!(classify(&parse_os_release(SILVERBLUE_44), false), Familia::FedoraAtomic);
        assert_eq!(classify(&parse_os_release(FEDORA_44), true), Familia::FedoraAtomic, "ostree manda");
        assert_eq!(classify(&parse_os_release(BAZZITE), true), Familia::FedoraAtomic);
        assert_eq!(classify(&parse_os_release(UBUNTU), false), Familia::Debian);
        assert_eq!(classify(&parse_os_release(MINT), false), Familia::Debian);
        assert_eq!(classify(&parse_os_release(ARCH), false), Familia::Arch);
        assert_eq!(classify(&parse_os_release(CACHY), false), Familia::Arch);
        assert_eq!(classify(&parse_os_release("ID=void\n"), false), Familia::OutraLinux);
    }

    #[test]
    fn comando_certo_por_familia() {
        let mk = |f| Sistema { familia: f, nome: String::new(), versao: None, flatpak: false, appimage: false };
        assert_eq!(mk(Familia::Fedora).comando_instalar(&PACOTES_ROCM).unwrap(), "sudo dnf install rocm-hip hipblas rocblas");
        let atomic = mk(Familia::FedoraAtomic).comando_instalar(&PACOTES_ROCM).unwrap();
        assert!(atomic.starts_with("sudo rpm-ostree install rocm-hip"));
        assert!(atomic.contains("reinicie"));
        assert!(mk(Familia::Debian).comando_instalar(&PACOTES_ROCM).is_none(), "sem ROCm compatível no apt");
        assert!(mk(Familia::Debian).comando_instalar(&PACOTES_VULKAN).unwrap().contains("mesa-vulkan-drivers"));
    }
}
