//! Qualquer modelo do Hugging Face, com um servidor local imitando o site:
//! busca, arquivos (partes e mmproj), cabeçalho por Range, download com mmproj,
//! modelo que pede aceite (sem e com token) e remoção.
//!
//! Um teste só neste arquivo: ele usa a variável IGNIS_HF_BASE (do processo).

mod comum;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path as AxPath, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use comum::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

struct Hf {
    arquivos: HashMap<String, Vec<u8>>, // "dono/nome/arquivo" -> bytes
    auth_no_publico: AtomicBool,
}

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn sibling(nome: &str, b: &[u8]) -> Value {
    json!({"rfilename": nome, "size": b.len(), "lfs": {"sha256": sha(b), "size": b.len()}})
}

async fn busca(axum::extract::RawQuery(q): axum::extract::RawQuery) -> Response {
    let q = q.unwrap_or_default();
    assert!(q.contains("filter=gguf"), "busca só GGUF: {q}");
    Json(json!([
        {"id": "teste/modelo-GGUF", "downloads": 1000, "likes": 5, "gated": false, "tags": ["gguf", "license:apache-2.0", "image-text-to-text"]},
        {"id": "empresa/restrito-GGUF", "downloads": 10, "likes": 1, "gated": "manual", "tags": ["gguf"]}
    ]))
    .into_response()
}

async fn repo(State(s): State<Arc<Hf>>, AxPath((dono, nome)): AxPath<(String, String)>) -> Response {
    let prefixo = format!("{dono}/{nome}/");
    let siblings: Vec<Value> = s
        .arquivos
        .iter()
        .filter_map(|(k, v)| k.strip_prefix(&prefixo).map(|n| sibling(n, v)))
        .chain(std::iter::once(json!({"rfilename": "README.md", "size": 10})))
        .collect();
    if siblings.len() == 1 {
        return StatusCode::NOT_FOUND.into_response();
    }
    let gated: Value = if dono == "empresa" { json!("manual") } else { json!(false) };
    Json(json!({"id": format!("{dono}/{nome}"), "sha": "rev123", "gated": gated, "cardData": {"license": "apache-2.0"}, "siblings": siblings}))
        .into_response()
}

async fn resolve(
    State(s): State<Arc<Hf>>,
    AxPath((dono, nome, rev, arquivo)): AxPath<(String, String, String, String)>,
    h: HeaderMap,
) -> Response {
    assert_eq!(rev, "rev123", "baixa a revisão fixada");
    let auth = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).map(String::from);
    if dono == "empresa" && auth.as_deref() != Some("Bearer hf_bom") {
        return (StatusCode::UNAUTHORIZED, [("x-error-code", "GatedRepo")], "Access restricted").into_response();
    }
    if dono != "empresa" && auth.is_some() {
        s.auth_no_publico.store(true, Ordering::SeqCst);
    }
    let Some(b) = s.arquivos.get(&format!("{dono}/{nome}/{arquivo}")) else { return StatusCode::NOT_FOUND.into_response() };
    let range = h.get(header::RANGE).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("bytes=")).map(String::from);
    match range {
        Some(r) => {
            let (a, z) = r.split_once('-').unwrap();
            let a: usize = a.parse().unwrap();
            let z: usize = z.parse::<usize>().map(|z| z.min(b.len() - 1)).unwrap_or(b.len() - 1);
            if a >= b.len() {
                return StatusCode::RANGE_NOT_SATISFIABLE.into_response();
            }
            (StatusCode::PARTIAL_CONTENT, [(header::CONTENT_RANGE, format!("bytes {a}-{z}/{}", b.len()))], b[a..=z].to_vec())
                .into_response()
        }
        None => b.clone().into_response(),
    }
}

async fn whoami(h: HeaderMap) -> Response {
    match h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        Some("Bearer hf_bom") => Json(json!({"name": "pessoa-teste", "type": "user"})).into_response(),
        _ => (StatusCode::UNAUTHORIZED, "invalid token").into_response(),
    }
}

fn estado(m: &ignis_core::service::Motor, id: &str) -> Option<(String, Option<String>)> {
    m.tarefas().into_iter().find(|t| t.id == id).map(|t| (t.estado, t.erro))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn qualquer_modelo_do_hugging_face() {
    let tmp = tempfile::tempdir().unwrap();
    let gguf = |nome: &str, tam: usize| {
        let mut b = std::fs::read(modelo_gguf(tmp.path(), nome, false)).unwrap();
        b.resize(tam, 3);
        b
    };
    let mut arquivos = HashMap::new();
    arquivos.insert("teste/modelo-GGUF/modelo-Q4_K_M.gguf".to_string(), gguf("q4", 200_000));
    arquivos.insert("teste/modelo-GGUF/grande/modelo-Q8_0-00001-of-00002.gguf".to_string(), gguf("q8", 150_000));
    arquivos.insert("teste/modelo-GGUF/grande/modelo-Q8_0-00002-of-00002.gguf".to_string(), vec![9u8; 120_000]);
    arquivos.insert("teste/modelo-GGUF/mmproj-modelo-f16.gguf".to_string(), gguf("mm", 50_000));
    arquivos.insert("empresa/restrito-GGUF/restrito-Q4_0.gguf".to_string(), gguf("r", 80_000));
    let hf = Arc::new(Hf { arquivos, auth_no_publico: AtomicBool::new(false) });
    let app = axum::Router::new()
        .route("/api/models", get(busca))
        .route("/api/models/{dono}/{nome}", get(repo))
        .route("/api/whoami-v2", get(whoami))
        .route("/{dono}/{nome}/resolve/{rev}/{*arquivo}", get(resolve))
        .with_state(hf.clone());
    let l = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    std::env::set_var("IGNIS_HF_BASE", format!("http://{addr}"));

    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;

    // busca
    let r = m.hub_search("modelo").await.unwrap();
    assert_eq!(r.len(), 2);
    assert!(r[0].multimodal && !r[0].pede_aceite);
    assert_eq!(r[0].licenca.as_deref(), Some("apache-2.0"));
    assert!(r[1].pede_aceite, "o restrito pede aceite");
    assert!(m.hub_search("x").await.is_err(), "busca curta demais");

    // arquivos do repositório
    let repo = m.hub_repo("teste/modelo-GGUF").await.unwrap();
    assert_eq!(repo.revisao, "rev123");
    assert_eq!(repo.arquivos.len(), 2, "{:?}", repo.arquivos);
    assert_eq!(repo.mmproj.len(), 1);
    let q8 = repo.arquivos.iter().find(|f| f.partes.len() == 2).unwrap();
    assert_eq!(q8.quantizacao.as_deref(), Some("Q8_0"));
    assert_eq!(q8.tamanho, 270_000);

    // cabeçalho por Range: sabe se cabe antes de baixar
    let rec = ignis_core::budget::Recursos { usa_placa: true, vram_livre_mb: Some(8000), ram_livre_mb: 16000 };
    let info = m.hub_info("teste/modelo-GGUF", "rev123", "modelo-Q4_K_M.gguf", &rec).await.unwrap();
    assert_eq!(info["info"]["camadas"], 28);
    assert_eq!(info["plano"]["encaixe"], "placa");

    // download com o arquivo de visão (mmproj)
    let tid = m.start_hub_download("teste/modelo-GGUF".into(), "modelo-Q4_K_M.gguf".into(), Some("mmproj-modelo-f16.gguf".into())).unwrap();
    esperar("baixar o Q4", Duration::from_secs(20), || estado(m, &tid).map(|e| e.0 == "concluida").unwrap_or(false)).await;
    let im = m.installed_models().into_iter().find(|x| x.id == "hf/teste/modelo-GGUF/modelo-Q4_K_M").expect("instalado");
    assert_eq!(im.nome, "modelo (Q4_K_M)");
    assert!(im.mmproj.as_ref().unwrap().is_file());
    assert_eq!(im.tamanho, 250_000, "tamanho conta o mmproj");
    assert!(!hf.auth_no_publico.load(Ordering::SeqCst), "sem token configurado, nada de cabeçalho de autorização");
    // o motor recebe o --mmproj
    let carregado = m.ensure_loaded(&im.id).await.unwrap();
    let v: Value = cliente().get(format!("{}/v1/models", carregado.base_url)).send().await.unwrap().json().await.unwrap();
    assert_eq!(v["data"][0]["mmproj"], json!(true), "llama-server subiu com --mmproj");
    m.unload().await;

    // modelo dividido em partes
    let tid2 = m.start_hub_download("teste/modelo-GGUF".into(), "grande/modelo-Q8_0.gguf".into(), None).unwrap();
    esperar("baixar o Q8", Duration::from_secs(20), || estado(m, &tid2).map(|e| e.0 == "concluida").unwrap_or(false)).await;
    let q8 = m.installed_models().into_iter().find(|x| x.id == "hf/teste/modelo-GGUF/modelo-Q8_0").expect("Q8 instalado");
    assert_eq!(q8.partes.len(), 1);
    assert!(q8.arquivo.file_name().unwrap().to_string_lossy().ends_with("00001-of-00002.gguf"));
    assert_eq!(q8.partes[0].parent(), q8.arquivo.parent(), "partes na mesma pasta (o llama.cpp exige)");

    // modelo que pede aceite, sem token: explica o que fazer
    let tid3 = m.start_hub_download("empresa/restrito-GGUF".into(), "restrito-Q4_0.gguf".into(), None).unwrap();
    esperar("falhar pedindo aceite", Duration::from_secs(20), || estado(m, &tid3).map(|e| e.0 == "erro").unwrap_or(false)).await;
    let erro = estado(m, &tid3).unwrap().1.unwrap();
    assert!(erro.contains("aceitar os termos"), "{erro}");

    // token errado é recusado; token certo é guardado e libera o download
    assert!(m.set_hf_token("hf_ruim").await.unwrap_err().to_string().contains("recusado"));
    let conta = m.set_hf_token("hf_bom").await.unwrap();
    assert_eq!(conta["nome"], "pessoa-teste");
    assert!(ignis_core::hub::token_path(&amb.paths.config_dir).exists());
    let tid4 = m.start_hub_download("empresa/restrito-GGUF".into(), "restrito-Q4_0.gguf".into(), None).unwrap();
    esperar("baixar com a conta", Duration::from_secs(20), || estado(m, &tid4).map(|e| e.0 == "concluida").unwrap_or(false)).await;
    assert!(m.installed_models().iter().any(|x| x.id == "hf/empresa/restrito-GGUF/restrito-Q4_0"));

    // remover apaga todas as partes
    let arquivos_q8: Vec<std::path::PathBuf> = std::iter::once(q8.arquivo.clone()).chain(q8.partes.clone()).collect();
    m.remove_model(&q8.id).await.unwrap();
    assert!(arquivos_q8.iter().all(|f| !f.exists()), "partes apagadas");
    // remover a conta
    let conta = m.set_hf_token("").await.unwrap();
    assert_eq!(conta["configurado"], json!(false));
}
