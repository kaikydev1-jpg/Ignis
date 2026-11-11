//! Downloads (cancelar, retomar, conferir hash, remover) com um servidor HTTP
//! local que imita o Hugging Face, e importação do Ollama.

mod comum;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use comum::*;
use ignis_core::catalog::{Arquitetura, Catalog, CatalogModel};
use serde_json::json;
use sha2::{Digest, Sha256};

struct Servidor {
    dados: Vec<u8>,
    pediu_range: AtomicBool,
    servido: AtomicU64,
}

async fn arquivo(State(s): State<Arc<Servidor>>, h: HeaderMap) -> Response {
    let inicio: usize = h
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.trim_end_matches('-').parse().ok())
        .unwrap_or(0);
    if inicio > 0 {
        s.pediu_range.store(true, Ordering::SeqCst);
    }
    let total = s.dados.len();
    let parte = s.dados[inicio.min(total)..].to_vec();
    let s2 = s.clone();
    // envia devagar (pedaços de 64 KB a cada 15 ms) para dar tempo de cancelar
    let stream = futures_util::stream::unfold(0usize, move |pos| {
        let parte = parte.clone();
        let s2 = s2.clone();
        async move {
            if pos >= parte.len() {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
            let fim = (pos + 65536).min(parte.len());
            s2.servido.fetch_add((fim - pos) as u64, Ordering::SeqCst);
            Some((Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(&parte[pos..fim])), fim))
        }
    });
    let mut r = Response::builder().header(header::CONTENT_LENGTH, total - inicio.min(total)).header(header::ACCEPT_RANGES, "bytes");
    r = if inicio > 0 {
        r.status(StatusCode::PARTIAL_CONTENT).header(header::CONTENT_RANGE, format!("bytes {inicio}-{}/{total}", total - 1))
    } else {
        r.status(StatusCode::OK)
    };
    r.body(Body::from_stream(stream)).unwrap().into_response()
}

async fn subir_servidor(dados: Vec<u8>) -> (String, Arc<Servidor>) {
    let s = Arc::new(Servidor { dados, pediu_range: AtomicBool::new(false), servido: AtomicU64::new(0) });
    let app = axum::Router::new().route("/modelo.gguf", get(arquivo)).with_state(s.clone());
    let l = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (format!("http://{addr}/modelo.gguf"), s)
}

/// GGUF válido com ~3 MB (metadados + enchimento).
fn gguf_grande(dir: &std::path::Path) -> Vec<u8> {
    let p = modelo_gguf(dir, "baixavel", false);
    let mut b = std::fs::read(p).unwrap();
    b.resize(3 * 1024 * 1024, 7);
    b
}

fn catalogo(url: &str, dados: &[u8], sha: Option<String>) -> Catalog {
    Catalog {
        versao: 1,
        modelos: vec![CatalogModel {
            id: "teste-1b".into(),
            nome: "Modelo de Teste 1B".into(),
            tipo: "conversa".into(),
            parametros_bilhoes: 1.0,
            quantizacao: "Q4_K_M".into(),
            repo: "teste/teste".into(),
            revisao: "abc".into(),
            arquivo: "teste-1b.gguf".into(),
            tamanho: dados.len() as u64,
            sha256: sha.unwrap_or_else(|| hex::encode(Sha256::digest(dados))),
            licenca: "MIT".into(),
            licenca_url: "https://exemplo.invalid".into(),
            uso: "Teste".into(),
            arquitetura: Arquitetura { camadas: 28, cabecas_kv: 8, dim_k: 64, dim_v: 64, embd: 1024, contexto_max: 4096 },
            modelo_de_teste: true,
            url: Some(url.into()),
        }],
    }
}

fn estado_tarefa(m: &ignis_core::service::Motor, id: &str) -> Option<String> {
    m.tarefas().into_iter().find(|t| t.id == id).map(|t| t.estado)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn baixar_cancelar_retomar_e_remover() {
    let tmp = tempfile::tempdir().unwrap();
    let dados = gguf_grande(tmp.path());
    let (url, srv) = subir_servidor(dados.clone()).await;
    let amb = ambiente(Opcoes { catalog: Some(catalogo(&url, &dados, None)), ..Default::default() });
    let m = &amb.motor;

    // começa e cancela no meio
    let tid = m.start_model_download("teste-1b").unwrap();
    esperar("baixar um pedaço", Duration::from_secs(10), || srv.servido.load(Ordering::SeqCst) > 500_000).await;
    assert!(m.cancel_task("teste-1b"));
    esperar("pausar", Duration::from_secs(5), || estado_tarefa(m, &tid).as_deref() == Some("pausada")).await;
    let parte = amb.paths.models_dir().join("teste-1b.gguf.part");
    let tam_parte = std::fs::metadata(&parte).unwrap().len();
    assert!(tam_parte > 0 && tam_parte < dados.len() as u64, "parte salva: {tam_parte}");
    assert!(m.installed_models().is_empty());

    // a lista mostra o download parcial
    let (_, lista) = ignis_core::api::call(m.clone(), "GET", "/motor/modelos", None).await;
    assert!(lista["catalogo"][0]["parcial"].as_u64().unwrap() > 0);

    // retoma de onde parou
    m.start_model_download("teste-1b").unwrap();
    esperar("concluir", Duration::from_secs(20), || estado_tarefa(m, &tid).as_deref() == Some("concluida")).await;
    assert!(srv.pediu_range.load(Ordering::SeqCst), "retomou com Range");
    let final_ = amb.paths.models_dir().join("teste-1b.gguf");
    assert_eq!(std::fs::read(&final_).unwrap(), dados, "arquivo íntegro");
    assert!(!parte.exists());
    let instalado = m.installed_models();
    assert_eq!(instalado.len(), 1);
    assert_eq!(instalado[0].id, "teste-1b");
    assert_eq!(instalado[0].licenca.as_deref(), Some("MIT"));

    // baixar de novo o que já está instalado é recusado
    assert!(m.start_model_download("teste-1b").is_err());

    // remove
    let msg = m.remove_model("teste-1b").await.unwrap();
    assert!(msg.contains("apagado"));
    assert!(!final_.exists());
    assert!(m.installed_models().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hash_errado_apaga_e_avisa() {
    let tmp = tempfile::tempdir().unwrap();
    let dados = gguf_grande(tmp.path());
    let (url, _srv) = subir_servidor(dados.clone()).await;
    let amb = ambiente(Opcoes { catalog: Some(catalogo(&url, &dados, Some("0".repeat(64)))), ..Default::default() });
    let m = &amb.motor;
    let tid = m.start_model_download("teste-1b").unwrap();
    esperar("falhar", Duration::from_secs(20), || estado_tarefa(m, &tid).as_deref() == Some("erro")).await;
    let t = m.tarefas().into_iter().find(|t| t.id == tid).unwrap();
    assert!(t.erro.unwrap().contains("SHA-256"));
    assert!(!amb.paths.models_dir().join("teste-1b.gguf").exists());
    assert!(!amb.paths.models_dir().join("teste-1b.gguf.part").exists());
    assert!(m.installed_models().is_empty());
    assert!(avisos_texto(m).contains("falhou"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remover_download_incompleto() {
    let tmp = tempfile::tempdir().unwrap();
    let dados = gguf_grande(tmp.path());
    let (url, srv) = subir_servidor(dados.clone()).await;
    let amb = ambiente(Opcoes { catalog: Some(catalogo(&url, &dados, None)), ..Default::default() });
    let m = &amb.motor;
    let tid = m.start_model_download("teste-1b").unwrap();
    esperar("baixar um pedaço", Duration::from_secs(10), || srv.servido.load(Ordering::SeqCst) > 300_000).await;
    m.cancel_task(&tid);
    esperar("pausar", Duration::from_secs(5), || estado_tarefa(m, &tid).as_deref() == Some("pausada")).await;
    let msg = m.remove_model("teste-1b").await.unwrap();
    assert!(msg.contains("incompleto"));
    assert!(!amb.paths.models_dir().join("teste-1b.gguf.part").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn importar_do_ollama_sem_mexer_nele() {
    let tmp = tempfile::tempdir().unwrap();
    let ollama_dir = tmp.path().join("ollama-models");
    let gguf = std::fs::read(modelo_gguf(tmp.path(), "do-ollama", false)).unwrap();
    let hex = ignis_core::ollama::write_fake_ollama(&ollama_dir, "qwen3", "0.6b", &gguf).unwrap();
    let blob = ollama_dir.join("blobs").join(format!("sha256-{hex}"));
    let antes = std::fs::read_dir(ollama_dir.join("blobs")).unwrap().count();

    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    // "usar daqui": lê no lugar
    let id = m.import_ollama("qwen3:0.6b", "usar", std::slice::from_ref(&ollama_dir)).unwrap();
    assert_eq!(id, "ollama/qwen3:0.6b");
    let im = m.installed_models().into_iter().find(|x| x.id == id).unwrap();
    assert_eq!(im.arquivo, blob, "não copiou");
    // carrega e conversa com ele
    let info = m.ensure_loaded(&id).await.unwrap();
    assert_eq!(info.modelo_id, id);
    // remover só tira da lista
    let msg = m.remove_model(&id).await.unwrap();
    assert!(msg.contains("mantido"));
    assert!(blob.exists(), "nunca apaga nada do Ollama");

    // "copiar"
    let id2 = m.import_ollama("qwen3:0.6b", "copiar", std::slice::from_ref(&ollama_dir)).unwrap();
    esperar("copiar", Duration::from_secs(10), || m.installed_models().iter().any(|x| x.id == id2)).await;
    let copia = m.installed_models().into_iter().find(|x| x.id == id2).unwrap();
    assert!(copia.arquivo.starts_with(amb.paths.models_dir()));
    assert_eq!(std::fs::read(&copia.arquivo).unwrap(), gguf);
    m.remove_model(&id2).await.unwrap();
    assert!(!copia.arquivo.exists(), "a cópia é apagada");
    assert!(blob.exists(), "o original continua");
    assert_eq!(std::fs::read_dir(ollama_dir.join("blobs")).unwrap().count(), antes, "pasta do Ollama intacta");

    // referência que não existe
    assert!(m.import_ollama("nao:existe", "usar", &[ollama_dir]).is_err());
    let _ = json!({});
}
