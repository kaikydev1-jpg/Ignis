//! "Abrir junto com o computador".
//!
//! - Windows: valor na chave Run do usuário (HKCU\Software\Microsoft\Windows\CurrentVersion\Run)
//! - Linux: arquivo `.desktop` em `~/.config/autostart` (vale para GNOME, KDE, XFCE...)
//!   * AppImage: aponta para o próprio arquivo .AppImage (`$APPIMAGE`)
//!   * Flatpak: usa `flatpak run <id>` (precisa da permissão xdg-config/autostart)
//!
//! Não é serviço do sistema: só abre o aplicativo normal, minimizado na bandeja.

use std::path::{Path, PathBuf};

use crate::consts::{APP_ID, APP_NAME};

pub const ARG_MINIMIZADO: &str = "--minimizado";

/// Como iniciar o Ignis no login.
#[derive(Debug, Clone, PartialEq)]
pub enum Launcher {
    Exe(PathBuf),
    Flatpak(String),
}

impl Launcher {
    /// Descobre a forma certa a partir do ambiente atual.
    pub fn detect() -> Option<Self> {
        if let Ok(id) = std::env::var("FLATPAK_ID") {
            return Some(Launcher::Flatpak(id));
        }
        if let Some(app) = std::env::var_os("APPIMAGE") {
            return Some(Launcher::Exe(PathBuf::from(app)));
        }
        std::env::current_exe().ok().map(Launcher::Exe)
    }

    fn command_line(&self) -> String {
        match self {
            Launcher::Exe(p) => {
                let s = p.to_string_lossy();
                if cfg!(windows) {
                    format!("\"{s}\" {ARG_MINIMIZADO}")
                } else {
                    // aspas no Exec do .desktop
                    format!("\"{}\" {ARG_MINIMIZADO}", s.replace('\\', "\\\\").replace('"', "\\\""))
                }
            }
            Launcher::Flatpak(id) => format!("flatpak run {id} {ARG_MINIMIZADO}"),
        }
    }
}

fn autostart_dir() -> Option<PathBuf> {
    // Dentro do Flatpak, XDG_CONFIG_HOME aponta para a pasta do sandbox; o
    // autostart de verdade fica em ~/.config/autostart.
    if std::env::var_os("FLATPAK_ID").is_some() {
        return dirs::home_dir().map(|h| h.join(".config/autostart"));
    }
    dirs::config_dir().map(|c| c.join("autostart"))
}

pub fn desktop_file_path() -> Option<PathBuf> {
    autostart_dir().map(|d| d.join(format!("{APP_ID}.desktop")))
}

pub fn desktop_entry(l: &Launcher) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={APP_NAME}\nComment=Motor de IA local\nExec={}\nIcon={}\nTerminal=false\nCategories=Utility;\nX-GNOME-Autostart-enabled=true\nX-KDE-autostart-after=panel\n",
        l.command_line(),
        match l {
            Launcher::Flatpak(id) => id.clone(),
            Launcher::Exe(_) => APP_ID.to_string(),
        }
    )
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::RegKey;
    const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

    pub fn enable(l: &Launcher) -> std::io::Result<()> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN)?;
        key.set_value(APP_NAME, &l.command_line())
    }
    pub fn disable() -> std::io::Result<()> {
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN, KEY_SET_VALUE)?;
        match key.delete_value(APP_NAME) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
    pub fn is_enabled() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN, KEY_READ)
            .and_then(|k| k.get_value::<String, _>(APP_NAME))
            .is_ok()
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn enable(l: &Launcher) -> std::io::Result<()> {
        let p = desktop_file_path().ok_or_else(|| std::io::Error::other("sem pasta de configuração"))?;
        write_entry(&p, l)
    }
    pub fn disable() -> std::io::Result<()> {
        if let Some(p) = desktop_file_path() {
            if p.exists() {
                std::fs::remove_file(p)?;
            }
        }
        Ok(())
    }
    pub fn is_enabled() -> bool {
        desktop_file_path().map(|p| p.exists()).unwrap_or(false)
    }
}

pub fn write_entry(path: &Path, l: &Launcher) -> std::io::Result<()> {
    crate::paths::write_atomic(path, desktop_entry(l).as_bytes())
}

pub fn set(enabled: bool, l: Option<&Launcher>) -> std::io::Result<()> {
    if enabled {
        match l {
            Some(l) => imp::enable(l),
            None => Err(std::io::Error::other("não sei o caminho do aplicativo")),
        }
    } else {
        imp::disable()
    }
}

pub fn is_enabled() -> bool {
    imp::is_enabled()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrada_desktop_do_appimage() {
        let e = desktop_entry(&Launcher::Exe("/home/ana/Apps/Ignis.AppImage".into()));
        assert!(e.contains("Exec=\"/home/ana/Apps/Ignis.AppImage\" --minimizado"));
        assert!(e.contains("X-GNOME-Autostart-enabled=true"));
    }

    #[test]
    fn entrada_desktop_do_flatpak() {
        let e = desktop_entry(&Launcher::Flatpak("io.github.kaikydev1jpg.Ignis".into()));
        assert!(e.contains("Exec=flatpak run io.github.kaikydev1jpg.Ignis --minimizado"));
        assert!(e.contains("Icon=io.github.kaikydev1jpg.Ignis"));
    }

    #[test]
    fn grava_e_apaga() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("autostart/ignis.desktop");
        write_entry(&p, &Launcher::Exe("/usr/bin/ignis".into())).unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("Exec=\"/usr/bin/ignis\" --minimizado"));
    }
}
