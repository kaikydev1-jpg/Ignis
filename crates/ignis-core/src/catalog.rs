//! Catálogo curto de modelos escolhidos a dedo (fixados por revisão e SHA-256).

use serde::{Deserialize, Serialize};

use crate::budget::{self, Encaixe, ModelShape, Recursos};

const EMBEDDED: &str = include_str!("../../../manifest/catalogo.json");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arquitetura {
    pub camadas: u32,
    pub cabecas_kv: u32,
    pub dim_k: u32,
    pub dim_v: u32,
    pub embd: u32,
    pub contexto_max: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub nome: String,
    /// "conversa", "codigo" ou "embeddings"
    pub tipo: String,
    pub parametros_bilhoes: f64,
    pub quantizacao: String,
    pub repo: String,
    pub revisao: String,
    pub arquivo: String,
    pub tamanho: u64,
    pub sha256: String,
    pub licenca: String,
    pub licenca_url: String,
    pub uso: String,
    pub arquitetura: Arquitetura,
    #[serde(default)]
    pub modelo_de_teste: bool,
    /// Endereço alternativo (usado nos testes com servidor local).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl CatalogModel {
    pub fn download_url(&self) -> String {
        self.url.clone().unwrap_or_else(|| format!("https://huggingface.co/{}/resolve/{}/{}", self.repo, self.revisao, self.arquivo))
    }
    pub fn embeddings(&self) -> bool {
        self.tipo == "embeddings"
    }
    pub fn shape(&self) -> ModelShape {
        let a = &self.arquitetura;
        ModelShape {
            tamanho_arquivo: self.tamanho,
            camadas: a.camadas,
            kv_bytes_por_token: crate::gguf::kv_bytes_por_token(a.camadas, a.cabecas_kv, a.dim_k, a.dim_v),
            contexto_max: a.contexto_max,
            embd: a.embd,
        }
    }
    /// Quão bem roda com os recursos dados.
    pub fn encaixe(&self, rec: &Recursos, contexto: u32) -> Encaixe {
        budget::plan(&self.shape(), rec, contexto).encaixe
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub versao: u32,
    pub modelos: Vec<CatalogModel>,
}

impl Catalog {
    pub fn embedded() -> Self {
        serde_json::from_str(EMBEDDED).expect("manifest/catalogo.json válido")
    }

    pub fn load(override_path: &std::path::Path) -> Self {
        let base = Self::embedded();
        match crate::paths::read_json::<Catalog>(override_path) {
            Some(c) if c.versao > base.versao && c.validate().is_ok() => c,
            _ => base,
        }
    }

    pub fn get(&self, id: &str) -> Option<&CatalogModel> {
        self.modelos.iter().find(|m| m.id == id)
    }

    /// Modelo pequeno usado no teste de velocidade.
    pub fn test_model(&self) -> Option<&CatalogModel> {
        self.modelos.iter().find(|m| m.modelo_de_teste).or_else(|| self.modelos.iter().min_by_key(|m| m.tamanho))
    }

    pub fn validate(&self) -> Result<(), String> {
        for m in &self.modelos {
            if m.sha256.len() != 64 || !m.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("SHA-256 inválido em {}", m.id));
            }
            if !m.arquivo.ends_with(".gguf") || m.arquivo.contains("..") || m.arquivo.contains('\\') {
                return Err(format!("arquivo inválido em {}", m.id));
            }
            if let Some(u) = &m.url {
                if !(u.starts_with("https://") || u.starts_with("http://127.0.0.1")) {
                    return Err(format!("url inválida em {}", m.id));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogo_embutido_valido() {
        let c = Catalog::embedded();
        c.validate().unwrap();
        assert!(c.modelos.len() >= 8 && c.modelos.len() <= 15, "catálogo curto");
        assert_eq!(c.test_model().unwrap().id, "qwen3-0.6b");
        let q = c.get("qwen3-8b").unwrap();
        assert!(q.download_url().starts_with("https://huggingface.co/Qwen/Qwen3-8B-GGUF/resolve/"));
        assert_eq!(q.shape().kv_bytes_por_token, 147_456);
        assert!(c.get("qwen3-embedding-0.6b").unwrap().embeddings());
        for m in &c.modelos {
            assert!(!m.uso.is_empty() && !m.licenca.is_empty(), "{} sem descrição/licença", m.id);
        }
    }

    #[test]
    fn filtro_por_placa() {
        let c = Catalog::embedded();
        let rx580 = Recursos { usa_placa: true, vram_livre_mb: Some(7800), ram_livre_mb: 12000 };
        assert_eq!(c.get("qwen3-8b").unwrap().encaixe(&rx580, 8192), Encaixe::Placa);
        assert_eq!(c.get("qwen3-14b").unwrap().encaixe(&rx580, 8192), Encaixe::Dividido);
        assert_eq!(c.get("gpt-oss-20b").unwrap().encaixe(&rx580, 8192), Encaixe::Dividido);
    }
}
