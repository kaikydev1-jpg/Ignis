//! Imitações do `tts-server` (omnivoice.cpp) e do `whisper-server` (whisper.cpp).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use super::arg;

struct Voz {
    pronto_em: Instant,
    vozes: Mutex<Vec<String>>,
}

/// WAV mínimo (16 bits, 24 kHz, mono) com `n` amostras.
fn wav(n: usize) -> Vec<u8> {
    let dados = (n * 2) as u32;
    let mut v = Vec::with_capacity(44 + n * 2);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + dados).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&24000u32.to_le_bytes());
    v.extend_from_slice(&48000u32.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&dados.to_le_bytes());
    v.resize(44 + n * 2, 0);
    v
}

fn arquivo_existe(args: &[String], nomes: &[&str]) -> Option<String> {
    let p = arg(args, nomes)?;
    std::path::Path::new(&p).is_file().then_some(p)
}

pub async fn voz(args: Vec<String>, demora_ms: u64) {
    if arquivo_existe(&args, &["--model"]).is_none() || arquivo_existe(&args, &["--codec"]).is_none() {
        eprintln!("[Server] FATAL: failed to open model or codec");
        std::process::exit(1);
    }
    let port: u16 = arg(&args, &["--port"]).and_then(|p| p.parse().ok()).unwrap_or(8080);
    eprintln!("[Server] fake tts-server porta {port}");
    let app = Arc::new(Voz { pronto_em: Instant::now() + Duration::from_millis(demora_ms), vozes: Mutex::new(Vec::new()) });
    let r = Router::new()
        .route("/health", get(|State(a): State<Arc<Voz>>| async move { saude(a.pronto_em) }))
        .route("/v1/models", get(|| async { Json(json!({"object": "list", "data": [{"id": "omnivoice-base.gguf", "object": "model"}]})) }))
        .route("/v1/audio/voices", get(listar_vozes).post(registrar_voz))
        .route("/v1/audio/speech", post(falar))
        .layer(DefaultBodyLimit::disable())
        .with_state(app);
    servir(port, r).await;
}

fn saude(pronto_em: Instant) -> Response {
    if Instant::now() < pronto_em {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status": "loading"}))).into_response();
    }
    Json(json!({"status": "ok"})).into_response()
}

async fn servir(port: u16, r: Router) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.expect("bind");
    axum::serve(listener, r).await.unwrap();
}

async fn listar_vozes(State(a): State<Arc<Voz>>) -> Json<Value> {
    let v: Vec<Value> = a.vozes.lock().unwrap().iter().map(|n| json!({"name": n})).collect();
    Json(json!({"object": "list", "data": v}))
}

async fn registrar_voz(State(a): State<Arc<Voz>>, Json(b): Json<Value>) -> Response {
    let nome = b.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let wav = b.get("wav_b64").and_then(|x| x.as_str()).unwrap_or("");
    let rvq = b.get("rvq_b64").and_then(|x| x.as_str()).unwrap_or("");
    if nome.is_empty() || (wav.is_empty() && rvq.is_empty()) || wav == "INVALIDO" {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": {"message": "'wav_b64' does not decode to a valid WAV"}}))).into_response();
    }
    let mut v = a.vozes.lock().unwrap();
    v.retain(|n| n != &nome);
    v.push(nome.clone());
    Json(json!({"name": nome})).into_response()
}

async fn falar(State(a): State<Arc<Voz>>, Json(b): Json<Value>) -> Response {
    let texto = b.get("input").and_then(|x| x.as_str()).unwrap_or("");
    if texto.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": {"message": "'input' is required"}}))).into_response();
    }
    if let Some(v) = b.get("voice").and_then(|x| x.as_str()) {
        if !a.vozes.lock().unwrap().iter().any(|n| n == v) {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": {"message": format!("unknown voice '{v}'")}}))).into_response();
        }
    }
    let amostras = 2400 * texto.chars().count().max(1);
    // igual ao tts-server de verdade: sem "response_format", devolve PCM cru
    if matches!(b.get("response_format").and_then(|x| x.as_str()), Some("pcm") | None) {
        return Response::builder().header("content-type", "audio/pcm").body(Body::from(vec![0u8; amostras * 2])).unwrap();
    }
    Response::builder().header("content-type", "audio/wav").body(Body::from(wav(amostras))).unwrap()
}

/// Valor de um campo de texto num corpo multipart (busca simples).
fn campo(corpo: &[u8], nome: &str) -> Option<String> {
    let s = String::from_utf8_lossy(corpo);
    let marca = format!("name=\"{nome}\"\r\n\r\n");
    let i = s.find(&marca)? + marca.len();
    Some(s[i..].split("\r\n").next().unwrap_or("").to_string())
}

pub async fn transcricao(args: Vec<String>, demora_ms: u64) {
    if arquivo_existe(&args, &["-m", "--model"]).is_none() {
        eprintln!("whisper_init_from_file: failed to open model");
        std::process::exit(1);
    }
    let port: u16 = arg(&args, &["--port"]).and_then(|p| p.parse().ok()).unwrap_or(8080);
    let rota = arg(&args, &["--inference-path"]).unwrap_or_else(|| "/inference".into());
    eprintln!("whisper server fake porta {port} rota {rota}");
    let pronto_em = Instant::now() + Duration::from_millis(demora_ms);
    let r = Router::new()
        .route("/health", get(move || async move { saude(pronto_em) }))
        .route(&rota, post(transcrever))
        .layer(DefaultBodyLimit::disable());
    servir(port, r).await;
}

async fn transcrever(body: Bytes) -> Response {
    let s = String::from_utf8_lossy(&body);
    if !s.contains("name=\"file\"") {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "no 'file' field in the request"}))).into_response();
    }
    let traduzir = campo(&body, "translate").as_deref() == Some("true");
    let texto = if traduzir { "Hello, this is a test." } else { "Olá, este é um teste." };
    match campo(&body, "response_format").as_deref() {
        Some("text") => {
            Response::builder().header("content-type", "text/plain; charset=utf-8").body(Body::from(format!("{texto}\n"))).unwrap()
        }
        _ => Json(json!({
            "task": if traduzir { "translate" } else { "transcribe" },
            "language": campo(&body, "language").unwrap_or_else(|| "pt".into()),
            "duration": 1.5,
            "text": texto,
            "segments": [{"id": 0, "start": 0.0, "end": 1.5, "text": texto}],
        }))
        .into_response(),
    }
}
