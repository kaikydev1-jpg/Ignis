//! `ignis-cli` — diagnóstico e modo sem janela.
//!
//! ignis-cli detectar        mostra a placa, os motores candidatos e a impressão digital (JSON)
//! ignis-cli servir          roda o motor e a API sem janela (Ctrl+C para sair)
//! ignis-cli gguf ARQUIVO    mostra os metadados de um modelo GGUF
//! ignis-cli ollama          lista os modelos GGUF encontrados na pasta do Ollama
//! ignis-cli versao          versões do Ignis e do llama.cpp fixado

use std::path::Path;

use ignis_core::engine::{build_candidates, EngineManifest};
use ignis_core::paths::Paths;
use ignis_core::service::{Motor, MotorOptions};
use serde_json::json;

const AJUDA: &str = "Uso: ignis-cli <detectar|servir|gguf ARQUIVO|ollama|versao>";

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    match cmd {
        "detectar" => {
            let m = EngineManifest::embedded();
            let det = ignis_core::gpu::detect(&m.libs_to_probe(ignis_core::engine::manifest::current_os())).await;
            let cands = build_candidates(&det, &m, None);
            let out = json!({
                "impressao_digital": det.fingerprint(),
                "placa_principal": det.principal(),
                "deteccao": det,
                "candidatos": cands,
            });
            println!("{}", serde_json::to_string_pretty(&out).unwrap());
        }
        "servir" => {
            tracing_subscriber::fmt().with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into())).init();
            let motor = Motor::new(Paths::from_env(), MotorOptions { launcher: None, ..Default::default() });
            motor.start_background();
            let m2 = motor.clone();
            let api = tokio::spawn(async move { ignis_core::api::serve(m2).await });
            eprintln!("Ignis sem janela. API em http://127.0.0.1:{}/v1 — Ctrl+C para sair.", motor.settings().porta);
            // Ctrl+C, ou versão nova pronta para instalar
            let mut saida = motor.pedido_de_saida();
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = saida.wait_for(|c| c.is_some()) => {}
            }
            let comando = saida.borrow().clone();
            motor.shutdown_all().await;
            let _ = api.await;
            if let Some(c) = comando {
                eprintln!("Instalando a versão nova do Ignis…");
                if let Err(e) = ignis_core::atualizador::rodar_separado(&c) {
                    eprintln!("Não consegui iniciar o instalador: {e}");
                }
            }
        }
        "gguf" => {
            let Some(p) = args.get(1) else {
                eprintln!("{AJUDA}");
                std::process::exit(2);
            };
            match ignis_core::gguf::ModelInfo::read(Path::new(p)) {
                Ok(i) => {
                    let kv = i.kv_bytes_por_token();
                    println!("{}", serde_json::to_string_pretty(&json!({"info": i, "kv_bytes_por_token": kv})).unwrap());
                }
                Err(e) => {
                    eprintln!("erro: {e}");
                    std::process::exit(1);
                }
            }
        }
        "ollama" => {
            let lista = ignis_core::ollama::scan_all(&[]);
            println!("{}", serde_json::to_string_pretty(&lista).unwrap());
        }
        "versao" | "--version" | "-V" => {
            let m = EngineManifest::embedded();
            println!(
                "Ignis {} (API {}) — llama.cpp {}",
                ignis_core::consts::APP_VERSION,
                ignis_core::consts::API_VERSION,
                m.llama_cpp.versao
            );
        }
        _ => {
            eprintln!("{AJUDA}");
            std::process::exit(2);
        }
    }
}
