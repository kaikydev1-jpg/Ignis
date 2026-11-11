//! Pastas usadas pelo Ignis.
//!
//! - Linux: config em `~/.config/ignis`, dados em `~/.local/share/ignis`
//! - Windows: config e dados em `%APPDATA%\ignis` e `%LOCALAPPDATA%\ignis`
//!
//! A variável `IGNIS_HOME` coloca tudo numa pasta só (usada nos testes).

use std::path::{Path, PathBuf};

use crate::consts::APP_ID;

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    /// Pastas padrão do sistema (ou `IGNIS_HOME`).
    pub fn from_env() -> Self {
        if let Some(home) = std::env::var_os("IGNIS_HOME") {
            return Self::under(PathBuf::from(home));
        }
        let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join(APP_ID);
        let data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join(APP_ID);
        Self { config_dir, data_dir }
    }

    /// Tudo debaixo de uma pasta (testes e modo portátil).
    pub fn under(base: impl AsRef<Path>) -> Self {
        let base = base.as_ref();
        Self { config_dir: base.join("config"), data_dir: base.join("dados") }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }
    pub fn state_file(&self) -> PathBuf {
        self.data_dir.join("estado.json")
    }
    pub fn models_registry(&self) -> PathBuf {
        self.data_dir.join("modelos.json")
    }
    pub fn models_dir(&self) -> PathBuf {
        self.data_dir.join("modelos")
    }
    pub fn engines_dir(&self) -> PathBuf {
        self.data_dir.join("motores")
    }
    pub fn downloads_dir(&self) -> PathBuf {
        self.data_dir.join("baixando")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("registros")
    }
    /// Manifesto de motores mais novo, aceito numa atualização.
    pub fn engines_manifest_override(&self) -> PathBuf {
        self.data_dir.join("motores.json")
    }
    /// Catálogo mais novo, aceito numa atualização.
    pub fn catalog_override(&self) -> PathBuf {
        self.data_dir.join("catalogo.json")
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for d in [&self.config_dir, &self.data_dir, &self.models_dir(), &self.engines_dir(), &self.downloads_dir(), &self.logs_dir()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}

/// Grava um arquivo de forma atômica (escreve num temporário e renomeia).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp-gravando");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Lê JSON de um arquivo; devolve `None` se não existir ou estiver corrompido.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!("arquivo {} ilegível: {e}", path.display());
            None
        }
    }
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_atomic(path, &bytes)
}
