//! Manifesto de motores (versões oficiais do llama.cpp fixadas com SHA-256).

use serde::{Deserialize, Serialize};

const EMBEDDED: &str = include_str!("../../../../manifest/motores.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Cuda,
    Rocm,
    Vulkan,
    /// `llama-server` próprio indicado pela pessoa.
    Proprio,
    Cpu,
}

impl Backend {
    pub fn nome(&self) -> &'static str {
        match self {
            Backend::Cuda => "CUDA (NVIDIA)",
            Backend::Rocm => "ROCm/HIP (AMD)",
            Backend::Vulkan => "Vulkan",
            Backend::Proprio => "llama-server próprio",
            Backend::Cpu => "Processador",
        }
    }
    /// Ordem de preferência antes do teste de velocidade (menor = tenta antes).
    pub fn prioridade(&self) -> u8 {
        match self {
            Backend::Cuda => 10,
            Backend::Rocm => 20,
            Backend::Vulkan => 30,
            Backend::Proprio => 35,
            Backend::Cpu => 90,
        }
    }
    pub fn usa_placa(&self) -> bool {
        !matches!(self, Backend::Cpu)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetFile {
    pub nome: String,
    pub sha256: String,
    pub tamanho: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    pub id: String,
    pub so: String,
    pub arq: String,
    pub backend: Backend,
    pub arquivos: Vec<AssetFile>,
    #[serde(default)]
    pub libs_do_sistema: Vec<String>,
    #[serde(default)]
    pub alvos_gfx: Vec<String>,
    #[serde(default)]
    pub alvos_gfx_oficiais: Option<Vec<String>>,
    #[serde(default)]
    pub driver_minimo: Option<String>,
    #[serde(default)]
    pub dica_falta: Option<String>,
    /// Conjunto de pacotes do sistema ("vulkan" ou "rocm") para montar a dica por distro.
    #[serde(default)]
    pub pacotes: Option<String>,
    #[serde(default)]
    pub nota: Option<String>,
}

impl Variant {
    pub fn tamanho_total(&self) -> u64 {
        self.arquivos.iter().map(|a| a.tamanho).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlamaCppInfo {
    pub versao: String,
    pub numero: u32,
    pub commit: String,
    pub licenca: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineManifest {
    pub versao_manifesto: u32,
    pub llama_cpp: LlamaCppInfo,
    pub url_base: String,
    pub variantes: Vec<Variant>,
}

impl EngineManifest {
    pub fn embedded() -> Self {
        serde_json::from_str(EMBEDDED).expect("manifest/motores.json válido")
    }

    /// Manifesto aceito numa atualização (se houver e for mais novo), senão o embutido.
    pub fn load(override_path: &std::path::Path) -> Self {
        let base = Self::embedded();
        match crate::paths::read_json::<EngineManifest>(override_path) {
            Some(m) if m.llama_cpp.numero > base.llama_cpp.numero && m.validate().is_ok() => m,
            _ => base,
        }
    }

    pub fn variant(&self, id: &str) -> Option<&Variant> {
        self.variantes.iter().find(|v| v.id == id)
    }

    pub fn for_platform(&self, so: &str, arq: &str) -> impl Iterator<Item = &Variant> {
        let so = so.to_string();
        let arq = arq.to_string();
        self.variantes.iter().filter(move |v| v.so == so && v.arq == arq)
    }

    pub fn url(&self, asset: &AssetFile) -> String {
        format!("{}{}", self.url_base, asset.nome)
    }

    /// Todas as bibliotecas do sistema que vale a pena procurar.
    pub fn libs_to_probe(&self, so: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .variantes
            .iter()
            .filter(|x| x.so == so)
            .flat_map(|x| x.libs_do_sistema.iter().cloned())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.url_base.starts_with("https://") {
            return Err("url_base precisa ser https".into());
        }
        for v in &self.variantes {
            if v.arquivos.is_empty() {
                return Err(format!("variante {} sem arquivos", v.id));
            }
            for a in &v.arquivos {
                if a.sha256.len() != 64 || !a.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(format!("SHA-256 inválido em {}", a.nome));
                }
                if a.nome.contains('/') || a.nome.contains('\\') || a.nome.contains("..") {
                    return Err(format!("nome de arquivo inválido: {}", a.nome));
                }
            }
        }
        Ok(())
    }
}

/// Plataforma atual no vocabulário do manifesto.
pub fn current_os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifesto_embutido_e_valido() {
        let m = EngineManifest::embedded();
        m.validate().unwrap();
        assert_eq!(m.llama_cpp.versao, "b12271");
        for id in ["linux-cpu", "linux-vulkan", "linux-rocm", "windows-cpu", "windows-vulkan", "windows-rocm", "windows-cuda-13"] {
            assert!(m.variant(id).is_some(), "{id}");
        }
        assert!(m.libs_to_probe("linux").contains(&"libamdhip64.so.7".to_string()));
    }
}
