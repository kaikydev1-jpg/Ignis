//! Imitação do `llama-server` para testes ("placa simulada").
//!
//! Lê o comportamento de `fake.json` na mesma pasta do executável (ou do
//! caminho em `FAKE_LLAMA_CONFIG`). Assim cada "motor" de teste (ROCm,
//! Vulkan, CPU...) é uma cópia deste programa com um `fake.json` diferente.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

mod audio;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct Config {
    /// "llama" (padrão), "voz" (imita o tts-server) ou "transcricao" (imita o whisper-server)
    tipo: String,
    /// "ok", "falha_ao_subir", "cai_depois"
    modo: String,
    cai_depois_ms: u64,
    demora_subir_ms: u64,
    tps_leitura: f64,
    tps_geracao: f64,
    nome_dispositivo: String,
    vram_mb: u64,
    log_erro: String,
    /// Arquivo onde gravar quantos tokens já foram enviados (teste do Parar).
    arquivo_contador: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tipo: "llama".into(),
            modo: "ok".into(),
            cai_depois_ms: 0,
            demora_subir_ms: 100,
            tps_leitura: 500.0,
            tps_geracao: 50.0,
            nome_dispositivo: "Placa Simulada".into(),
            vram_mb: 8192,
            log_erro: "erro simulado".into(),
            arquivo_contador: None,
        }
    }
}

struct App {
    cfg: Config,
    pronto_em: Instant,
    alias: String,
    embeddings: bool,
    mmproj: bool,
    tokens: AtomicU64,
}

fn load_config() -> Config {
    let path = std::env::var_os("FAKE_LLAMA_CONFIG")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("fake.json"))));
    path.and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn arg(args: &[String], names: &[&str]) -> Option<String> {
    args.iter().position(|a| names.contains(&a.as_str())).and_then(|i| args.get(i + 1).cloned())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg = load_config();
    if args.iter().any(|a| a == "--version") {
        eprintln!("version: 0 (fake)");
        return;
    }
    if args.iter().any(|a| a == "--list-devices") {
        println!("Available devices:");
        println!("  Fake0: {} ({} MiB, {} MiB free)", cfg.nome_dispositivo, cfg.vram_mb, cfg.vram_mb.saturating_sub(200));
        return;
    }
    if cfg.modo == "falha_ao_subir" {
        eprintln!("{}", cfg.log_erro);
        std::process::exit(1);
    }
    match cfg.tipo.as_str() {
        "voz" => return audio::voz(args, cfg.demora_subir_ms).await,
        "transcricao" => return audio::transcricao(args, cfg.demora_subir_ms).await,
        _ => {}
    }
    let model = arg(&args, &["-m", "--model"]).unwrap_or_default();
    if !std::path::Path::new(&model).is_file() {
        eprintln!("llama_model_load: error loading model: failed to open {model}");
        eprintln!("main: error: failed to load model '{model}'");
        std::process::exit(1);
    }
    let mmproj = arg(&args, &["--mmproj", "-mm"]);
    if let Some(mm) = &mmproj {
        if !std::path::Path::new(mm).is_file() {
            eprintln!("clip_init: failed to load mmproj {mm}");
            std::process::exit(1);
        }
    }
    let port: u16 = arg(&args, &["--port"]).and_then(|p| p.parse().ok()).unwrap_or(9931);
    let alias = arg(&args, &["-a", "--alias"]).unwrap_or_else(|| "modelo".into());
    let embeddings = args.iter().any(|a| a == "--embeddings" || a == "--embedding");
    eprintln!("main: fake llama-server porta {port} modelo {model}");

    let app = Arc::new(App {
        pronto_em: Instant::now() + Duration::from_millis(cfg.demora_subir_ms),
        cfg: cfg.clone(),
        alias,
        embeddings,
        mmproj: mmproj.is_some(),
        tokens: AtomicU64::new(0),
    });
    if cfg.modo == "cai_depois" {
        let ms = cfg.demora_subir_ms + cfg.cai_depois_ms;
        let msg = cfg.log_erro.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            eprintln!("{msg}");
            std::process::exit(134);
        });
    }
    let router = Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/completion", post(completion))
        .route("/v1/chat/completions", post(chat))
        .route("/v1/completions", post(chat))
        .route("/v1/embeddings", post(embeddings_route))
        // O llama-server oficial aceita pedidos grandes (fotos/áudio em base64).
        .layer(DefaultBodyLimit::disable())
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.expect("bind");
    axum::serve(listener, router).await.unwrap();
}

async fn health(State(app): State<Arc<App>>) -> Response {
    if Instant::now() < app.pronto_em {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": {"code": 503, "message": "Loading model"}}))).into_response();
    }
    Json(json!({"status": "ok"})).into_response()
}

async fn models(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({"object": "list", "data": [{"id": app.alias, "object": "model", "owned_by": "llamacpp", "mmproj": app.mmproj}]}))
}

fn timings(app: &App, prompt_n: u64, gen_n: u64) -> Value {
    json!({
        "prompt_n": prompt_n,
        "prompt_ms": prompt_n as f64 / app.cfg.tps_leitura * 1000.0,
        "prompt_per_second": app.cfg.tps_leitura,
        "predicted_n": gen_n,
        "predicted_ms": gen_n as f64 / app.cfg.tps_geracao * 1000.0,
        "predicted_per_second": app.cfg.tps_geracao,
    })
}

/// Endpoint nativo usado pelo teste de velocidade.
async fn completion(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Json<Value> {
    let prompt_n = (body["prompt"].as_str().unwrap_or("").len() as u64 / 4).max(1);
    let n = body["n_predict"].as_u64().unwrap_or(16);
    let secs = prompt_n as f64 / app.cfg.tps_leitura + n as f64 / app.cfg.tps_geracao;
    tokio::time::sleep(Duration::from_secs_f64(secs.min(5.0))).await;
    Json(json!({"content": "ok ".repeat(n as usize), "timings": timings(&app, prompt_n, n)}))
}

fn count(app: &App) {
    let n = app.tokens.fetch_add(1, Ordering::SeqCst) + 1;
    if let Some(p) = &app.cfg.arquivo_contador {
        let _ = std::fs::write(p, n.to_string());
    }
}

async fn chat(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Response {
    if app.embeddings {
        return (StatusCode::NOT_IMPLEMENTED, Json(json!({"error": {"code": 501, "message": "This server does not support completions. Start it without `--embeddings`", "type": "not_supported_error"}}))).into_response();
    }
    let max = body["max_tokens"].as_u64().or(body["n_predict"].as_u64()).unwrap_or(16);
    let stream = body["stream"].as_bool().unwrap_or(false);
    let delay = Duration::from_secs_f64(1.0 / app.cfg.tps_geracao.max(0.1));
    let alias = app.alias.clone();
    if !stream {
        let secs = max as f64 / app.cfg.tps_geracao;
        tokio::time::sleep(Duration::from_secs_f64(secs.min(5.0))).await;
        for _ in 0..max {
            count(&app);
        }
        return Json(json!({
            "id": "chatcmpl-fake", "object": "chat.completion", "created": 0, "model": alias,
            "choices": [{"index": 0, "finish_reason": "stop", "message": {"role": "assistant", "content": "palavra ".repeat(max as usize).trim_end()}}],
            "usage": {"prompt_tokens": 5, "completion_tokens": max, "total_tokens": 5 + max},
            "timings": timings(&app, 5, max),
        }))
        .into_response();
    }
    let app2 = app.clone();
    let s = futures_util::stream::unfold(0u64, move |i| {
        let app = app2.clone();
        let alias = alias.clone();
        async move {
            if i > max + 1 {
                return None;
            }
            let data = if i < max {
                tokio::time::sleep(delay).await;
                count(&app);
                json!({"id": "chatcmpl-fake", "object": "chat.completion.chunk", "created": 0, "model": alias,
                       "choices": [{"index": 0, "delta": {"content": "palavra "}, "finish_reason": null}]})
                .to_string()
            } else if i == max {
                json!({"id": "chatcmpl-fake", "object": "chat.completion.chunk", "created": 0, "model": alias,
                       "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}], "timings": timings(&app, 5, max)})
                .to_string()
            } else {
                "[DONE]".to_string()
            };
            Some((Ok::<_, std::io::Error>(bytes::Bytes::from(format!("data: {data}\n\n"))), i + 1))
        }
    });
    Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(s)).unwrap()
}

async fn embeddings_route(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Response {
    if !app.embeddings {
        return (StatusCode::NOT_IMPLEMENTED, Json(json!({"error": {"code": 501, "message": "This server does not support embeddings. Start it with `--embeddings`", "type": "not_supported_error"}}))).into_response();
    }
    let inputs: Vec<String> = match &body["input"] {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        _ => vec![],
    };
    let data: Vec<Value> = inputs
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let v: Vec<f32> = (0..8).map(|k| ((s.len() + k) % 7) as f32 / 7.0).collect();
            json!({"object": "embedding", "index": i, "embedding": v})
        })
        .collect();
    Json(json!({"object": "list", "data": data, "model": app.alias, "usage": {"prompt_tokens": 1, "total_tokens": 1}})).into_response()
}
