//! Porta de conversa local (só 127.0.0.1).
//!
//! - Compatível com a API da OpenAI: `/v1/models`, `/v1/chat/completions`
//!   (com streaming e Parar), `/v1/completions`, `/v1/embeddings`.
//! - Rotas próprias em `/motor/*` (estado, modelos, versão...).
//!
//! O contrato está documentado em CONTRATO_VALKODE.md.

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::budget::{self, Recursos};
use crate::config::SettingsPatch;
use crate::service::{Motor, MotorError};

pub const HEADER_REQUISICAO: &str = "x-ignis-requisicao";

type M = State<Arc<Motor>>;

fn err_response(status: u16, code: &str, msg: &str) -> Response {
    let st = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let tipo = match status {
        400 | 404 => "invalid_request_error",
        409 | 503 => "server_busy",
        _ => "server_error",
    };
    (st, Json(json!({"error": {"message": msg, "type": tipo, "code": code}, "erro": msg}))).into_response()
}

impl IntoResponse for MotorError {
    fn into_response(self) -> Response {
        err_response(self.status(), self.codigo(), &self.to_string())
    }
}

fn ok(v: Value) -> Response {
    Json(v).into_response()
}

// ---------- proteção: só este computador ----------

fn host_allowed(host: &str) -> bool {
    let h = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().map(|x| format!("[{x}]")).unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or("").to_string()
    };
    matches!(h.as_str(), "127.0.0.1" | "localhost" | "[::1]")
}

pub fn origin_allowed(o: &str) -> bool {
    let loopback = ["http://127.0.0.1", "https://127.0.0.1", "http://localhost", "https://localhost", "http://[::1]"];
    if loopback.iter().any(|p| o == *p || o.starts_with(&format!("{p}:"))) {
        return true;
    }
    matches!(o, "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost")
}

async fn guard(req: Request, next: Next) -> Response {
    if let Some(host) = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()) {
        if !host_allowed(host) {
            return err_response(403, "acesso_negado", "A API do Ignis só aceita pedidos deste computador (127.0.0.1).");
        }
    }
    let origin = req.headers().get(header::ORIGIN).and_then(|h| h.to_str().ok()).map(String::from);
    if let Some(o) = &origin {
        if !origin_allowed(o) {
            return err_response(403, "origem_negada", "Sites da internet não podem usar o Ignis. Só programas deste computador.");
        }
    }
    if req.method() == Method::OPTIONS {
        let mut r = StatusCode::NO_CONTENT.into_response();
        add_cors(&mut r, origin.as_deref());
        // A origem já foi conferida: aceita os cabeçalhos que o cliente pedir
        // (o cliente oficial da OpenAI manda x-stainless-* e outros).
        if origin.is_some() {
            if let Some(v) = req.headers().get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
                r.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_HEADERS, v.clone());
            }
        }
        return r;
    }
    let mut r = next.run(req).await;
    add_cors(&mut r, origin.as_deref());
    r
}

fn add_cors(r: &mut Response, origin: Option<&str>) {
    if let Some(o) = origin {
        let h = r.headers_mut();
        if let Ok(v) = HeaderValue::from_str(o) {
            h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
        }
        h.insert(header::VARY, HeaderValue::from_static("Origin"));
        h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, DELETE, OPTIONS"));
        h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type, authorization"));
        h.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, HeaderValue::from_static(HEADER_REQUISICAO));
    }
}

// ---------- rotas ----------

/// Tamanho máximo de um pedido. Fotos e áudio vão em base64 dentro do JSON e
/// passam fácil do limite padrão do axum (2 MB). A porta é só local.
pub const LIMITE_PEDIDO: usize = 256 * 1024 * 1024;

pub fn router(motor: Arc<Motor>) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/v1/models", get(v1_models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/completions", post(completions))
        .route("/v1/embeddings", post(embeddings))
        .route("/motor/estado", get(estado))
        .route("/motor/versao", get(versao))
        .route("/motor/relatorio", get(relatorio))
        .route("/motor/registro", get(registro))
        .route("/motor/modelos", get(modelos))
        .route("/motor/modelos/baixar", post(baixar))
        .route("/motor/modelos/cancelar", post(cancelar))
        .route("/motor/modelos/remover", post(remover))
        .route("/motor/modelos/ollama", get(ollama_listar))
        .route("/motor/modelos/importar", post(importar))
        .route("/motor/modelos/carregar", post(carregar))
        .route("/motor/modelos/descarregar", post(descarregar))
        .route("/motor/modelos/padrao", post(padrao))
        .route("/motor/parar", post(parar))
        .route("/motor/pausar", post(pausar))
        .route("/motor/retomar", post(retomar))
        .route("/motor/preparar", post(preparar))
        .route("/motor/testar", post(testar))
        .route("/motor/detectar", post(detectar))
        .route("/motor/motores", get(motores))
        .route("/motor/motores/forcar", post(forcar))
        .route("/motor/tarefas/cancelar", post(cancelar))
        .route("/motor/config", get(config_get).post(config_set))
        .route("/motor/atualizacoes", get(atualizacoes))
        .route("/motor/atualizacoes/procurar", post(atualizacoes_procurar))
        .route("/motor/atualizacoes/aplicar", post(atualizacoes_aplicar))
        .route("/motor/hf/buscar", get(hf_buscar))
        .route("/motor/hf/repo", get(hf_repo))
        .route("/motor/hf/info", post(hf_info))
        .route("/motor/hf/baixar", post(hf_baixar))
        .route("/motor/hf/conta", get(hf_conta).post(hf_conta_salvar))
        .layer(DefaultBodyLimit::max(LIMITE_PEDIDO))
        .layer(middleware::from_fn(guard))
        .with_state(motor)
}

async fn root() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "Ignis rodando. Estado em /motor/estado; API compatível com OpenAI em /v1.\n",
    )
        .into_response()
}

async fn v1_models(State(m): M) -> Response {
    let carregado = m.loaded_info().map(|i| i.modelo_id);
    let data: Vec<Value> = m
        .installed_models()
        .into_iter()
        .filter(|x| x.existe())
        .map(|x| {
            json!({
                "id": x.id,
                "object": "model",
                "created": x.instalado_em,
                "owned_by": "ignis",
                "ignis": {
                    "nome": x.nome,
                    "tamanho": x.tamanho,
                    "embeddings": x.embeddings,
                    "carregado": carregado.as_deref() == Some(x.id.as_str()),
                    "contexto_max": x.info.as_ref().map(|i| i.contexto_treino),
                }
            })
        })
        .collect();
    ok(json!({"object": "list", "data": data}))
}

async fn chat_completions(State(m): M, body: Bytes) -> Response {
    proxy(m, "/v1/chat/completions", false, body).await
}
async fn completions(State(m): M, body: Bytes) -> Response {
    proxy(m, "/v1/completions", false, body).await
}
async fn embeddings(State(m): M, body: Bytes) -> Response {
    proxy(m, "/v1/embeddings", true, body).await
}

#[derive(Deserialize)]
struct Timings {
    prompt_per_second: Option<f64>,
    predicted_per_second: Option<f64>,
}

fn sniff_timings(m: &Motor, v: &Value) {
    if let Some(t) = v.get("timings") {
        if let Ok(t) = serde_json::from_value::<Timings>(t.clone()) {
            m.record_speed(t.prompt_per_second, t.predicted_per_second);
        }
    }
}

async fn proxy(m: Arc<Motor>, path: &'static str, embeddings: bool, body: Bytes) -> Response {
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return err_response(400, "json_invalido", "O corpo do pedido precisa ser JSON válido."),
    };
    let pedido = parsed.get("model").and_then(|v| v.as_str());
    let stream = parsed.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut guard = m.begin_request();
    let id = match m.resolve_model(pedido, embeddings) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let info = tokio::select! {
        _ = guard.cancel.cancelled() => return err_response(499, "parado", "Pedido parado."),
        r = m.ensure_loaded(&id) => match r {
            Ok(i) => i,
            Err(e) => return e.into_response(),
        },
    };
    guard.activate();
    let req_id = guard.id.to_string();
    let url = format!("{}{}", info.base_url, path);
    let send = m.http_local().post(url).header(header::CONTENT_TYPE, "application/json").body(body).send();
    let resp = tokio::select! {
        _ = guard.cancel.cancelled() => return err_response(499, "parado", "Pedido parado."),
        r = send => match r {
            Ok(r) => r,
            Err(e) => return err_response(502, "motor_sem_resposta", &format!("O motor não respondeu ({e}). Ele pode ter caído; tente de novo.")),
        },
    };
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let ctype = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/json").to_string();
    if !(stream && ctype.starts_with("text/event-stream")) {
        let bytes = tokio::select! {
            _ = guard.cancel.cancelled() => return err_response(499, "parado", "Pedido parado."),
            b = resp.bytes() => match b {
                Ok(b) => b,
                Err(e) => return err_response(502, "motor_sem_resposta", &format!("A resposta do motor foi interrompida ({e}).")),
            },
        };
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
            sniff_timings(&m, &v);
        }
        drop(guard);
        return Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, ctype)
            .header(HEADER_REQUISICAO, req_id)
            .body(Body::from(bytes))
            .unwrap();
    }

    struct St {
        up: futures_util::stream::BoxStream<'static, reqwest::Result<Bytes>>,
        guard: crate::service::RequestGuard,
        motor: Arc<Motor>,
        carry: String,
        done: bool,
    }
    let st = St { up: resp.bytes_stream().boxed(), guard, motor: m.clone(), carry: String::new(), done: false };
    let s = futures_util::stream::unfold(st, |mut st| async move {
        if st.done {
            return None;
        }
        tokio::select! {
            _ = st.guard.cancel.cancelled() => {
                st.done = true;
                Some((Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: [DONE]\n\n")), st))
            }
            c = st.up.next() => match c {
                Some(Ok(b)) => {
                    // procura "timings" no fim da resposta para mostrar a velocidade
                    if let Ok(text) = std::str::from_utf8(&b) {
                        st.carry.push_str(text);
                        if st.carry.contains("\"timings\"") {
                            for line in st.carry.lines() {
                                if let Some(j) = line.strip_prefix("data: ") {
                                    if let Ok(v) = serde_json::from_str::<Value>(j) {
                                        sniff_timings(&st.motor, &v);
                                    }
                                }
                            }
                        }
                        if let Some(pos) = st.carry.rfind('\n') {
                            st.carry.drain(..=pos);
                        }
                        if st.carry.len() > 65536 {
                            st.carry.clear();
                        }
                    }
                    Some((Ok(b), st))
                }
                Some(Err(e)) => {
                    st.done = true;
                    Some((Err(std::io::Error::other(e)), st))
                }
                None => None,
            },
        }
    });
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(HEADER_REQUISICAO, req_id)
        .body(Body::from_stream(s))
        .unwrap()
}

async fn estado(State(m): M) -> Response {
    ok(m.status_json())
}

async fn versao(State(m): M) -> Response {
    ok(m.version_json())
}

async fn relatorio(State(m): M) -> Response {
    let det = m.detection();
    ok(json!({
        "versao": m.version_json(),
        "deteccao": det,
        "impressao_digital": det.as_ref().map(|d| d.fingerprint()),
        "candidatos": m.candidates(),
        "teste": m.bench(),
        "configuracao": m.settings(),
        "avisos": m.avisos(),
    }))
}

async fn registro(State(m): M) -> Response {
    ok(json!({"linhas": m.server_log(200)}))
}

fn recursos_para(m: &Motor) -> Recursos {
    let det = m.detection();
    let principal = det.as_ref().and_then(|d| d.principal()).cloned();
    let melhor = m.engine_order().into_iter().next().map(|(c, _)| c).or_else(|| m.candidates().into_iter().find(|c| c.disponivel));
    Recursos {
        usa_placa: principal.is_some() && melhor.map(|c| c.backend.usa_placa()).unwrap_or(true),
        vram_livre_mb: principal.and_then(|g| g.vram_livre_mb.or(g.vram_total_mb)),
        ram_livre_mb: det.map(|d| d.ram_livre_mb).unwrap_or(0),
    }
}

async fn modelos(State(m): M) -> Response {
    let rec = recursos_para(&m);
    let ctx = m.settings().contexto_preferido;
    let padrao = m.settings().modelo_padrao;
    let carregado = m.loaded_info().map(|i| i.modelo_id);
    let tarefas = m.tarefas();
    let instalados: Vec<Value> = m
        .installed_models()
        .into_iter()
        .map(|x| {
            let plano = budget::plan(&x.shape(), &rec, ctx);
            json!({
                "id": x.id, "nome": x.nome, "tamanho": x.tamanho, "origem": x.origem, "embeddings": x.embeddings,
                "existe": x.existe(), "carregado": carregado.as_deref() == Some(x.id.as_str()),
                "padrao": padrao.as_deref() == Some(x.id.as_str()), "licenca": x.licenca, "arquivo": x.arquivo,
                "encaixe": plano.encaixe, "rotulo": plano.encaixe.rotulo(), "aviso": plano.aviso, "contexto": plano.contexto,
                "memoria_placa_mb": plano.memoria_placa_mb, "memoria_ram_mb": plano.memoria_ram_mb,
                "visao": x.mmproj.is_some(),
                "tipo": if x.embeddings { "embeddings" } else { "conversa" },
            })
        })
        .collect();
    let ids_instalados: Vec<String> = m.installed_models().iter().filter(|x| x.existe()).map(|x| x.id.clone()).collect();
    let models_dir = m.paths.models_dir();
    let catalogo: Vec<Value> = m
        .catalog()
        .modelos
        .into_iter()
        .map(|c| {
            let plano = budget::plan(&c.shape(), &rec, ctx);
            let tarefa = tarefas.iter().find(|t| t.id == format!("modelo:{}", c.id));
            let parcial = crate::download::partial_size(&models_dir.join(&c.arquivo)).unwrap_or(0);
            let mut v = serde_json::to_value(&c).unwrap_or(Value::Null);
            if let Value::Object(o) = &mut v {
                o.insert("instalado".into(), json!(ids_instalados.contains(&c.id)));
                o.insert("encaixe".into(), json!(plano.encaixe));
                o.insert("rotulo".into(), json!(plano.encaixe.rotulo()));
                o.insert("aviso".into(), json!(plano.aviso));
                o.insert("memoria_placa_mb".into(), json!(plano.memoria_placa_mb));
                o.insert("memoria_ram_mb".into(), json!(plano.memoria_ram_mb));
                o.insert("contexto_plano".into(), json!(plano.contexto));
                let recomendado = plano.encaixe == budget::Encaixe::Placa
                    || (!rec.usa_placa && plano.encaixe == budget::Encaixe::Processador && c.parametros_bilhoes <= 4.0);
                o.insert("recomendado".into(), json!(recomendado));
                o.insert("parcial".into(), json!(parcial));
                o.insert("tarefa".into(), json!(tarefa));
            }
            v
        })
        .collect();
    ok(json!({"instalados": instalados, "catalogo": catalogo, "tarefas": tarefas, "recursos": rec}))
}

#[derive(Deserialize)]
struct IdBody {
    id: Option<String>,
}

async fn baixar(State(m): M, Json(b): Json<IdBody>) -> Response {
    let Some(id) = b.id else { return err_response(400, "falta_id", "Informe o id do modelo do catálogo.") };
    match m.start_model_download(&id) {
        Ok(t) => ok(json!({"ok": true, "tarefa": t})),
        Err(e) => e.into_response(),
    }
}

async fn cancelar(State(m): M, Json(b): Json<IdBody>) -> Response {
    let Some(id) = b.id else { return err_response(400, "falta_id", "Informe o id.") };
    ok(json!({"ok": m.cancel_task(&id)}))
}

async fn remover(State(m): M, Json(b): Json<IdBody>) -> Response {
    let Some(id) = b.id else { return err_response(400, "falta_id", "Informe o id do modelo.") };
    match m.remove_model(&id).await {
        Ok(msg) => ok(json!({"ok": true, "mensagem": msg})),
        Err(e) => e.into_response(),
    }
}

async fn ollama_listar(State(m): M) -> Response {
    let mm = m.clone();
    let lista = tokio::task::spawn_blocking(move || mm.ollama_scan()).await.unwrap_or_default();
    let pastas: Vec<String> = crate::ollama::default_dirs().iter().map(|p| p.display().to_string()).collect();
    ok(json!({"modelos": lista, "pastas_procuradas": pastas}))
}

#[derive(Deserialize)]
struct ImportBody {
    referencia: Option<String>,
    modo: Option<String>,
    caminho: Option<String>,
}

async fn importar(State(m): M, Json(b): Json<ImportBody>) -> Response {
    if let Some(c) = b.caminho {
        return match m.import_file(std::path::Path::new(&c)) {
            Ok(id) => ok(json!({"ok": true, "id": id})),
            Err(e) => e.into_response(),
        };
    }
    let Some(r) = b.referencia else { return err_response(400, "falta_referencia", "Informe a referência do Ollama (ex.: llama3.2:3b).") };
    let modo = b.modo.unwrap_or_else(|| "usar".into());
    match m.import_ollama(&r, &modo, &[]) {
        Ok(id) => ok(json!({"ok": true, "id": id})),
        Err(e) => e.into_response(),
    }
}

async fn carregar(State(m): M, Json(b): Json<IdBody>) -> Response {
    let pedido = b.id.as_deref();
    let embeddings = pedido.and_then(|p| m.installed_models().into_iter().find(|x| x.id == p)).map(|x| x.embeddings).unwrap_or(false);
    let id = match m.resolve_model(pedido, embeddings) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    match m.ensure_loaded(&id).await {
        Ok(i) => ok(json!({"ok": true, "modelo": i.modelo_id, "motor": i.variante, "contexto": i.plano.contexto})),
        Err(e) => e.into_response(),
    }
}

async fn descarregar(State(m): M) -> Response {
    ok(json!({"ok": m.unload().await}))
}

async fn padrao(State(m): M, Json(b): Json<IdBody>) -> Response {
    match m.update_settings(SettingsPatch { modelo_padrao: Some(b.id.unwrap_or_default()), ..Default::default() }) {
        Ok(_) => ok(json!({"ok": true})),
        Err(e) => e.into_response(),
    }
}

#[derive(Deserialize, Default)]
struct PararBody {
    id: Option<serde_json::Value>,
}

async fn parar(State(m): M, body: Bytes) -> Response {
    let b: PararBody = serde_json::from_slice(&body).unwrap_or_default();
    let id = b.id.and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
    ok(json!({"ok": true, "paradas": m.stop_requests(id)}))
}

async fn pausar(State(m): M) -> Response {
    m.pause().await;
    ok(json!({"ok": true, "pausado": true}))
}

async fn retomar(State(m): M) -> Response {
    m.resume();
    ok(json!({"ok": true, "pausado": false}))
}

async fn preparar(State(m): M) -> Response {
    match m.prepare() {
        Ok(()) => ok(json!({"ok": true, "plano": m.preparation_plan()})),
        Err(e) => e.into_response(),
    }
}

async fn testar(State(m): M) -> Response {
    m.run_benchmark();
    ok(json!({"ok": true}))
}

async fn detectar(State(m): M) -> Response {
    let d = m.detect_now().await;
    ok(json!({"ok": true, "placa": d.principal()}))
}

async fn motores(State(m): M) -> Response {
    let man = m.manifest();
    let ordem: Vec<String> = m.engine_order().into_iter().map(|(c, _)| c.variante).collect();
    let cands: Vec<Value> = m
        .candidates()
        .into_iter()
        .map(|c| {
            let instalado = m.engine_exe(&c.variante).is_some();
            let mut v = serde_json::to_value(&c).unwrap_or(Value::Null);
            if let Value::Object(o) = &mut v {
                o.insert("instalado".into(), json!(instalado));
            }
            v
        })
        .collect();
    ok(json!({
        "llama_cpp": man.llama_cpp,
        "candidatos": cands,
        "ordem": ordem,
        "forcado": m.settings().motor_forcado,
        "preparacao": m.preparation_plan(),
    }))
}

#[derive(Deserialize)]
struct ForcarBody {
    variante: Option<String>,
}

async fn forcar(State(m): M, Json(b): Json<ForcarBody>) -> Response {
    let v = b.variante.filter(|s| !s.is_empty() && s != "auto");
    match m.force_engine(v) {
        Ok(()) => {
            // recarrega no motor novo se estiver parado
            if m.ativos() == 0 && m.loaded_info().is_some() {
                m.unload().await;
            }
            ok(json!({"ok": true}))
        }
        Err(e) => e.into_response(),
    }
}

async fn config_get(State(m): M) -> Response {
    let mut v = serde_json::to_value(m.settings()).unwrap_or(Value::Null);
    if let Value::Object(o) = &mut v {
        o.insert("abrir_com_computador_ativo".into(), json!(crate::autostart::is_enabled()));
    }
    ok(v)
}

async fn config_set(State(m): M, Json(p): Json<SettingsPatch>) -> Response {
    match m.update_settings(p) {
        Ok(s) => ok(serde_json::to_value(s).unwrap_or(Value::Null)),
        Err(e) => e.into_response(),
    }
}

async fn atualizacoes(State(m): M) -> Response {
    ok(json!({"info": m.update_info()}))
}

async fn atualizacoes_procurar(State(m): M) -> Response {
    match m.check_updates().await {
        Ok(r) => ok(json!({"ok": true, "info": r})),
        Err(e) => e.into_response(),
    }
}

#[derive(Deserialize)]
struct AplicarBody {
    item: String,
}

async fn atualizacoes_aplicar(State(m): M, Json(b): Json<AplicarBody>) -> Response {
    match m.apply_update(&b.item) {
        Ok(()) => ok(json!({"ok": true})),
        Err(e) => e.into_response(),
    }
}

// ---------- Hugging Face ----------

type Q = axum::extract::Query<std::collections::HashMap<String, String>>;

async fn hf_buscar(State(m): M, q: Q) -> Response {
    let termo = q.get("q").cloned().unwrap_or_default();
    match m.hub_search(&termo).await {
        Ok(v) => ok(json!({"modelos": v, "conta": m.hf_conta()})),
        Err(e) => e.into_response(),
    }
}

async fn hf_repo(State(m): M, q: Q) -> Response {
    let repo = q.get("repo").cloned().unwrap_or_default();
    let r = match m.hub_repo(&repo).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    // estimativa só pelo tamanho (o cabeçalho exato é lido em /motor/hf/info)
    let rec = recursos_para(&m);
    let ctx = m.settings().contexto_preferido;
    let arquivos: Vec<Value> = r
        .arquivos
        .iter()
        .map(|f| {
            let shape = budget::ModelShape {
                tamanho_arquivo: f.tamanho,
                camadas: 32,
                kv_bytes_por_token: 128 * 1024,
                contexto_max: 32_768,
                embd: 4096,
            };
            let plano = budget::plan(&shape, &rec, ctx);
            let mut v = serde_json::to_value(f).unwrap_or(Value::Null);
            if let Value::Object(o) = &mut v {
                o.insert("encaixe".into(), json!(plano.encaixe));
                o.insert("rotulo".into(), json!(plano.encaixe.rotulo()));
                o.insert("memoria_placa_mb".into(), json!(plano.memoria_placa_mb));
                o.insert("primeira_parte".into(), json!(f.partes.first().map(|p| p.nome.clone())));
            }
            v
        })
        .collect();
    ok(json!({
        "repo": r.repo, "revisao": r.revisao, "pede_aceite": r.pede_aceite, "licenca": r.licenca,
        "arquivos": arquivos, "mmproj": r.mmproj, "conta": m.hf_conta(), "recursos": rec,
        "pagina": format!("https://huggingface.co/{}", r.repo),
    }))
}

#[derive(Deserialize)]
struct HfArquivo {
    repo: String,
    #[serde(default)]
    revisao: Option<String>,
    arquivo: String,
    #[serde(default)]
    mmproj: Option<String>,
}

async fn hf_info(State(m): M, Json(b): Json<HfArquivo>) -> Response {
    let rec = recursos_para(&m);
    match m.hub_info(&b.repo, b.revisao.as_deref().unwrap_or("main"), &b.arquivo, &rec).await {
        Ok(v) => ok(v),
        Err(e) => e.into_response(),
    }
}

async fn hf_baixar(State(m): M, Json(b): Json<HfArquivo>) -> Response {
    match m.start_hub_download(b.repo, b.arquivo, b.mmproj) {
        Ok(t) => ok(json!({"ok": true, "tarefa": t})),
        Err(e) => e.into_response(),
    }
}

async fn hf_conta(State(m): M) -> Response {
    ok(m.hf_conta())
}

#[derive(Deserialize)]
struct TokenBody {
    token: String,
}

async fn hf_conta_salvar(State(m): M, Json(b): Json<TokenBody>) -> Response {
    match m.set_hf_token(&b.token).await {
        Ok(v) => ok(v),
        Err(e) => e.into_response(),
    }
}

/// Mantém a API no ar, trocando de porta quando a configuração mudar.
pub async fn serve(motor: Arc<Motor>) {
    let mut rx = motor.porta_rx();
    loop {
        let porta = *rx.borrow_and_update();
        match tokio::net::TcpListener::bind(("127.0.0.1", porta)).await {
            Ok(listener) => {
                tracing::info!("API do Ignis em http://127.0.0.1:{porta}");
                motor.set_api_status(Ok(porta));
                let shutdown = motor.shutdown.clone();
                let mut rx2 = rx.clone();
                let app = router(motor.clone());
                let r = axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        tokio::select! {
                            _ = shutdown.cancelled() => {}
                            _ = rx2.changed() => {}
                        }
                    })
                    .await;
                if let Err(e) = r {
                    tracing::error!("API parou: {e}");
                }
            }
            Err(e) => {
                let msg = format!("A porta {porta} já está em uso por outro programa ({e}). Troque a porta na seção Motor.");
                motor.set_api_status(Err(msg.clone()));
                motor.aviso("erro", msg);
                tokio::select! {
                    _ = motor.shutdown.cancelled() => {}
                    _ = rx.changed() => {}
                    _ = tokio::time::sleep(Duration::from_secs(15)) => {}
                }
            }
        }
        if motor.shutdown.is_cancelled() {
            break;
        }
    }
}

/// Atende um pedido sem rede (usado pela janela do aplicativo via Tauri).
pub async fn call(motor: Arc<Motor>, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    use tower::ServiceExt;
    let method = Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET);
    let body = match body {
        Some(v) => Body::from(serde_json::to_vec(&v).unwrap_or_default()),
        None => Body::empty(),
    };
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap();
    let resp = match router(motor).oneshot(req).await {
        Ok(r) => r,
        Err(e) => match e {},
    };
    let status = resp.status().as_u16();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024 * 1024).await.unwrap_or_default();
    let v = serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({"texto": String::from_utf8_lossy(&bytes)}));
    (status, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_e_origens() {
        assert!(host_allowed("127.0.0.1:11470"));
        assert!(host_allowed("localhost:11470"));
        assert!(host_allowed("[::1]:11470"));
        assert!(!host_allowed("evil.com"));
        assert!(!host_allowed("127.0.0.1.evil.com:11470"));
        assert!(origin_allowed("http://localhost:3000"));
        assert!(origin_allowed("tauri://localhost"));
        assert!(!origin_allowed("https://evil.com"));
        assert!(!origin_allowed("http://localhost.evil.com"));
        assert!(!origin_allowed("null"));
    }
}
