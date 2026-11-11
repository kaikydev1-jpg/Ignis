//! Download com retomada (HTTP Range), cancelamento e conferência de SHA-256.
//!
//! O arquivo vai sendo gravado em `<destino>.part`. Cancelar mantém o `.part`
//! para retomar depois; se o SHA-256 não conferir, o `.part` é apagado.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct Progress {
    pub baixado: u64,
    pub total: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("download cancelado")]
    Cancelled,
    #[error("não consegui conectar ({0}). Confira a internet e tente de novo.")]
    Network(String),
    #[error("o servidor respondeu com erro {0}. Tente de novo mais tarde.")]
    Http(u16),
    #[error("o arquivo baixado não confere com o esperado (SHA-256 diferente). Ele foi apagado; tente baixar de novo.")]
    HashMismatch { esperado: String, obtido: String },
    #[error("o arquivo baixado tem tamanho diferente do esperado ({obtido} em vez de {esperado} bytes). Tente de novo.")]
    SizeMismatch { esperado: u64, obtido: u64 },
    #[error("acabou o espaço em disco. Libere espaço e clique em Retomar.")]
    NoSpace,
    #[error("erro ao gravar no disco: {0}")]
    Io(std::io::Error),
}

impl From<std::io::Error> for DownloadError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::StorageFull || matches!(e.raw_os_error(), Some(28) | Some(112)) {
            DownloadError::NoSpace
        } else {
            DownloadError::Io(e)
        }
    }
}

pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Quanto já foi baixado (tamanho do `.part`), para mostrar "Retomar".
pub fn partial_size(dest: &Path) -> Option<u64> {
    std::fs::metadata(part_path(dest)).ok().map(|m| m.len()).filter(|&n| n > 0)
}

pub fn remove_partial(dest: &Path) {
    let _ = std::fs::remove_file(part_path(dest));
}

/// Cliente HTTP padrão do Ignis (sem telemetria; User-Agent genérico).
///
/// Usa os certificados do sistema; se eles não puderem ser lidos (ex.: Flatpak
/// com variável SSL_CERT_FILE apontando para fora do sandbox), usa a lista da
/// Mozilla embutida. Nunca trava o aplicativo.
pub fn http_client() -> reqwest::Client {
    crate::install_crypto_provider();
    let base = || {
        reqwest::Client::builder()
            .user_agent(crate::consts::USER_AGENT)
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(90))
    };
    match base().build() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("certificados do sistema indisponíveis ({e}); usando a lista embutida da Mozilla");
            base().tls_certs_only(embedded_roots()).build().unwrap_or_else(|e| {
                tracing::error!("cliente HTTP sem TLS: {e}");
                plain_client()
            })
        }
    }
}

/// Autoridades certificadoras embutidas (Mozilla, via webpki-root-certs).
pub fn embedded_roots() -> Vec<reqwest::Certificate> {
    webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().filter_map(|c| reqwest::Certificate::from_der(c.as_ref()).ok()).collect()
}

/// Cliente só para endereços locais (127.0.0.1): não carrega certificados.
pub fn plain_client() -> reqwest::Client {
    crate::install_crypto_provider();
    reqwest::Client::builder().no_proxy().tls_certs_only(Vec::new()).build().unwrap_or_default()
}

/// SHA-256 de um arquivo inteiro (bloqueante).
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

fn hash_prefix(path: &Path, h: &mut Sha256) -> std::io::Result<()> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        h.update(&buf[..n]);
    }
}

/// Baixa `url` para `dest`, retomando de `dest.part` se existir.
#[allow(clippy::too_many_arguments)]
pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    expected_size: Option<u64>,
    cancel: &CancellationToken,
    on_progress: &(dyn Fn(Progress) + Send + Sync),
    hf_token: Option<&str>,
) -> Result<(), DownloadError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = part_path(dest);
    let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if let Some(size) = expected_size {
        if have > size {
            std::fs::remove_file(&part)?;
            have = 0;
        }
    }

    let mut hasher = Sha256::new();
    let mut total = expected_size;
    let complete_already = expected_size.is_some() && Some(have) == expected_size;

    if !complete_already {
        if have > 0 {
            let p = part.clone();
            let mut h2 = Sha256::new();
            hasher = tokio::task::spawn_blocking(move || hash_prefix(&p, &mut h2).map(|_| h2))
                .await
                .map_err(|e| DownloadError::Io(std::io::Error::other(e)))??;
        }
        let mut req = client.get(url);
        // o token da pessoa só vai para o Hugging Face (redirecionamentos para
        // outros servidores não levam o cabeçalho de autorização)
        if let Some(t) = hf_token.filter(|t| !t.is_empty()) {
            if url.starts_with(&crate::hub::base_url()) {
                req = req.bearer_auth(t);
            }
        }
        if have > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(DownloadError::Cancelled),
            r = req.send() => r.map_err(|e| DownloadError::Network(short_err(&e)))?,
        };
        let status = resp.status().as_u16();
        let mut file = match status {
            206 => std::fs::OpenOptions::new().append(true).open(&part)?,
            200 => {
                // servidor ignorou o Range: recomeça do zero
                have = 0;
                hasher = Sha256::new();
                std::fs::File::create(&part)?
            }
            416 if have > 0 => {
                // já está tudo aqui; só conferir
                std::fs::OpenOptions::new().append(true).open(&part)?
            }
            s => return Err(DownloadError::Http(s)),
        };
        if status != 416 {
            if total.is_none() {
                total = resp.content_length().map(|l| l + have);
            }
            let mut stream = resp.bytes_stream();
            let mut last = Instant::now();
            on_progress(Progress { baixado: have, total });
            loop {
                let chunk = tokio::select! {
                    _ = cancel.cancelled() => {
                        file.flush()?;
                        return Err(DownloadError::Cancelled);
                    }
                    c = stream.next() => c,
                };
                let Some(chunk) = chunk else { break };
                let chunk = chunk.map_err(|e| DownloadError::Network(short_err(&e)))?;
                file.write_all(&chunk)?;
                hasher.update(&chunk);
                have += chunk.len() as u64;
                if last.elapsed() > Duration::from_millis(250) {
                    last = Instant::now();
                    on_progress(Progress { baixado: have, total });
                }
            }
            file.flush()?;
            file.sync_all()?;
        }
    } else {
        let p = part.clone();
        let mut h2 = Sha256::new();
        hasher = tokio::task::spawn_blocking(move || hash_prefix(&p, &mut h2).map(|_| h2))
            .await
            .map_err(|e| DownloadError::Io(std::io::Error::other(e)))??;
    }

    let size = std::fs::metadata(&part)?.len();
    on_progress(Progress { baixado: size, total: total.or(Some(size)) });
    if let Some(esperado) = expected_size {
        if size != esperado {
            if size > esperado {
                let _ = std::fs::remove_file(&part);
            }
            return Err(DownloadError::SizeMismatch { esperado, obtido: size });
        }
    }
    let obtido = hex::encode(hasher.finalize());
    if let Some(esperado) = expected_sha256 {
        if !obtido.eq_ignore_ascii_case(esperado) {
            let _ = std::fs::remove_file(&part);
            return Err(DownloadError::HashMismatch { esperado: esperado.to_string(), obtido });
        }
    }
    std::fs::rename(&part, dest)?;
    Ok(())
}

fn short_err(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "tempo esgotado".into()
    } else if e.is_connect() {
        "sem conexão".into()
    } else {
        let s = e.to_string();
        s.chars().take(160).collect()
    }
}
