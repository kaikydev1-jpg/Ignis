//! Qualquer modelo GGUF do Hugging Face: busca, lista de arquivos (agrupando
//! partes e o arquivo de visão/áudio "mmproj"), leitura do cabeçalho por Range
//! (para saber se cabe antes de baixar) e a conta da pessoa (token), usada só
//! para modelos que pedem para aceitar termos.
//!
//! O token fica só no computador da pessoa e só é enviado ao huggingface.co.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::gguf::{GgufError, ModelInfo};

/// Endereço do Hugging Face (troca nos testes por um servidor local).
pub fn base_url() -> String {
    std::env::var("IGNIS_HF_BASE").unwrap_or_else(|_| "https://huggingface.co".into())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubModel {
    pub repo: String,
    pub downloads: u64,
    pub likes: u64,
    /// Precisa aceitar termos no site (e entrar com a conta).
    pub pede_aceite: bool,
    pub licenca: Option<String>,
    /// Entende imagem/áudio (pelas etiquetas do repositório).
    pub multimodal: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubPart {
    pub nome: String,
    pub tamanho: u64,
    pub sha256: Option<String>,
}

/// Um modelo dentro do repositório (uma quantização), que pode ter várias partes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubFile {
    /// Nome de exibição (primeira parte, sem o sufixo -00001-of-0000N).
    pub nome: String,
    pub quantizacao: Option<String>,
    pub tamanho: u64,
    pub partes: Vec<HubPart>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubRepo {
    pub repo: String,
    pub revisao: String,
    pub pede_aceite: bool,
    pub licenca: Option<String>,
    pub arquivos: Vec<HubFile>,
    /// Arquivos de visão/áudio (mmproj) disponíveis.
    pub mmproj: Vec<HubPart>,
}

#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error("Este modelo pede para aceitar os termos de uso no Hugging Face. Abra a página do modelo, aceite os termos com a sua conta e cole o seu token do Hugging Face no Ignis (Motor → Conta do Hugging Face).")]
    PedeAceite { repo: String },
    #[error("O token do Hugging Face foi recusado. Confira se ele está certo e se tem permissão de leitura.")]
    TokenRecusado,
    #[error("Modelo não encontrado no Hugging Face.")]
    NaoEncontrado,
    #[error("Não consegui falar com o Hugging Face ({0}). Confira a internet.")]
    Rede(String),
    #[error("Resposta inesperada do Hugging Face ({0}).")]
    Estranho(String),
}

fn gated(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::String(s) => !s.is_empty() && s != "false",
        _ => false,
    }
}

fn license_from_tags(tags: &[String]) -> Option<String> {
    tags.iter().find_map(|t| t.strip_prefix("license:").map(String::from))
}

/// Nome do repositório válido ("dono/nome")?
pub fn valid_repo(repo: &str) -> bool {
    let mut it = repo.split('/');
    let (Some(a), Some(b), None) = (it.next(), it.next(), it.next()) else { return false };
    let ok = |s: &str| {
        !s.is_empty()
            && s.len() <= 96
            && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            && !s.starts_with('.')
    };
    ok(a) && ok(b)
}

/// Quantização a partir do nome do arquivo (Q4_K_M, Q8_0, IQ4_XS, F16, BF16, MXFP4...).
pub fn quant_from_name(name: &str) -> Option<String> {
    let upper = name.to_uppercase();
    let stem = upper.trim_end_matches(".GGUF");
    let tokens: Vec<&str> = stem.split(['-', '.', '_']).collect();
    // junta pedaços como Q4 + K + M
    for i in 0..tokens.len() {
        let t = tokens[i];
        let is_q = (t.starts_with('Q') || t.starts_with("IQ"))
            && t.trim_start_matches('I').trim_start_matches('Q').chars().next().is_some_and(|c| c.is_ascii_digit());
        if is_q {
            let mut q = t.to_string();
            for next in tokens.iter().skip(i + 1).take(2) {
                if next.len() <= 2 && next.chars().all(|c| c.is_ascii_alphanumeric()) && !next.chars().all(|c| c.is_ascii_digit())
                    || matches!(*next, "0" | "1")
                {
                    q.push('_');
                    q.push_str(next);
                } else {
                    break;
                }
            }
            return Some(q);
        }
        if matches!(t, "F16" | "BF16" | "F32" | "MXFP4" | "FP16") {
            return Some(t.to_string());
        }
    }
    None
}

/// "nome-00001-of-00003.gguf" → Some(("nome", 1, 3))
pub fn split_part(name: &str) -> Option<(String, u32, u32)> {
    let stem = name.strip_suffix(".gguf")?;
    let (base, rest) = stem.rsplit_once("-of-")?;
    let total: u32 = rest.parse().ok()?;
    let (base, idx) = base.rsplit_once('-')?;
    if idx.len() != 5 || rest.len() != 5 {
        return None;
    }
    Some((base.to_string(), idx.parse().ok()?, total))
}

#[derive(Deserialize)]
struct Sibling {
    rfilename: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    lfs: Option<Lfs>,
}

#[derive(Deserialize)]
struct Lfs {
    sha256: Option<String>,
    size: Option<u64>,
}

/// Agrupa os arquivos .gguf de um repositório.
pub fn group_files(siblings: &[(String, u64, Option<String>)]) -> (Vec<HubFile>, Vec<HubPart>) {
    let mut files: Vec<HubFile> = Vec::new();
    let mut mmproj = Vec::new();
    let mut parts: std::collections::BTreeMap<String, Vec<(u32, HubPart)>> = Default::default();
    for (nome, tamanho, sha) in siblings {
        if !nome.to_lowercase().ends_with(".gguf") {
            continue;
        }
        let part = HubPart { nome: nome.clone(), tamanho: *tamanho, sha256: sha.clone() };
        let base_name = nome.rsplit('/').next().unwrap_or(nome).to_lowercase();
        if base_name.contains("mmproj") {
            mmproj.push(part);
            continue;
        }
        match split_part(nome) {
            Some((base, idx, _)) => parts.entry(base).or_default().push((idx, part)),
            None => files.push(HubFile { nome: nome.clone(), quantizacao: quant_from_name(nome), tamanho: *tamanho, partes: vec![part] }),
        }
    }
    for (base, mut ps) in parts {
        ps.sort_by_key(|(i, _)| *i);
        let tamanho = ps.iter().map(|(_, p)| p.tamanho).sum();
        let partes: Vec<HubPart> = ps.into_iter().map(|(_, p)| p).collect();
        files.push(HubFile { nome: format!("{base}.gguf"), quantizacao: quant_from_name(&base), tamanho, partes });
    }
    files.sort_by_key(|f| f.tamanho);
    (files, mmproj)
}

/// Codifica um valor para usar na URL.
pub fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn auth(req: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
    match token {
        Some(t) if !t.is_empty() => req.bearer_auth(t),
        _ => req,
    }
}

fn map_status(r: &reqwest::Response, repo: &str, token: Option<&str>) -> Option<HubError> {
    let code = r.status().as_u16();
    let gated = r.headers().get("x-error-code").and_then(|v| v.to_str().ok()) == Some("GatedRepo");
    match code {
        200..=299 => None,
        401 | 403 if gated => Some(HubError::PedeAceite { repo: repo.to_string() }),
        401 if token.is_some() => Some(HubError::TokenRecusado),
        401 | 403 => Some(HubError::PedeAceite { repo: repo.to_string() }),
        404 => Some(HubError::NaoEncontrado),
        c => Some(HubError::Estranho(format!("erro {c}"))),
    }
}

/// Busca repositórios com modelos GGUF.
pub async fn search(client: &reqwest::Client, token: Option<&str>, query: &str, limit: u32) -> Result<Vec<HubModel>, HubError> {
    let url =
        format!("{}/api/models?search={}&filter=gguf&sort=downloads&direction=-1&limit={}", base_url(), enc(query.trim()), limit.min(50));
    let req = client.get(url);
    let r = auth(req, token).send().await.map_err(|e| HubError::Rede(e.to_string()))?;
    if let Some(e) = map_status(&r, "", token) {
        return Err(e);
    }
    let items: Vec<serde_json::Value> = r.json().await.map_err(|e| HubError::Estranho(e.to_string()))?;
    Ok(items
        .into_iter()
        .filter_map(|m| {
            let repo = m.get("id")?.as_str()?.to_string();
            let tags: Vec<String> = m.get("tags").and_then(|t| serde_json::from_value(t.clone()).ok()).unwrap_or_default();
            Some(HubModel {
                downloads: m.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
                likes: m.get("likes").and_then(|v| v.as_u64()).unwrap_or(0),
                pede_aceite: m.get("gated").map(gated).unwrap_or(false),
                licenca: license_from_tags(&tags),
                multimodal: tags
                    .iter()
                    .any(|t| matches!(t.as_str(), "multimodal" | "image-text-to-text" | "audio-text-to-text" | "any-to-any" | "vision")),
                repo,
            })
        })
        .collect())
}

/// Arquivos GGUF de um repositório.
pub async fn files(client: &reqwest::Client, token: Option<&str>, repo: &str) -> Result<HubRepo, HubError> {
    if !valid_repo(repo) {
        return Err(HubError::NaoEncontrado);
    }
    let url = format!("{}/api/models/{repo}?blobs=true", base_url());
    let r = auth(client.get(url), token).send().await.map_err(|e| HubError::Rede(e.to_string()))?;
    if let Some(e) = map_status(&r, repo, token) {
        return Err(e);
    }
    let v: serde_json::Value = r.json().await.map_err(|e| HubError::Estranho(e.to_string()))?;
    let revisao = v.get("sha").and_then(|s| s.as_str()).unwrap_or("main").to_string();
    let siblings: Vec<Sibling> = v.get("siblings").and_then(|s| serde_json::from_value(s.clone()).ok()).unwrap_or_default();
    let flat: Vec<(String, u64, Option<String>)> = siblings
        .into_iter()
        .map(|s| {
            let size = s.lfs.as_ref().and_then(|l| l.size).or(s.size).unwrap_or(0);
            (s.rfilename, size, s.lfs.and_then(|l| l.sha256))
        })
        .collect();
    let (arquivos, mmproj) = group_files(&flat);
    let tags: Vec<String> = v.get("tags").and_then(|t| serde_json::from_value(t.clone()).ok()).unwrap_or_default();
    let licenca =
        v.get("cardData").and_then(|c| c.get("license")).and_then(|l| l.as_str()).map(String::from).or_else(|| license_from_tags(&tags));
    Ok(HubRepo { repo: repo.into(), revisao, pede_aceite: v.get("gated").map(gated).unwrap_or(false), licenca, arquivos, mmproj })
}

pub fn file_url(repo: &str, revisao: &str, arquivo: &str) -> String {
    format!("{}/{repo}/resolve/{revisao}/{arquivo}", base_url())
}

/// Lê só o começo do arquivo (Range) para saber arquitetura, camadas e contexto.
pub async fn header_info(
    client: &reqwest::Client,
    token: Option<&str>,
    repo: &str,
    revisao: &str,
    arquivo: &str,
) -> Result<ModelInfo, HubError> {
    // vocabulários grandes ocupam alguns MB; tenta 8 MB e depois 32 MB
    for limite in [8u64 << 20, 32u64 << 20] {
        let r = auth(client.get(file_url(repo, revisao, arquivo)), token)
            .header(reqwest::header::RANGE, format!("bytes=0-{}", limite - 1))
            .send()
            .await
            .map_err(|e| HubError::Rede(e.to_string()))?;
        if let Some(e) = map_status(&r, repo, token) {
            return Err(e);
        }
        let bytes = r.bytes().await.map_err(|e| HubError::Rede(e.to_string()))?;
        match crate::gguf::read_metadata(std::io::Cursor::new(bytes)) {
            Ok(meta) => return Ok(ModelInfo::from_metadata(&meta)),
            Err(GgufError::Io(_)) => continue, // cortado no meio: tenta mais
            Err(e) => return Err(HubError::Estranho(e.to_string())),
        }
    }
    Err(HubError::Estranho("cabeçalho grande demais".into()))
}

/// Quem é a pessoa dona do token (para mostrar "conectado como ...").
pub async fn whoami(client: &reqwest::Client, token: &str) -> Result<String, HubError> {
    let r =
        client.get(format!("{}/api/whoami-v2", base_url())).bearer_auth(token).send().await.map_err(|e| HubError::Rede(e.to_string()))?;
    if r.status().as_u16() == 401 {
        return Err(HubError::TokenRecusado);
    }
    if let Some(e) = map_status(&r, "", Some(token)) {
        return Err(e);
    }
    let v: serde_json::Value = r.json().await.map_err(|e| HubError::Estranho(e.to_string()))?;
    Ok(v.get("name").and_then(|n| n.as_str()).unwrap_or("conta do Hugging Face").to_string())
}

// ---------- token guardado no computador ----------

pub fn token_path(config_dir: &Path) -> PathBuf {
    config_dir.join("hf-token")
}

pub fn load_token(config_dir: &Path) -> Option<String> {
    std::fs::read_to_string(token_path(config_dir)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn save_token(config_dir: &Path, token: Option<&str>) -> std::io::Result<()> {
    let p = token_path(config_dir);
    match token.map(str::trim).filter(|t| !t.is_empty()) {
        None => {
            if p.exists() {
                std::fs::remove_file(p)?;
            }
            Ok(())
        }
        Some(t) => {
            std::fs::create_dir_all(config_dir)?;
            crate::paths::write_atomic(&p, t.as_bytes())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantizacoes() {
        assert_eq!(quant_from_name("gemma-3-4b-it-Q4_K_M.gguf").as_deref(), Some("Q4_K_M"));
        assert_eq!(quant_from_name("Qwen3-8B-Q8_0.gguf").as_deref(), Some("Q8_0"));
        assert_eq!(quant_from_name("model.IQ4_XS.gguf").as_deref(), Some("IQ4_XS"));
        assert_eq!(quant_from_name("qwen2.5-coder-7b-instruct-q4_k_m.gguf").as_deref(), Some("Q4_K_M"));
        assert_eq!(quant_from_name("gpt-oss-20b-MXFP4.gguf").as_deref(), Some("MXFP4"));
        assert_eq!(quant_from_name("gemma-3-4b-it-f16.gguf").as_deref(), Some("F16"));
        assert_eq!(quant_from_name("Phi-4-mini-instruct.BF16.gguf").as_deref(), Some("BF16"));
    }

    #[test]
    fn partes_e_mmproj() {
        assert_eq!(split_part("m-q4_k_m-00002-of-00003.gguf"), Some(("m-q4_k_m".into(), 2, 3)));
        assert_eq!(split_part("m-q4_k_m.gguf"), None);
        let flat = vec![
            ("README.md".to_string(), 100, None),
            ("m-Q8_0.gguf".to_string(), 8_000, Some("a".repeat(64))),
            ("big/m-Q4_K_M-00002-of-00002.gguf".to_string(), 3_000, None),
            ("big/m-Q4_K_M-00001-of-00002.gguf".to_string(), 4_000, None),
            ("mmproj-model-f16.gguf".to_string(), 800, None),
        ];
        let (files, mm) = group_files(&flat);
        assert_eq!(mm.len(), 1);
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.nome == "m-Q8_0.gguf"));
        let split = files.iter().find(|f| f.partes.len() == 2).unwrap();
        assert_eq!(split.tamanho, 7_000);
        assert_eq!(split.partes.len(), 2);
        assert!(split.partes[0].nome.ends_with("00001-of-00002.gguf"), "partes em ordem");
        assert_eq!(split.quantizacao.as_deref(), Some("Q4_K_M"));
    }

    #[test]
    fn codifica_busca() {
        assert_eq!(enc("qwen3 coder"), "qwen3%20coder");
        assert_eq!(enc("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn nomes_de_repositorio() {
        assert!(valid_repo("ggml-org/gemma-3-4b-it-GGUF"));
        assert!(!valid_repo("../etc"));
        assert!(!valid_repo("a/b/c"));
        assert!(!valid_repo("a/"));
    }

    #[test]
    fn token_guardado_so_no_computador() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_token(dir.path()), None);
        save_token(dir.path(), Some("  hf_abc123  ")).unwrap();
        assert_eq!(load_token(dir.path()).as_deref(), Some("hf_abc123"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(token_path(dir.path())).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "só a pessoa lê o token");
        }
        save_token(dir.path(), Some("")).unwrap();
        assert_eq!(load_token(dir.path()), None);
    }
}
