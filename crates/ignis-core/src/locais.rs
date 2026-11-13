//! Modelos que outros programas já baixaram neste computador.
//!
//! Antes de baixar um modelo, o Ignis procura o mesmo arquivo aqui:
//! - **Ollama** guarda cada arquivo como `blobs/sha256-<hash>`;
//! - **Hugging Face** (cache do `huggingface_hub`) guarda como `models--dono--nome/blobs/<hash>`;
//! - **LM Studio** guarda com o nome original, em pastas por repositório.
//!
//! Nos dois primeiros, achar pelo SHA-256 é imediato (o nome do arquivo é o
//! hash). O Ignis nunca apaga nada dessas pastas.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fonte {
    Ollama,
    HuggingFace,
    LmStudio,
}

impl Fonte {
    pub fn nome(self) -> &'static str {
        match self {
            Fonte::Ollama => "Ollama",
            Fonte::HuggingFace => "Hugging Face",
            Fonte::LmStudio => "LM Studio",
        }
    }
}

/// Um arquivo GGUF já no computador.
#[derive(Debug, Clone, Serialize)]
pub struct ModeloLocal {
    pub fonte: Fonte,
    pub caminho: PathBuf,
    pub tamanho: u64,
    /// Nome para mostrar (ex.: "unsloth/Qwen3-0.6B-GGUF · Qwen3-0.6B-Q8_0.gguf").
    pub nome: String,
}

/// Onde ficam os modelos baixados pelo Hugging Face (`huggingface_hub`).
pub fn pasta_hugging_face() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("HF_HUB_CACHE").or_else(|| std::env::var_os("HUGGINGFACE_HUB_CACHE")) {
        return Some(PathBuf::from(p));
    }
    if let Some(p) = std::env::var_os("HF_HOME") {
        return Some(PathBuf::from(p).join("hub"));
    }
    dirs::home_dir().map(|h| h.join(".cache").join("huggingface").join("hub"))
}

/// Pastas do LM Studio (a nova e a antiga).
pub fn pastas_lm_studio() -> Vec<PathBuf> {
    let Some(h) = dirs::home_dir() else { return Vec::new() };
    vec![h.join(".lmstudio").join("models"), h.join(".cache").join("lm-studio").join("models")]
}

fn hex_ok(sha: &str) -> bool {
    sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit())
}

/// Procura um arquivo com este SHA-256 já baixado pelo Ollama ou pelo Hugging Face.
/// Não lê nenhum arquivo: nessas pastas o nome do arquivo é o próprio hash.
pub fn achar_por_sha256(sha: &str, pastas_ollama: &[PathBuf], pasta_hf: Option<&Path>) -> Option<ModeloLocal> {
    let sha = sha.to_lowercase();
    if !hex_ok(&sha) {
        return None;
    }
    for d in pastas_ollama {
        for nome in [format!("sha256-{sha}"), format!("sha256:{sha}")] {
            let p = d.join("blobs").join(nome);
            if let Ok(md) = std::fs::metadata(&p) {
                if md.is_file() {
                    return Some(ModeloLocal { fonte: Fonte::Ollama, tamanho: md.len(), nome: "arquivo do Ollama".into(), caminho: p });
                }
            }
        }
    }
    let hf = pasta_hf?;
    for repo in std::fs::read_dir(hf).ok()?.flatten() {
        let nome_repo = repo.file_name().to_string_lossy().into_owned();
        if !nome_repo.starts_with("models--") {
            continue;
        }
        let p = repo.path().join("blobs").join(&sha);
        if let Ok(md) = std::fs::metadata(&p) {
            if md.is_file() {
                return Some(ModeloLocal { fonte: Fonte::HuggingFace, tamanho: md.len(), nome: repo_legivel(&nome_repo), caminho: p });
            }
        }
    }
    None
}

/// "models--unsloth--Qwen3-0.6B-GGUF" -> "unsloth/Qwen3-0.6B-GGUF"
fn repo_legivel(pasta: &str) -> String {
    pasta.trim_start_matches("models--").replacen("--", "/", 1)
}

fn eh_gguf_de_modelo(p: &Path) -> bool {
    let nome = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    // arquivos de visão/áudio (mmproj) sozinhos não são modelos
    nome.ends_with(".gguf") && !nome.contains("mmproj") && crate::gguf::is_gguf(p)
}

/// GGUFs no cache do Hugging Face (pela pasta `snapshots`, que tem os nomes originais).
pub fn ggufs_hugging_face(pasta_hf: &Path) -> Vec<ModeloLocal> {
    let mut out = Vec::new();
    let Ok(repos) = std::fs::read_dir(pasta_hf) else { return out };
    for repo in repos.flatten() {
        let nome_repo = repo.file_name().to_string_lossy().into_owned();
        if !nome_repo.starts_with("models--") {
            continue;
        }
        let Ok(revs) = std::fs::read_dir(repo.path().join("snapshots")) else { continue };
        for rev in revs.flatten() {
            let mut arquivos = Vec::new();
            andar(&rev.path(), 3, &mut arquivos);
            for a in arquivos {
                // o snapshot aponta para o blob; usa o caminho real
                let real = std::fs::canonicalize(&a).unwrap_or(a.clone());
                let Ok(md) = std::fs::metadata(&real) else { continue };
                if out.iter().any(|m: &ModeloLocal| m.caminho == real) {
                    continue;
                }
                let arquivo = a.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(ModeloLocal {
                    fonte: Fonte::HuggingFace,
                    tamanho: md.len(),
                    nome: format!("{} · {arquivo}", repo_legivel(&nome_repo)),
                    caminho: real,
                });
            }
        }
    }
    out
}

/// GGUFs nas pastas do LM Studio.
pub fn ggufs_lm_studio(pastas: &[PathBuf]) -> Vec<ModeloLocal> {
    let mut out = Vec::new();
    for base in pastas {
        let mut arquivos = Vec::new();
        andar(base, 4, &mut arquivos);
        for a in arquivos {
            let Ok(md) = std::fs::metadata(&a) else { continue };
            let rel = a.strip_prefix(base).unwrap_or(&a).to_string_lossy().replace('\\', "/");
            out.push(ModeloLocal { fonte: Fonte::LmStudio, tamanho: md.len(), nome: rel, caminho: a });
        }
    }
    out
}

fn andar(dir: &Path, profundidade: u32, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if profundidade > 0 {
                andar(&p, profundidade - 1, out);
            }
        } else if eh_gguf_de_modelo(&p) {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gguf::{write_test_gguf, Value as G};

    fn gguf(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        write_test_gguf(p, &[("general.architecture", G::Str("qwen3".into()))]).unwrap();
    }

    #[test]
    fn acha_pelo_hash_no_ollama_e_no_hugging_face_sem_ler_arquivos() {
        let d = tempfile::tempdir().unwrap();
        let sha_o = "a".repeat(64);
        let sha_h = "b".repeat(64);
        let ollama = d.path().join("ollama");
        gguf(&ollama.join("blobs").join(format!("sha256-{sha_o}")));
        let hf = d.path().join("hf");
        gguf(&hf.join("models--unsloth--Qwen3-0.6B-GGUF").join("blobs").join(&sha_h));
        let o = achar_por_sha256(&sha_o.to_uppercase(), std::slice::from_ref(&ollama), Some(&hf)).unwrap();
        assert_eq!(o.fonte, Fonte::Ollama);
        let h = achar_por_sha256(&sha_h, std::slice::from_ref(&ollama), Some(&hf)).unwrap();
        assert_eq!((h.fonte, h.nome.as_str()), (Fonte::HuggingFace, "unsloth/Qwen3-0.6B-GGUF"));
        assert!(achar_por_sha256(&"c".repeat(64), &[ollama], Some(&hf)).is_none());
        assert!(achar_por_sha256("nao-e-hash", &[], None).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn lista_ggufs_do_cache_do_hugging_face_e_do_lm_studio() {
        let d = tempfile::tempdir().unwrap();
        let hf = d.path().join("hf");
        let repo = hf.join("models--unsloth--Qwen3-0.6B-GGUF");
        let blob = repo.join("blobs").join("d".repeat(64));
        gguf(&blob);
        let snap = repo.join("snapshots").join("abc123");
        std::fs::create_dir_all(&snap).unwrap();
        std::os::unix::fs::symlink(&blob, snap.join("Qwen3-0.6B-Q8_0.gguf")).unwrap();
        // mmproj sozinho não conta
        gguf(&snap.join("mmproj-F16.gguf"));
        let l = ggufs_hugging_face(&hf);
        assert_eq!(l.len(), 1, "{l:?}");
        assert_eq!(l[0].nome, "unsloth/Qwen3-0.6B-GGUF · Qwen3-0.6B-Q8_0.gguf");
        assert_eq!(l[0].caminho, std::fs::canonicalize(&blob).unwrap());

        let lm = d.path().join("lmstudio");
        gguf(&lm.join("lmstudio-community").join("Qwen3-8B-GGUF").join("Qwen3-8B-Q4_K_M.gguf"));
        let l = ggufs_lm_studio(&[lm]);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].nome, "lmstudio-community/Qwen3-8B-GGUF/Qwen3-8B-Q4_K_M.gguf");
    }
}
