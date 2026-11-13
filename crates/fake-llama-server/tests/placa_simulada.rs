//! Testes de ponta a ponta com a placa simulada (sem placa de vídeo):
//! escolha do motor, queda para o próximo, API, streaming, Parar,
//! descarregar por tempo, pausa e embeddings.

mod comum;

use std::time::{Duration, Instant};

use comum::*;
use futures_util::StreamExt;
use ignis_core::service::Fase;
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn teste_de_velocidade_escolhe_o_mais_rapido() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    m.start_background();
    let modelo = modelo_gguf(amb.dir.path(), "pequeno", false);
    m.import_file(&modelo).unwrap();
    m.run_benchmark();
    esperar("teste terminar", Duration::from_secs(60), || m.bench().is_some() && !m.bench_running()).await;
    assert_eq!(m.fase().0, Fase::Parado);
    let t = m.bench().unwrap();
    assert_eq!(t.resultados.len(), 3, "{:?}", t.resultados);
    assert!(t.resultados.iter().all(|r| r.ok), "{:?}", t.resultados);
    assert_eq!(t.ordem, vec!["sim-rocm", "sim-vulkan", "sim-cpu"]);
    assert_eq!(m.engine_order()[0].0.variante, "sim-rocm");
    assert!(avisos_texto(m).contains("mais rápido"));
    // o estado mostra o teste como atual
    let st = m.status_json();
    assert_eq!(st["teste"]["atual"], json!(true));
    assert_eq!(st["precisa_preparar"], json!(false));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn motor_que_falha_no_teste_fica_de_fora() {
    let amb = ambiente(Opcoes {
        motores: vec![
            ("sim-rocm", json!({"modo": "falha_ao_subir", "log_erro": "rocBLAS error: hipErrorNoBinaryForGpu"})),
            ("sim-vulkan", cfg(60.0)),
            ("sim-cpu", cfg(10.0)),
        ],
        ..Default::default()
    });
    let m = &amb.motor;
    let modelo = modelo_gguf(amb.dir.path(), "pequeno", false);
    m.import_file(&modelo).unwrap();
    m.run_benchmark();
    esperar("teste terminar", Duration::from_secs(60), || m.bench().is_some() && !m.bench_running()).await;
    let t = m.bench().unwrap();
    let rocm = t.resultados.iter().find(|r| r.variante == "sim-rocm").unwrap();
    assert!(!rocm.ok);
    assert!(rocm.erro.as_deref().unwrap().contains("suporte para a sua placa"), "{:?}", rocm.erro);
    assert_eq!(t.ordem, vec!["sim-vulkan", "sim-cpu"]);
    let info = m.ensure_loaded("arquivo/pequeno").await.unwrap();
    assert_eq!(info.variante, "sim-vulkan");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn falha_ao_subir_passa_para_o_proximo_e_avisa() {
    let amb = ambiente(Opcoes {
        motores: vec![
            ("sim-rocm", json!({"modo": "falha_ao_subir", "log_erro": "ggml_cuda_init: failed; hipErrorNoBinaryForGpu"})),
            ("sim-vulkan", cfg(60.0)),
            ("sim-cpu", cfg(10.0)),
        ],
        ..Default::default()
    });
    let m = &amb.motor;
    let modelo = modelo_gguf(amb.dir.path(), "pequeno", false);
    let id = m.import_file(&modelo).unwrap();
    // sem teste feito: ordem de prioridade (ROCm primeiro)
    assert_eq!(m.engine_order()[0].0.variante, "sim-rocm");
    let info = m.ensure_loaded(&id).await.unwrap();
    assert_eq!(info.variante, "sim-vulkan");
    let av = avisos_texto(m);
    assert!(av.contains("ROCm/HIP (AMD) não subiu"), "{av}");
    assert!(av.contains("usando Vulkan"), "{av}");
    // na próxima vez o ROCm vai para o fim da fila
    assert_eq!(m.engine_order().last().unwrap().0.variante, "sim-rocm");
    m.unload().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn motor_que_cai_depois_de_carregar_e_trocado_sozinho() {
    let amb = ambiente(Opcoes {
        motores: vec![
            (
                "sim-rocm",
                json!({"modo": "cai_depois", "cai_depois_ms": 800, "log_erro": "HIP error: an illegal memory access was encountered", "tps_geracao": 80.0}),
            ),
            ("sim-vulkan", cfg(60.0)),
            ("sim-cpu", cfg(10.0)),
        ],
        ..Default::default()
    });
    let m = &amb.motor;
    m.start_background();
    let modelo = modelo_gguf(amb.dir.path(), "pequeno", false);
    let id = m.import_file(&modelo).unwrap();
    let info = m.ensure_loaded(&id).await.unwrap();
    assert_eq!(info.variante, "sim-rocm");
    esperar("trocar para o Vulkan", Duration::from_secs(20), || m.loaded_info().map(|i| i.variante == "sim-vulkan").unwrap_or(false)).await;
    let av = avisos_texto(m);
    assert!(av.contains("parou de repente"), "{av}");
    assert!(av.contains("Passei para Vulkan"), "{av}");
    m.unload().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn motor_forcado_manualmente_vem_primeiro() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    let modelo = modelo_gguf(amb.dir.path(), "pequeno", false);
    let id = m.import_file(&modelo).unwrap();
    m.force_engine(Some("sim-cpu".into())).unwrap();
    assert_eq!(m.engine_order()[0].0.variante, "sim-cpu");
    assert_eq!(m.ensure_loaded(&id).await.unwrap().variante, "sim-cpu");
    assert!(m.force_engine(Some("nao-existe".into())).is_err());
    m.unload().await;
}

async fn ler_sse(resp: reqwest::Response, max_chunks: usize) -> (Vec<Value>, bool) {
    let mut s = resp.bytes_stream();
    let mut buf = String::new();
    let mut chunks = Vec::new();
    let mut done = false;
    while let Some(b) = s.next().await {
        buf.push_str(std::str::from_utf8(&b.unwrap()).unwrap());
        while let Some(pos) = buf.find("\n\n") {
            let ev: String = buf.drain(..pos + 2).collect();
            let data = ev.trim().strip_prefix("data: ").unwrap_or("").to_string();
            if data == "[DONE]" {
                done = true;
            } else if let Ok(v) = serde_json::from_str::<Value>(&data) {
                chunks.push(v);
            }
        }
        if done || chunks.len() >= max_chunks {
            break;
        }
    }
    (chunks, done)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn api_openai_com_streaming() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    m.start_background();
    let a = modelo_gguf(amb.dir.path(), "conversa", false);
    let id = m.import_file(&a).unwrap();
    let base = subir_api(m).await;
    let c = cliente();

    let models: Value = c.get(format!("{base}/v1/models")).send().await.unwrap().json().await.unwrap();
    assert_eq!(models["object"], "list");
    assert_eq!(models["data"][0]["id"], json!(id));

    // sem streaming
    let r: Value = c
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"model": id, "messages": [{"role": "user", "content": "oi"}], "max_tokens": 3}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["choices"][0]["message"]["role"], "assistant");

    // com streaming
    let resp = c
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"model": id, "stream": true, "messages": [{"role": "user", "content": "oi"}], "max_tokens": 5}))
        .send()
        .await
        .unwrap();
    assert!(resp.headers().get("x-ignis-requisicao").is_some());
    assert!(resp.headers()["content-type"].to_str().unwrap().starts_with("text/event-stream"));
    let (chunks, done) = ler_sse(resp, 100).await;
    assert!(done, "terminou com [DONE]");
    assert_eq!(chunks.iter().filter(|c| c["choices"][0]["delta"]["content"].is_string()).count(), 5);
    // a velocidade aparece no estado
    let st: Value = c.get(format!("{base}/motor/estado")).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["fase"], "pronto");
    assert_eq!(st["modelo"]["id"], json!(id));
    assert_eq!(st["motor"]["variante"], "sim-rocm");
    assert!(st["velocidade"]["geracao_tps"].as_f64().unwrap() > 0.0);

    // modelo que não existe
    let r = c.post(format!("{base}/v1/chat/completions")).json(&json!({"model": "nao-existe", "messages": []})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 404);
    let e: Value = r.json().await.unwrap();
    assert_eq!(e["error"]["code"], "model_not_found");

    // versão
    let v: Value = c.get(format!("{base}/motor/versao")).send().await.unwrap().json().await.unwrap();
    assert_eq!(v["api"], 1);
    assert_eq!(v["llama_cpp"]["versao"], "b1");
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pedido_grande_com_imagem_ou_audio_passa() {
    // Foto ou áudio em base64 passam fácil de 2 MB (o limite padrão do axum).
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    m.start_background();
    let a = modelo_gguf(amb.dir.path(), "conversa", false);
    let id = m.import_file(&a).unwrap();
    let base = subir_api(m).await;
    let foto = format!("data:image/jpeg;base64,{}", "A".repeat(12 * 1024 * 1024));
    let r = cliente()
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"model": id, "max_tokens": 3, "messages": [{"role": "user", "content": [
            {"type": "text", "text": "O que tem na foto?"},
            {"type": "image_url", "image_url": {"url": foto}}
        ]}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200, "pedido de 12 MB deve passar");
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parar_interrompe_a_resposta() {
    let amb = ambiente(Opcoes { motores: vec![("sim-vulkan", cfg(25.0)), ("sim-cpu", cfg(10.0))], ..Default::default() });
    let m = &amb.motor;
    let a = modelo_gguf(amb.dir.path(), "conversa", false);
    let id = m.import_file(&a).unwrap();
    let base = subir_api(m).await;
    let c = cliente();
    let resp = c
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"model": id, "stream": true, "messages": [{"role": "user", "content": "conte até mil"}], "max_tokens": 1000}))
        .send()
        .await
        .unwrap();
    let req_id = resp.headers()["x-ignis-requisicao"].to_str().unwrap().to_string();
    let t0 = Instant::now();
    let leitura = tokio::spawn(ler_sse(resp, 100_000));
    tokio::time::sleep(Duration::from_millis(500)).await;
    let r: Value = c.post(format!("{base}/motor/parar")).json(&json!({"id": req_id})).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["paradas"], 1);
    let (chunks, done) = tokio::time::timeout(Duration::from_secs(5), leitura).await.unwrap().unwrap();
    assert!(done, "fecha com [DONE]");
    assert!(chunks.len() < 100, "parou cedo: {}", chunks.len());
    assert!(t0.elapsed() < Duration::from_secs(5));
    // o motor parou de gerar
    tokio::time::sleep(Duration::from_millis(600)).await;
    let n1 = contador(&amb, "sim-vulkan");
    tokio::time::sleep(Duration::from_millis(800)).await;
    let n2 = contador(&amb, "sim-vulkan");
    assert!(n2 <= n1 + 1, "o motor continuou gerando: {n1} -> {n2}");
    assert_eq!(m.ativos(), 0);
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fechar_a_conexao_tambem_para() {
    let amb = ambiente(Opcoes { motores: vec![("sim-vulkan", cfg(25.0))], ..Default::default() });
    let m = &amb.motor;
    let a = modelo_gguf(amb.dir.path(), "conversa", false);
    let id = m.import_file(&a).unwrap();
    let base = subir_api(m).await;
    let c = cliente();
    let resp = c
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"model": id, "stream": true, "messages": [], "max_tokens": 1000}))
        .send()
        .await
        .unwrap();
    let _ = ler_sse(resp, 3).await; // lê 3 pedaços e larga a conexão
    tokio::time::sleep(Duration::from_millis(800)).await;
    let n1 = contador(&amb, "sim-vulkan");
    tokio::time::sleep(Duration::from_millis(800)).await;
    let n2 = contador(&amb, "sim-vulkan");
    assert!(n2 <= n1 + 1, "o motor continuou gerando: {n1} -> {n2}");
    esperar("conversa terminar", Duration::from_secs(5), || m.ativos() == 0).await;
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn descarrega_sozinho_depois_de_parado() {
    let amb = ambiente(Opcoes { config: json!({"descarregar_apos_seg": 1}), tick_ms: 100, ..Default::default() });
    let m = &amb.motor;
    m.start_background();
    let a = modelo_gguf(amb.dir.path(), "conversa", false);
    let id = m.import_file(&a).unwrap();
    m.ensure_loaded(&id).await.unwrap();
    assert!(m.loaded_info().is_some());
    esperar("descarregar por tempo", Duration::from_secs(10), || m.loaded_info().is_none()).await;
    esperar("aviso de descarregado", Duration::from_secs(2), || avisos_texto(m).contains("descarregado depois de 1 s parado")).await;
    assert_eq!(m.fase().0, Fase::Parado);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pausar_e_retomar() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    let a = modelo_gguf(amb.dir.path(), "conversa", false);
    let id = m.import_file(&a).unwrap();
    let base = subir_api(m).await;
    let c = cliente();
    m.ensure_loaded(&id).await.unwrap();
    c.post(format!("{base}/motor/pausar")).send().await.unwrap();
    assert!(m.loaded_info().is_none(), "pausar libera a placa");
    let r = c.post(format!("{base}/v1/chat/completions")).json(&json!({"model": id, "messages": []})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 503);
    let e: Value = r.json().await.unwrap();
    assert_eq!(e["error"]["code"], "ignis_pausado");
    let st: Value = c.get(format!("{base}/motor/estado")).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["fase"], "pausado");
    c.post(format!("{base}/motor/retomar")).send().await.unwrap();
    let r =
        c.post(format!("{base}/v1/chat/completions")).json(&json!({"model": id, "messages": [], "max_tokens": 2})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn um_modelo_por_vez_e_embeddings() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    let a = m.import_file(&modelo_gguf(amb.dir.path(), "conversa-a", false)).unwrap();
    let b = m.import_file(&modelo_gguf(amb.dir.path(), "conversa-b", false)).unwrap();
    let e = m.import_file(&modelo_gguf(amb.dir.path(), "vetores", true)).unwrap();
    let base = subir_api(m).await;
    let c = cliente();
    for id in [&a, &b] {
        let r = c
            .post(format!("{base}/v1/chat/completions"))
            .json(&json!({"model": id, "messages": [], "max_tokens": 1}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 200);
        assert_eq!(m.loaded_info().unwrap().modelo_id, *id, "troca para o modelo pedido");
    }
    let models: Value = c.get(format!("{base}/v1/models")).send().await.unwrap().json().await.unwrap();
    let carregados: Vec<&Value> = models["data"].as_array().unwrap().iter().filter(|x| x["ignis"]["carregado"] == json!(true)).collect();
    assert_eq!(carregados.len(), 1);
    // embeddings
    let r: Value = c
        .post(format!("{base}/v1/embeddings"))
        .json(&json!({"model": e, "input": ["olá", "mundo"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["data"].as_array().unwrap().len(), 2);
    // embeddings sem dizer o modelo usa o de embeddings
    let r = c.post(format!("{base}/v1/embeddings")).json(&json!({"input": "olá"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    // modelo de conversa não gera embeddings
    let r = c.post(format!("{base}/v1/embeddings")).json(&json!({"model": a, "input": "olá"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);
    // modelo de embeddings não conversa
    let r = c.post(format!("{base}/v1/chat/completions")).json(&json!({"model": e, "messages": []})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn api_recusa_sites_e_outros_hosts() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    let base = subir_api(m).await;
    let c = cliente();
    let r = c.get(format!("{base}/motor/estado")).header("Origin", "https://site-malicioso.com").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    let r = c.post(format!("{base}/motor/pausar")).header("Origin", "https://site-malicioso.com").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    assert!(!m.is_paused());
    let r = c.get(format!("{base}/motor/estado")).header("Host", "exemplo.com").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403, "proteção contra DNS rebinding");
    let r = c.get(format!("{base}/motor/estado")).header("Origin", "http://localhost:5173").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert_eq!(r.headers()["access-control-allow-origin"], "http://localhost:5173");
    // App local com o cliente oficial da OpenAI (manda cabeçalhos x-stainless-*)
    let r = c
        .request(reqwest::Method::OPTIONS, format!("{base}/v1/chat/completions"))
        .header("Origin", "http://localhost:5173")
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "content-type, authorization, x-stainless-os, x-stainless-lang")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 204);
    let permitidos = r.headers()["access-control-allow-headers"].to_str().unwrap().to_lowercase();
    assert!(permitidos.contains("x-stainless-os") && permitidos.contains("content-type"), "{permitidos}");
    // Origem "null" (iframe isolado de um site) continua recusada
    let r = c.get(format!("{base}/motor/estado")).header("Origin", "null").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn janela_fala_com_o_nucleo_sem_rede() {
    let amb = ambiente(Opcoes::default());
    let (status, v) = ignis_core::api::call(amb.motor.clone(), "GET", "/motor/estado", None).await;
    assert_eq!(status, 200);
    assert_eq!(v["app"]["nome"], "Ignis");
    let (status, _) = ignis_core::api::call(amb.motor.clone(), "POST", "/motor/config", Some(json!({"descarregar_apos_min": 5}))).await;
    assert_eq!(status, 200);
    assert_eq!(amb.motor.settings().descarregar_apos_seg, 300);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn troca_de_porta_sem_reiniciar() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    let base = subir_api(m).await;
    let c = cliente();
    let nova = free_port();
    let r = c.post(format!("{base}/motor/config")).json(&json!({"porta": nova})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let novo = format!("http://127.0.0.1:{nova}");
    let t0 = Instant::now();
    loop {
        if c.get(format!("{novo}/motor/versao")).send().await.map(|r| r.status().is_success()).unwrap_or(false) {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "porta nova não subiu");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(m.api_status(), Ok(nova));
    m.shutdown_all().await;
}

/// Modelo de teste "grande": o mesmo GGUF com bytes a mais no fim (como os dados dos tensores).
fn modelo_grande(dir: &std::path::Path, nome: &str, vezes: usize) -> std::path::PathBuf {
    let p = modelo_gguf(dir, nome, false);
    let tam = std::fs::metadata(&p).unwrap().len() as usize;
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    std::io::Write::write_all(&mut f, &vec![0u8; tam * vezes]).unwrap();
    p
}

/// Relatório do Valkode (RX 580): no teste com o modelo pequeno o processador
/// ganhou, mas para um modelo grande que cabe na placa a placa é bem mais rápida.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn modelo_grande_que_cabe_vai_para_a_placa_e_o_ignis_aprende_com_o_uso() {
    let amb = ambiente(Opcoes { motores: vec![("sim-vulkan", cfg(37.5)), ("sim-cpu", cfg(68.2))], ..Default::default() });
    let m = &amb.motor;
    m.start_background();
    let pequeno = m.import_file(&modelo_gguf(amb.dir.path(), "pequeno", false)).unwrap();
    m.run_benchmark();
    esperar("teste terminar", Duration::from_secs(60), || m.bench().is_some() && !m.bench_running()).await;
    assert_eq!(m.bench().unwrap().ordem, vec!["sim-cpu", "sim-vulkan"], "no teste pequeno o processador ganha");

    // modelo grande que cabe na placa: vai para a placa
    let grande = m.import_file(&modelo_grande(amb.dir.path(), "grande", 60)).unwrap();
    let info = m.ensure_loaded(&grande).await.unwrap();
    assert_eq!(info.variante, "sim-vulkan");
    assert_eq!(info.motivo, "o modelo cabe na placa");
    let st = m.status_json();
    assert_eq!(st["modelo"]["motivo_motor"], "o modelo cabe na placa");
    let ult = &st["ultimo_carregamento"];
    assert_eq!(ult["ok"], json!(true));
    assert_eq!(ult["tentativas"][0]["variante"], "sim-vulkan");
    assert!(ult["segundos_total"].as_f64().unwrap() >= 0.0);

    // o modelo pequeno continua seguindo o teste
    let info = m.ensure_loaded(&pequeno).await.unwrap();
    assert_eq!((info.variante.as_str(), info.motivo.as_str()), ("sim-cpu", "foi o mais rápido no teste de velocidade"));

    // uso real: a velocidade de cada motor com o modelo grande fica anotada
    let base = subir_api(m).await;
    let c = cliente();
    let conversar = || async {
        let r: Value = c
            .post(format!("{base}/v1/chat/completions"))
            .json(&json!({"model": grande, "max_tokens": 20, "messages": [{"role": "user", "content": "oi"}]}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(r["choices"][0]["message"]["role"], "assistant");
    };
    conversar().await;
    m.force_engine(Some("sim-cpu".into())).unwrap();
    conversar().await;
    m.force_engine(None).unwrap();
    let med = m.medidas_do_modelo(&grande);
    assert_eq!(med.get("sim-vulkan"), Some(&37.5), "{med:?}");
    assert_eq!(med.get("sim-cpu"), Some(&68.2), "{med:?}");
    // com as duas medidas, vale o mais rápido medido (aqui, na placa simulada, o processador)
    m.unload().await;
    let info = m.ensure_loaded(&grande).await.unwrap();
    assert_eq!((info.variante.as_str(), info.motivo.as_str()), ("sim-cpu", "foi o mais rápido medido com este modelo"));
    let lista: Value = c.get(format!("{base}/motor/modelos")).send().await.unwrap().json().await.unwrap();
    let g = lista["instalados"].as_array().unwrap().iter().find(|x| x["id"] == json!(grande)).unwrap().clone();
    assert_eq!(g["velocidades"][0]["variante"], "sim-cpu");
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn testar_com_o_proprio_modelo() {
    let amb = ambiente(Opcoes { motores: vec![("sim-vulkan", cfg(16.1)), ("sim-cpu", cfg(8.1))], ..Default::default() });
    let m = &amb.motor;
    m.start_background();
    let grande = m.import_file(&modelo_grande(amb.dir.path(), "grande", 60)).unwrap();
    let base = subir_api(m).await;
    let c = cliente();
    let r = c.post(format!("{base}/motor/testar")).json(&json!({"modelo": grande})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    esperar("teste com o modelo", Duration::from_secs(60), || m.medidas_do_modelo(&grande).len() == 2 && !m.bench_running()).await;
    let med = m.medidas_do_modelo(&grande);
    assert_eq!((med["sim-vulkan"], med["sim-cpu"]), (16.1, 8.1));
    assert!(avisos_texto(m).contains("O Ignis vai usar ele para este modelo"));
    let info = m.ensure_loaded(&grande).await.unwrap();
    assert_eq!((info.variante.as_str(), info.motivo.as_str()), ("sim-vulkan", "foi o mais rápido medido com este modelo"));
    // modelo de embeddings não tem teste de velocidade
    let emb = m.import_file(&modelo_gguf(amb.dir.path(), "emb", true)).unwrap();
    let r = c.post(format!("{base}/motor/testar")).json(&json!({"modelo": emb})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);
    m.shutdown_all().await;
}
