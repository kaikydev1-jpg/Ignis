//! Importação única dos modelos do Ollama.
//!
//! Lê os manifestos em `<pasta>/manifests/<host>/<espaço>/<modelo>/<tag>` e
//! acha o "blob" do modelo em `<pasta>/blobs/sha256-<hash>`. Só oferece os que
//! são GGUF. **Nunca apaga nem altera nada** dentro da pasta do Ollama.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MEDIA_MODEL: &str = "application/vnd.ollama.image.model";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OllamaModel {
    /// Como o Ollama chama ("llama3.2:3b", "usuario/modelo:tag").
    pub referencia: String,
    pub caminho: PathBuf,
    pub tamanho: u64,
    /// sha256 do blob (sem o prefixo "sha256:").
    pub sha256: String,
    pub pasta_ollama: PathBuf,
}

#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    layers: Vec<Layer>,
}

#[derive(Deserialize)]
struct Layer {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
    #[serde(default)]
    size: u64,
}

/// Pastas onde o Ollama costuma guardar modelos.
pub fn default_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("OLLAMA_MODELS") {
        v.push(PathBuf::from(p));
    }
    if let Some(home) = dirs::home_dir() {
        v.push(home.join(".ollama").join("models"));
    }
    if cfg!(target_os = "linux") {
        v.push(PathBuf::from("/usr/share/ollama/.ollama/models"));
        v.push(PathBuf::from("/var/lib/ollama/.ollama/models"));
        v.push(PathBuf::from("/var/lib/ollama/models"));
    }
    let mut seen = Vec::new();
    v.retain(|p| {
        let k = p.clone();
        if seen.contains(&k) {
            false
        } else {
            seen.push(k);
            true
        }
    });
    v
}

fn referencia(parts: &[String]) -> Option<String> {
    // host / espaço / modelo / tag
    let [host, ns, model, tag] = parts else { return None };
    Some(match (host.as_str(), ns.as_str()) {
        ("registry.ollama.ai", "library") => format!("{model}:{tag}"),
        ("registry.ollama.ai", _) => format!("{ns}/{model}:{tag}"),
        _ => format!("{host}/{ns}/{model}:{tag}"),
    })
}

fn walk(dir: &Path, prefix: &mut Vec<String>, out: &mut Vec<(Vec<String>, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let p = e.path();
        prefix.push(name);
        if p.is_dir() {
            if prefix.len() < 4 {
                walk(&p, prefix, out);
            }
        } else if prefix.len() == 4 {
            out.push((prefix.clone(), p));
        }
        prefix.pop();
    }
}

fn is_gguf_file(p: &Path) -> bool {
    crate::gguf::is_gguf(p)
}

/// Lista os modelos GGUF de uma pasta do Ollama.
pub fn scan(models_dir: &Path) -> Vec<OllamaModel> {
    let mut found = Vec::new();
    walk(&models_dir.join("manifests"), &mut Vec::new(), &mut found);
    let mut out = Vec::new();
    for (parts, manifest_path) in found {
        let Some(r) = referencia(&parts) else { continue };
        let Ok(bytes) = std::fs::read(&manifest_path) else { continue };
        let Ok(m) = serde_json::from_slice::<Manifest>(&bytes) else { continue };
        let Some(layer) = m.layers.iter().find(|l| l.media_type == MEDIA_MODEL) else { continue };
        let Some(hex) = layer.digest.strip_prefix("sha256:") else { continue };
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let blobs = models_dir.join("blobs");
        let caminho = [blobs.join(format!("sha256-{hex}")), blobs.join(format!("sha256:{hex}"))]
            .into_iter()
            .find(|p| p.is_file());
        let Some(caminho) = caminho else { continue };
        if !is_gguf_file(&caminho) {
            continue;
        }
        let tamanho = std::fs::metadata(&caminho).map(|m| m.len()).unwrap_or(layer.size);
        out.push(OllamaModel { referencia: r, caminho, tamanho, sha256: hex.to_lowercase(), pasta_ollama: models_dir.to_path_buf() });
    }
    out.sort_by(|a, b| a.referencia.cmp(&b.referencia));
    out
}

/// Procura em todas as pastas conhecidas.
pub fn scan_all(extra: &[PathBuf]) -> Vec<OllamaModel> {
    let mut dirs = default_dirs();
    dirs.extend(extra.iter().cloned());
    let mut out: Vec<OllamaModel> = Vec::new();
    for d in dirs {
        for m in scan(&d) {
            if !out.iter().any(|x| x.sha256 == m.sha256 && x.referencia == m.referencia) {
                out.push(m);
            }
        }
    }
    out
}

/// Id do modelo no Ignis para uma referência do Ollama.
pub fn model_id(referencia: &str) -> String {
    format!("ollama/{referencia}")
}

/// Copia (lendo o original sem alterá-lo) conferindo o SHA-256 no caminho.
/// `cancel` é checado a cada bloco; `progress` recebe bytes copiados.
pub fn copy_verified(
    src: &Path,
    dst: &Path,
    sha256: &str,
    cancel: &dyn Fn() -> bool,
    progress: &dyn Fn(u64),
) -> std::io::Result<bool> {
    let tmp = crate::download::part_path(dst);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut r = std::fs::File::open(src)?;
    let mut w = std::fs::File::create(&tmp)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 4 << 20];
    let mut done = 0u64;
    loop {
        if cancel() {
            drop(w);
            let _ = std::fs::remove_file(&tmp);
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelado"));
        }
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n])?;
        h.update(&buf[..n]);
        done += n as u64;
        progress(done);
    }
    w.sync_all()?;
    drop(w);
    if !hex::encode(h.finalize()).eq_ignore_ascii_case(sha256) {
        let _ = std::fs::remove_file(&tmp);
        return Ok(false);
    }
    std::fs::rename(&tmp, dst)?;
    Ok(true)
}

/// Ferramenta de teste: cria uma pasta do Ollama falsa com um modelo GGUF.
pub fn write_fake_ollama(dir: &Path, model: &str, tag: &str, gguf_bytes: &[u8]) -> std::io::Result<String> {
    let hex = hex::encode(Sha256::digest(gguf_bytes));
    let blobs = dir.join("blobs");
    std::fs::create_dir_all(&blobs)?;
    std::fs::write(blobs.join(format!("sha256-{hex}")), gguf_bytes)?;
    let tmpl = b"{{ .Prompt }}";
    let tmpl_hex = hex::encode(Sha256::digest(tmpl));
    std::fs::write(blobs.join(format!("sha256-{tmpl_hex}")), tmpl)?;
    let mdir = dir.join("manifests/registry.ollama.ai/library").join(model);
    std::fs::create_dir_all(&mdir)?;
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
        "config": {"mediaType": "application/vnd.docker.container.image.v1+json", "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000", "size": 10},
        "layers": [
            {"mediaType": MEDIA_MODEL, "digest": format!("sha256:{hex}"), "size": gguf_bytes.len()},
            {"mediaType": "application/vnd.ollama.image.template", "digest": format!("sha256:{tmpl_hex}"), "size": tmpl.len()}
        ]
    });
    std::fs::write(mdir.join(tag), serde_json::to_vec(&manifest)?)?;
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn referencias() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(referencia(&s(&["registry.ollama.ai", "library", "llama3.2", "3b"])).unwrap(), "llama3.2:3b");
        assert_eq!(referencia(&s(&["registry.ollama.ai", "fulano", "m", "latest"])).unwrap(), "fulano/m:latest");
        assert_eq!(referencia(&s(&["hf.co", "Qwen", "Qwen3-GGUF", "Q4_K_M"])).unwrap(), "hf.co/Qwen/Qwen3-GGUF:Q4_K_M");
    }

    #[test]
    fn acha_so_gguf() {
        let tmp = tempfile::tempdir().unwrap();
        let mut gguf = b"GGUF".to_vec();
        gguf.extend_from_slice(&[3, 0, 0, 0]);
        gguf.extend_from_slice(&[0u8; 100]);
        let hex = write_fake_ollama(tmp.path(), "qwen3", "0.6b", &gguf).unwrap();
        // um "modelo" que não é GGUF
        write_fake_ollama(tmp.path(), "outro", "latest", b"NAOGGUF-xxxxxxxx").unwrap();
        let found = scan(tmp.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].referencia, "qwen3:0.6b");
        assert_eq!(found[0].sha256, hex);
        assert_eq!(found[0].tamanho, gguf.len() as u64);
    }

    #[test]
    fn copia_conferindo_hash_sem_mexer_no_original() {
        let tmp = tempfile::tempdir().unwrap();
        let data = b"GGUF\x03\x00\x00\x00conteudo";
        let hex = write_fake_ollama(tmp.path(), "m", "1b", data).unwrap();
        let src = scan(tmp.path()).remove(0).caminho;
        let dst = tmp.path().join("ignis/m.gguf");
        assert!(copy_verified(&src, &dst, &hex, &|| false, &|_| {}).unwrap());
        assert_eq!(std::fs::read(&dst).unwrap(), data);
        assert!(src.exists());
        // hash errado não deixa arquivo
        let dst2 = tmp.path().join("ignis/m2.gguf");
        assert!(!copy_verified(&src, &dst2, &"0".repeat(64), &|| false, &|_| {}).unwrap());
        assert!(!dst2.exists());
    }

    #[test]
    fn pasta_inexistente() {
        assert!(scan(Path::new("/nao/existe/mesmo")).is_empty());
    }
}
