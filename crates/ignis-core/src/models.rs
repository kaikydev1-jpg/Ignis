//! Registro dos modelos instalados (`modelos.json`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::budget::ModelShape;
use crate::gguf::ModelInfo;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "kebab-case")]
pub enum Origem {
    /// Baixado do catálogo.
    Catalogo { id: String },
    /// Lido no lugar, direto da pasta do Ollama (nunca apagamos).
    OllamaReferencia { referencia: String },
    /// Copiado do Ollama para a pasta do Ignis.
    OllamaCopia { referencia: String },
    /// Arquivo GGUF indicado pela pessoa (lido no lugar).
    Arquivo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstalledModel {
    pub id: String,
    pub nome: String,
    pub arquivo: PathBuf,
    pub tamanho: u64,
    pub sha256: Option<String>,
    pub origem: Origem,
    pub embeddings: bool,
    pub info: Option<ModelInfo>,
    pub instalado_em: u64,
    #[serde(default)]
    pub licenca: Option<String>,
}

impl InstalledModel {
    pub fn existe(&self) -> bool {
        self.arquivo.is_file()
    }

    pub fn shape(&self) -> ModelShape {
        let i = self.info.clone().unwrap_or_default();
        ModelShape {
            tamanho_arquivo: self.tamanho,
            camadas: i.camadas,
            kv_bytes_por_token: i.kv_bytes_por_token(),
            contexto_max: i.contexto_treino,
            embd: i.embd,
        }
    }

    /// Monta a partir de um arquivo GGUF no disco.
    pub fn from_file(
        id: String,
        nome: Option<String>,
        arquivo: PathBuf,
        origem: Origem,
        sha256: Option<String>,
    ) -> Result<Self, crate::gguf::GgufError> {
        let info = ModelInfo::read(&arquivo)?;
        let tamanho = std::fs::metadata(&arquivo)?.len();
        Ok(Self {
            nome: nome.or_else(|| info.nome.clone()).unwrap_or_else(|| id.clone()),
            id,
            embeddings: info.embeddings,
            licenca: info.licenca.clone(),
            info: Some(info),
            arquivo,
            tamanho,
            sha256,
            origem,
            instalado_em: crate::util::now_unix(),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoveOutcome {
    /// Arquivo apagado da pasta do Ignis.
    Apagado,
    /// Só tirado da lista (arquivo é do Ollama ou de outra pasta e foi mantido).
    SoDaLista,
}

#[derive(Debug, Default)]
pub struct Registry {
    path: PathBuf,
    pub modelos: Vec<InstalledModel>,
}

impl Registry {
    pub fn load(path: PathBuf) -> Self {
        let modelos = crate::paths::read_json(&path).unwrap_or_default();
        Self { path, modelos }
    }

    pub fn save(&self) -> std::io::Result<()> {
        crate::paths::write_json(&self.path, &self.modelos)
    }

    pub fn get(&self, id: &str) -> Option<&InstalledModel> {
        self.modelos.iter().find(|m| m.id == id)
    }

    pub fn add(&mut self, m: InstalledModel) -> std::io::Result<()> {
        self.modelos.retain(|x| x.id != m.id);
        self.modelos.push(m);
        self.modelos.sort_by_key(|a| a.nome.to_lowercase());
        self.save()
    }

    /// Remove da lista e, se o arquivo estiver dentro da pasta de modelos do
    /// Ignis, apaga do disco. Arquivos de fora (Ollama, outras pastas) nunca são apagados.
    pub fn remove(&mut self, id: &str, models_dir: &Path) -> std::io::Result<Option<RemoveOutcome>> {
        let Some(m) = self.get(id).cloned() else { return Ok(None) };
        let outcome = if is_inside(&m.arquivo, models_dir) && !matches!(m.origem, Origem::OllamaReferencia { .. } | Origem::Arquivo) {
            if m.arquivo.exists() {
                std::fs::remove_file(&m.arquivo)?;
            }
            crate::download::remove_partial(&m.arquivo);
            RemoveOutcome::Apagado
        } else {
            RemoveOutcome::SoDaLista
        };
        self.modelos.retain(|x| x.id != id);
        self.save()?;
        Ok(Some(outcome))
    }

    pub fn ids(&self) -> Vec<String> {
        self.modelos.iter().map(|m| m.id.clone()).collect()
    }
}

/// `path` está dentro de `dir`? (resolve links e "..")
pub fn is_inside(path: &Path, dir: &Path) -> bool {
    let (Ok(p), Ok(d)) = (
        std::fs::canonicalize(path).or_else(|_| {
            // arquivo pode não existir mais: usa a pasta-mãe
            path.parent().map(std::fs::canonicalize).unwrap_or(Ok(PathBuf::new())).map(|pp| pp.join(path.file_name().unwrap_or_default()))
        }),
        std::fs::canonicalize(dir),
    ) else {
        return false;
    };
    p.starts_with(&d) && p != d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gguf::{write_test_gguf, Value};

    fn gguf(p: &Path) {
        write_test_gguf(
            p,
            &[
                ("general.architecture", Value::Str("qwen2".into())),
                ("qwen2.block_count", Value::Int(24)),
                ("qwen2.embedding_length", Value::Int(896)),
                ("qwen2.attention.head_count", Value::Int(14)),
            ],
        )
        .unwrap();
    }

    #[test]
    fn apaga_so_o_que_e_do_ignis() {
        let tmp = tempfile::tempdir().unwrap();
        let models = tmp.path().join("modelos");
        let fora = tmp.path().join("ollama");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::create_dir_all(&fora).unwrap();
        let a = models.join("a.gguf");
        let b = fora.join("sha256-abc");
        gguf(&a);
        gguf(&b);
        let mut r = Registry::load(tmp.path().join("modelos.json"));
        r.add(InstalledModel::from_file("a".into(), None, a.clone(), Origem::Catalogo { id: "a".into() }, None).unwrap()).unwrap();
        r.add(
            InstalledModel::from_file("b".into(), None, b.clone(), Origem::OllamaReferencia { referencia: "x:1b".into() }, None).unwrap(),
        )
        .unwrap();
        assert_eq!(r.remove("b", &models).unwrap(), Some(RemoveOutcome::SoDaLista));
        assert!(b.exists(), "arquivo do Ollama continua lá");
        assert_eq!(r.remove("a", &models).unwrap(), Some(RemoveOutcome::Apagado));
        assert!(!a.exists());
        assert_eq!(r.remove("zzz", &models).unwrap(), None);
        // recarrega do disco
        assert!(Registry::load(tmp.path().join("modelos.json")).modelos.is_empty());
    }

    #[test]
    fn caminho_com_ponto_ponto_nao_engana() {
        let tmp = tempfile::tempdir().unwrap();
        let models = tmp.path().join("modelos");
        std::fs::create_dir_all(&models).unwrap();
        let fora = tmp.path().join("x.gguf");
        std::fs::write(&fora, b"x").unwrap();
        assert!(!is_inside(&models.join("../x.gguf"), &models));
        assert!(!is_inside(&models, &models));
    }
}
