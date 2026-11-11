//! Integração com o llama-server OFICIAL (motor de processador) e um modelo
//! pequeno de verdade (~0,5 B). Roda quando estas variáveis existem:
//!
//! - `IGNIS_TEST_ENGINE_ARCHIVE`: pacote oficial do llama.cpp para processador
//!   (ex.: llama-b12271-bin-ubuntu-x64.tar.gz ou llama-b12271-bin-win-cpu-x64.zip)
//! - `IGNIS_TEST_MODEL`: um GGUF pequeno (ex.: qwen2.5-0.5b-instruct-q4_k_m.gguf)
//!
//! O pacote é servido por um servidor HTTP local e instalado pelo caminho
//! normal (download + SHA-256 + extração), igual ao que acontece de verdade.

mod comum;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path as AxPath, State};
use axum::response::IntoResponse;
use comum::*;
use futures_util::StreamExt;
use ignis_core::distro::{Familia, Sistema};
use ignis_core::engine::manifest::current_os;
use ignis_core::engine::{Backend, EngineManifest, EngineStore};
use ignis_core::gpu::{Detection, RawSources};
use ignis_core::paths::Paths;
use ignis_core::service::{Motor, MotorOptions};
use serde_json::{json, Value};

fn env_path(k: &str) -> Option<PathBuf> {
    std::env::var_os(k).map(PathBuf::from).filter(|p| p.is_file())
}

async fn servir_arquivo(arquivo: PathBuf) -> String {
    async fn get(State(p): State<Arc<PathBuf>>, AxPath(nome): AxPath<String>) -> axum::response::Response {
        if p.file_name().map(|n| n.to_string_lossy() == nome).unwrap_or(false) {
            std::fs::read(&*p).unwrap().into_response()
        } else {
            axum::http::StatusCode::NOT_FOUND.into_response()
        }
    }
    let app = axum::Router::new().route("/{nome}", axum::routing::get(get)).with_state(Arc::new(arquivo));
    let l = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://{addr}/")
}

fn sem_placa() -> Detection {
    Detection {
        sistema: Sistema {
            familia: if cfg!(windows) { Familia::Windows } else { Familia::Fedora },
            nome: "Teste".into(),
            versao: None,
            flatpak: false,
            appimage: false,
        },
        arq: std::env::consts::ARCH.into(),
        placas: vec![],
        ram_total_mb: 8000,
        ram_livre_mb: 6000,
        libs: BTreeMap::new(),
        kfd_acessivel: false,
        ferramentas: vec![],
        fontes_brutas: RawSources::default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn llama_server_oficial_de_ponta_a_ponta() {
    let (Some(pacote), Some(modelo)) = (env_path("IGNIS_TEST_ENGINE_ARCHIVE"), env_path("IGNIS_TEST_MODEL")) else {
        eprintln!("pulando: defina IGNIS_TEST_ENGINE_ARCHIVE e IGNIS_TEST_MODEL");
        return;
    };
    let base_url = servir_arquivo(pacote.clone()).await;
    let mut manifest = EngineManifest::embedded();
    manifest.url_base = base_url; // servidor local de teste no lugar do GitHub
    let cpu = manifest.for_platform(current_os(), std::env::consts::ARCH).find(|v| v.backend == Backend::Cpu).unwrap().clone();
    assert_eq!(cpu.arquivos[0].nome, pacote.file_name().unwrap().to_string_lossy(), "pacote certo para este sistema");

    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::under(tmp.path());
    paths.ensure().unwrap();
    let porta = free_port();
    std::fs::write(
        paths.config_file(),
        serde_json::to_vec(
            &json!({"primeira_execucao_feita": true, "porta": porta, "descarregar_apos_seg": 0, "contexto_preferido": 2048}),
        )
        .unwrap(),
    )
    .unwrap();

    // 1) instalação real: download + SHA-256 + extração
    let store = EngineStore::new(paths.engines_dir(), paths.downloads_dir());
    let http = cliente();
    let t0 = Instant::now();
    let inst =
        store.install(&http, &manifest, &cpu, &tokio_util::sync::CancellationToken::new(), &|_| {}).await.expect("instalar motor oficial");
    eprintln!("motor instalado em {:.1}s: {}", t0.elapsed().as_secs_f32(), inst.executavel.display());
    assert!(inst.executavel.is_file());
    assert!(store.installed(&manifest, &cpu).is_some());

    // 2) núcleo + API com o motor oficial
    let motor = Motor::new(
        paths.clone(),
        MotorOptions {
            detection_override: Some(sem_placa()),
            tick: Duration::from_millis(200),
            launcher: None,
            load_timeout: Duration::from_secs(120),
            catalog_override: None,
            manifest_override: Some(manifest.clone()),
            auto_teste: false,
            extras_motor: None,
        },
    );
    motor.start_background();
    let id = motor.import_file(&modelo).unwrap();
    let base = subir_api(&motor).await;
    let c = cliente();

    // conversa sem streaming
    let t0 = Instant::now();
    let r = c
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"model": id, "temperature": 0, "max_tokens": 16, "messages": [{"role": "user", "content": "Responda apenas com a palavra: olá"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let v: Value = r.json().await.unwrap();
    let texto = v["choices"][0]["message"]["content"].as_str().unwrap_or("");
    eprintln!("resposta ({:.1}s): {texto:?}", t0.elapsed().as_secs_f32());
    assert!(!texto.is_empty());
    assert_eq!(motor.loaded_info().unwrap().backend, Backend::Cpu);

    // conversa com streaming
    let resp = c
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"model": id, "stream": true, "temperature": 0, "max_tokens": 24, "messages": [{"role": "user", "content": "Conte de 1 a 5."}]}))
        .send()
        .await
        .unwrap();
    let mut s = resp.bytes_stream();
    let mut corpo = String::new();
    while let Some(b) = s.next().await {
        corpo.push_str(&String::from_utf8_lossy(&b.unwrap()));
    }
    assert!(corpo.contains("data: [DONE]"), "{corpo}");
    assert!(corpo.matches("\"delta\"").count() >= 3);
    let st: Value = c.get(format!("{base}/motor/estado")).send().await.unwrap().json().await.unwrap();
    eprintln!("velocidade: {}", st["velocidade"]);
    assert!(st["velocidade"]["geracao_tps"].as_f64().unwrap_or(0.0) > 0.0);

    // embeddings num modelo de conversa: recusado com explicação
    let r = c.post(format!("{base}/v1/embeddings")).json(&json!({"model": id, "input": "oi"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);

    // Parar no meio de uma resposta longa
    let resp = c
        .post(format!("{base}/v1/chat/completions"))
        // ignore_eos: a resposta não pode acabar sozinha antes do Parar
        .json(&json!({"model": id, "stream": true, "max_tokens": 2000, "ignore_eos": true, "messages": [{"role": "user", "content": "Escreva uma história bem longa sobre um dragão."}]}))
        .send()
        .await
        .unwrap();
    let req_id = resp.headers()["x-ignis-requisicao"].to_str().unwrap().to_string();
    let leitura = tokio::spawn(async move {
        let mut s = resp.bytes_stream();
        let mut n = 0usize;
        let mut done = false;
        while let Some(b) = s.next().await {
            let t = String::from_utf8_lossy(&b.unwrap()).to_string();
            n += t.matches("\"delta\"").count();
            if t.contains("[DONE]") {
                done = true;
            }
        }
        (n, done)
    });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(!leitura.is_finished(), "a resposta terminou antes do Parar (o modelo parou sozinho?)");
    let t_parar = Instant::now();
    let r: Value = c.post(format!("{base}/motor/parar")).json(&json!({"id": req_id})).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["paradas"], 1, "conversas ativas: {}", motor.ativos());
    let (n, done) = tokio::time::timeout(Duration::from_secs(5), leitura).await.unwrap().unwrap();
    eprintln!("parou depois de {n} pedaços em {:?}", t_parar.elapsed());
    assert!(done && n < 2000);
    // o llama-server parou de gerar
    let interno = motor.loaded_info().unwrap().base_url;
    let parado = async {
        loop {
            let slots: Value = c.get(format!("{interno}/slots")).send().await.unwrap().json().await.unwrap_or(json!([]));
            if slots.as_array().map(|a| a.iter().all(|s| s["is_processing"] != json!(true))).unwrap_or(true) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), parado).await.expect("llama-server continuou gerando depois do Parar");

    // descarregar por tempo
    c.post(format!("{base}/motor/config")).json(&json!({"descarregar_apos_seg": 1})).send().await.unwrap();
    esperar("descarregar por tempo", Duration::from_secs(10), || motor.loaded_info().is_none()).await;

    // teste de velocidade com o motor real
    motor.run_benchmark();
    esperar("teste de velocidade", Duration::from_secs(90), || motor.bench().is_some() && !motor.bench_running()).await;
    let t = motor.bench().unwrap();
    eprintln!("teste: {:?}", t.resultados);
    assert!(t.resultados[0].ok, "{:?}", t.resultados[0].erro);
    assert!(t.resultados[0].geracao_tps.unwrap() > 0.0);

    motor.shutdown_all().await;
}
