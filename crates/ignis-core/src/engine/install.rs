//! Instalação dos motores: baixa os arquivos do release oficial, confere o
//! SHA-256 de cada um, extrai numa pasta temporária e só então troca.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::manifest::{EngineManifest, Variant};
use crate::download::{self, DownloadError, Progress};

pub const MARKER: &str = ".instalado.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstalledEngine {
    pub versao: String,
    pub variante: String,
    pub pasta: PathBuf,
    pub executavel: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    versao: String,
    variante: String,
    sha256: Vec<String>,
    executavel: String,
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error("o pacote do motor está com defeito: {0}")]
    Archive(String),
    #[error("o pacote do motor não tem o llama-server")]
    NoServer,
    #[error("erro de disco: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct EngineStore {
    root: PathBuf,
    downloads: PathBuf,
    /// Arquivos que vão junto em toda pasta de motor (no Windows: as DLLs do
    /// Visual C++ que o llama-server oficial precisa e que nem todo PC tem).
    extras: Option<PathBuf>,
}

pub fn server_exe_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

impl EngineStore {
    pub fn new(root: PathBuf, downloads: PathBuf) -> Self {
        Self { root, downloads, extras: None }
    }

    /// Define a pasta de arquivos extras copiados para cada motor.
    pub fn with_extras(mut self, extras: Option<PathBuf>) -> Self {
        self.extras = extras.filter(|p| p.is_dir());
        self
    }

    /// Copia os extras que faltarem para a pasta do motor.
    pub fn apply_extras(&self, dir: &Path) {
        let Some(src) = &self.extras else { return };
        let Ok(rd) = std::fs::read_dir(src) else { return };
        for e in rd.flatten() {
            let dest = dir.join(e.file_name());
            if e.path().is_file() && !dest.exists() {
                if let Err(err) = std::fs::copy(e.path(), &dest) {
                    tracing::warn!("não copiei {}: {err}", dest.display());
                }
            }
        }
    }

    pub fn variant_dir(&self, versao: &str, variante: &str) -> PathBuf {
        self.root.join(versao).join(variante)
    }

    /// Motor instalado e íntegro (o marcador confere com o manifesto)?
    pub fn installed(&self, m: &EngineManifest, v: &Variant) -> Option<InstalledEngine> {
        let dir = self.variant_dir(&m.llama_cpp.versao, &v.id);
        let marker: Marker = crate::paths::read_json(&dir.join(MARKER))?;
        let esperado: Vec<String> = v.arquivos.iter().map(|a| a.sha256.to_lowercase()).collect();
        if marker.sha256 != esperado || marker.versao != m.llama_cpp.versao {
            return None;
        }
        let exe = dir.join(&marker.executavel);
        if exe.is_file() {
            self.apply_extras(&dir);
        }
        exe.is_file().then(|| InstalledEngine { versao: m.llama_cpp.versao.clone(), variante: v.id.clone(), pasta: dir, executavel: exe })
    }

    /// Baixa (com retomada) e instala. `progress` recebe bytes somados de todos os arquivos.
    pub async fn install(
        &self,
        client: &reqwest::Client,
        m: &EngineManifest,
        v: &Variant,
        cancel: &CancellationToken,
        progress: &(dyn Fn(Progress) + Send + Sync),
    ) -> Result<InstalledEngine, InstallError> {
        if let Some(e) = self.installed(m, v) {
            return Ok(e);
        }
        let total: u64 = v.tamanho_total();
        let dl_dir = self.downloads.join("motores");
        std::fs::create_dir_all(&dl_dir)?;
        let mut done_before = 0u64;
        let mut archives = Vec::new();
        for a in &v.arquivos {
            let dest = dl_dir.join(&a.nome);
            if !dest.exists() || std::fs::metadata(&dest).map(|md| md.len()).unwrap_or(0) != a.tamanho {
                let base = done_before;
                let cb = move |p: Progress| progress(Progress { baixado: base + p.baixado, total: Some(total) });
                download::download_file(client, &m.url(a), &dest, Some(&a.sha256), Some(a.tamanho), cancel, &cb, None).await?;
            } else {
                // arquivo já baixado: confere de novo antes de usar
                let d2 = dest.clone();
                let sha = tokio::task::spawn_blocking(move || download::sha256_file(&d2))
                    .await
                    .map_err(|e| InstallError::Io(std::io::Error::other(e)))??;
                if !sha.eq_ignore_ascii_case(&a.sha256) {
                    std::fs::remove_file(&dest)?;
                    return Err(InstallError::Download(DownloadError::HashMismatch { esperado: a.sha256.clone(), obtido: sha }));
                }
            }
            done_before += a.tamanho;
            progress(Progress { baixado: done_before, total: Some(total) });
            archives.push(dest);
        }

        let final_dir = self.variant_dir(&m.llama_cpp.versao, &v.id);
        let staging = final_dir.with_file_name(format!(".{}.instalando", v.id));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let staging2 = staging.clone();
        let archives2 = archives.clone();
        tokio::task::spawn_blocking(move || -> Result<(), InstallError> {
            for a in &archives2 {
                extract(a, &staging2)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| InstallError::Io(std::io::Error::other(e)))??;

        let exe_name = server_exe_name();
        if !staging.join(exe_name).is_file() {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(InstallError::NoServer);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let p = staging.join(exe_name);
            let mut perm = std::fs::metadata(&p)?.permissions();
            perm.set_mode(perm.mode() | 0o755);
            std::fs::set_permissions(&p, perm)?;
        }
        let marker = Marker {
            versao: m.llama_cpp.versao.clone(),
            variante: v.id.clone(),
            sha256: v.arquivos.iter().map(|a| a.sha256.to_lowercase()).collect(),
            executavel: exe_name.to_string(),
        };
        crate::paths::write_json(&staging.join(MARKER), &marker)?;
        let _ = std::fs::remove_dir_all(&final_dir);
        std::fs::create_dir_all(final_dir.parent().unwrap())?;
        std::fs::rename(&staging, &final_dir)?;
        for a in archives {
            let _ = std::fs::remove_file(a);
        }
        self.installed(m, v).ok_or(InstallError::NoServer)
    }

    /// Instala a partir de um executável local (testes com a placa simulada
    /// e instalação manual). Copia o executável e os arquivos vizinhos indicados.
    pub fn install_from_files(
        &self,
        m: &EngineManifest,
        v: &Variant,
        exe: &Path,
        extras: &[(&str, &[u8])],
    ) -> std::io::Result<InstalledEngine> {
        let dir = self.variant_dir(&m.llama_cpp.versao, &v.id);
        std::fs::create_dir_all(&dir)?;
        let exe_name = server_exe_name();
        std::fs::copy(exe, dir.join(exe_name))?;
        for (name, bytes) in extras {
            std::fs::write(dir.join(name), bytes)?;
        }
        let marker = Marker {
            versao: m.llama_cpp.versao.clone(),
            variante: v.id.clone(),
            sha256: v.arquivos.iter().map(|a| a.sha256.to_lowercase()).collect(),
            executavel: exe_name.to_string(),
        };
        crate::paths::write_json(&dir.join(MARKER), &marker)?;
        self.installed(m, v).ok_or_else(|| std::io::Error::other("instalação manual falhou"))
    }

    pub fn remove(&self, versao: &str, variante: &str) -> std::io::Result<()> {
        let d = self.variant_dir(versao, variante);
        if d.exists() {
            std::fs::remove_dir_all(d)?;
        }
        Ok(())
    }

    /// Apaga motores de versões antigas do llama.cpp.
    pub fn remove_other_versions(&self, keep: &str) {
        if let Ok(rd) = std::fs::read_dir(&self.root) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy() != keep && e.path().is_dir() {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }

    /// Espaço usado pelos motores.
    pub fn disk_usage(&self) -> u64 {
        dir_size(&self.root)
    }

    /// Parte já baixada de uma variante (para "Retomar").
    pub fn partial_bytes(&self, v: &Variant) -> u64 {
        let dl = self.downloads.join("motores");
        v.arquivos
            .iter()
            .map(|a| {
                let d = dl.join(&a.nome);
                std::fs::metadata(&d).map(|m| m.len()).ok().or_else(|| download::partial_size(&d)).unwrap_or(0)
            })
            .sum()
    }
}

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| {
            let md = e.metadata();
            match md {
                Ok(m) if m.is_dir() => dir_size(&e.path()),
                Ok(m) => m.len(),
                Err(_) => 0,
            }
        })
        .sum()
}

/// Caminho relativo seguro: sem "..", sem raiz, e tirando a pasta de topo
/// dos pacotes do llama.cpp ("llama-b12271/", "cudart-llama-.../").
fn safe_relative(p: &Path) -> Option<PathBuf> {
    let comps: Vec<Component> = p.components().collect();
    let mut out = PathBuf::new();
    for (i, c) in comps.iter().enumerate() {
        match c {
            Component::Normal(s) => {
                let s = s.to_string_lossy();
                if i == 0 && comps.len() > 1 && (s.starts_with("llama-") || s.starts_with("cudart-")) {
                    continue;
                }
                out.push(s.as_ref());
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

pub(crate) fn extract(archive: &Path, dest: &Path) -> Result<(), InstallError> {
    let name = archive.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if name.ends_with(".zip") {
        extract_zip(archive, dest)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive, dest)
    } else {
        Err(InstallError::Archive(format!("formato desconhecido: {name}")))
    }
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<(), InstallError> {
    let f = std::fs::File::open(archive)?;
    let gz = flate2::read::GzDecoder::new(std::io::BufReader::new(f));
    let mut ar = tar::Archive::new(gz);
    ar.set_preserve_permissions(true);
    for entry in ar.entries().map_err(|e| InstallError::Archive(e.to_string()))? {
        let mut entry = entry.map_err(|e| InstallError::Archive(e.to_string()))?;
        let path = entry.path().map_err(|e| InstallError::Archive(e.to_string()))?.into_owned();
        let Some(rel) = safe_relative(&path) else { continue };
        let kind = entry.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            if let Ok(Some(target)) = entry.link_name() {
                // links só para arquivos vizinhos (libggml.so → libggml.so.0)
                if target.components().any(|c| !matches!(c, Component::Normal(_))) {
                    continue;
                }
            }
        }
        let out = dest.join(&rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if kind.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        let _ = std::fs::remove_file(&out);
        entry.unpack(&out).map_err(|e| InstallError::Archive(e.to_string()))?;
    }
    Ok(())
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<(), InstallError> {
    let f = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(f)).map_err(|e| InstallError::Archive(e.to_string()))?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| InstallError::Archive(e.to_string()))?;
        let Some(name) = file.enclosed_name() else { continue };
        let Some(rel) = safe_relative(&name) else { continue };
        let out = dest.join(rel);
        if file.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut w = std::fs::File::create(&out)?;
        std::io::copy(&mut file, &mut w)?;
        #[cfg(unix)]
        if let Some(mode) = file.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extras_vao_para_a_pasta_do_motor() {
        let tmp = tempfile::tempdir().unwrap();
        let extras = tmp.path().join("vcrt");
        std::fs::create_dir_all(&extras).unwrap();
        std::fs::write(extras.join("vcruntime140.dll"), b"dll").unwrap();
        let motor = tmp.path().join("motor");
        std::fs::create_dir_all(&motor).unwrap();
        std::fs::write(motor.join("msvcp140.dll"), b"ja-existia").unwrap();
        std::fs::write(extras.join("msvcp140.dll"), b"nova").unwrap();
        let store = EngineStore::new(tmp.path().join("m"), tmp.path().join("d")).with_extras(Some(extras));
        store.apply_extras(&motor);
        assert_eq!(std::fs::read(motor.join("vcruntime140.dll")).unwrap(), b"dll");
        assert_eq!(std::fs::read(motor.join("msvcp140.dll")).unwrap(), b"ja-existia", "não sobrescreve");
        // pasta de extras inexistente é ignorada
        let s2 = EngineStore::new(tmp.path().join("m"), tmp.path().join("d")).with_extras(Some(tmp.path().join("nao-existe")));
        s2.apply_extras(&motor);
    }

    #[test]
    fn caminhos_seguros() {
        assert_eq!(safe_relative(Path::new("llama-b12271/llama-server")), Some(PathBuf::from("llama-server")));
        assert_eq!(safe_relative(Path::new("cudart-llama-b1-bin/libcudart.so.12")), Some(PathBuf::from("libcudart.so.12")));
        assert_eq!(safe_relative(Path::new("llama-server.exe")), Some(PathBuf::from("llama-server.exe")));
        assert_eq!(safe_relative(Path::new("LICENSE")), Some(PathBuf::from("LICENSE")));
        assert_eq!(safe_relative(Path::new("../etc/passwd")), None);
        assert_eq!(safe_relative(Path::new("/etc/passwd")), None);
        assert_eq!(safe_relative(Path::new("llama-b1/sub/../x")), None);
    }
}
