//! Pedidos da Fornalha e da Erethea Engine: dois modelos juntos (conversa + embeddings),
//! reserva da placa, aceleradores e memória no estado, e uso comercial de cada modelo.

mod comum;

use std::time::Duration;

use comum::*;
use ignis_core::gguf::{write_test_gguf, Value as GValue};
use serde_json::{json, Value};

/// GGUF com a licença gravada nos metadados (`general.license`).
fn modelo_com_licenca(dir: &std::path::Path, nome: &str, licenca: &str) -> std::path::PathBuf {
    let p = dir.join(format!("{nome}.gguf"));
    write_test_gguf(
        &p,
        &[
            ("general.architecture", GValue::Str("qwen3".into())),
            ("general.name", GValue::Str(nome.into())),
            ("general.license", GValue::Str(licenca.into())),
            ("qwen3.block_count", GValue::Int(28)),
            ("qwen3.context_length", GValue::Int(40960)),
            ("qwen3.embedding_length", GValue::Int(1024)),
            ("qwen3.attention.head_count", GValue::Int(16)),
            ("qwen3.attention.head_count_kv", GValue::Int(8)),
        ],
    )
    .unwrap();
    p
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cerebro_e_embeddings_ficam_carregados_juntos() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    let a = m.import_file(&modelo_gguf(amb.dir.path(), "cerebro", false)).unwrap();
    let e = m.import_file(&modelo_gguf(amb.dir.path(), "vetores", true)).unwrap();
    let base = subir_api(m).await;
    let c = cliente();
    let r = c.post(format!("{base}/v1/chat/completions")).json(&json!({"model": a, "messages": [], "max_tokens": 1})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let cerebro = m.loaded_info_de(false).unwrap();
    let r = c.post(format!("{base}/v1/embeddings")).json(&json!({"model": e, "input": "olá"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    // o cérebro continuou carregado (mesmo processo)
    let depois = m.loaded_info_de(false).expect("cérebro continua carregado");
    assert_eq!(depois.base_url, cerebro.base_url);
    assert_eq!(depois.desde, cerebro.desde);
    assert_eq!(m.loaded_info_de(true).unwrap().modelo_id, e);
    // conversar de novo não recarrega nada
    let r = c.post(format!("{base}/v1/chat/completions")).json(&json!({"model": a, "messages": [], "max_tokens": 1})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert_eq!(m.loaded_info_de(true).unwrap().modelo_id, e, "embeddings continua carregado");
    assert_eq!(m.loaded_info_de(false).unwrap().base_url, cerebro.base_url);

    let models: Value = c.get(format!("{base}/v1/models")).send().await.unwrap().json().await.unwrap();
    let carregados: Vec<&Value> = models["data"].as_array().unwrap().iter().filter(|x| x["ignis"]["carregado"] == json!(true)).collect();
    assert_eq!(carregados.len(), 2, "{models}");
    let st: Value = c.get(format!("{base}/motor/estado")).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["modelos_carregados"].as_array().unwrap().len(), 2);
    assert_eq!(st["modelo"]["id"], json!(a), "o \"modelo\" do estado continua sendo o de conversa");

    // descarregar libera os dois
    let r: Value = c.post(format!("{base}/motor/modelos/descarregar")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["ok"], json!(true));
    assert!(m.carregados().is_empty());
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn trocar_o_cerebro_nao_tira_o_embeddings() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    let a = m.import_file(&modelo_gguf(amb.dir.path(), "cerebro-a", false)).unwrap();
    let b = m.import_file(&modelo_gguf(amb.dir.path(), "cerebro-b", false)).unwrap();
    let e = m.import_file(&modelo_gguf(amb.dir.path(), "vetores", true)).unwrap();
    m.ensure_loaded(&e).await.unwrap();
    let emb = m.loaded_info_de(true).unwrap();
    m.ensure_loaded(&a).await.unwrap();
    m.ensure_loaded(&b).await.unwrap();
    assert_eq!(m.loaded_info_de(false).unwrap().modelo_id, b);
    assert_eq!(m.loaded_info_de(true).unwrap().base_url, emb.base_url, "embeddings não foi recarregado");
    assert_eq!(m.carregados().len(), 2);
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn descarrega_sozinho_cada_um() {
    let amb = ambiente(Opcoes { config: json!({"descarregar_apos_seg": 1}), tick_ms: 100, ..Default::default() });
    let m = &amb.motor;
    m.start_background();
    let a = m.import_file(&modelo_gguf(amb.dir.path(), "cerebro", false)).unwrap();
    let e = m.import_file(&modelo_gguf(amb.dir.path(), "vetores", true)).unwrap();
    m.ensure_loaded(&a).await.unwrap();
    m.ensure_loaded(&e).await.unwrap();
    esperar("descarregar os dois por tempo", Duration::from_secs(10), || m.carregados().is_empty()).await;
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reservar_a_placa_descarrega_e_recusa_ate_liberar() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    let a = m.import_file(&modelo_gguf(amb.dir.path(), "cerebro", false)).unwrap();
    let e = m.import_file(&modelo_gguf(amb.dir.path(), "vetores", true)).unwrap();
    let base = subir_api(m).await;
    let c = cliente();
    m.ensure_loaded(&a).await.unwrap();
    m.ensure_loaded(&e).await.unwrap();

    // pedido do jeito da Fornalha (placa simulada: 7.800 MB livres, menos que 12.000)
    let r = c.post(format!("{base}/motor/reservar")).json(&json!({"mb": 12000, "minutos": 15})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["descarregados"], json!(2), "conversa e embeddings: {v}");
    assert_eq!(v["suficiente"], json!(false));
    assert_eq!(v["vram_livre_mb"], json!(7800));
    assert_eq!(v["reserva"]["ativa"], json!(true));
    assert_eq!(v["reserva"]["mb"], json!(12000));
    let resta = v["reserva"]["restante_seg"].as_u64().unwrap();
    assert!((899..=900).contains(&resta), "{resta}");
    assert!(m.carregados().is_empty(), "os dois modelos saíram da placa");

    // nada carrega durante a reserva: conversa, embeddings e carregar à mão
    for (rota, corpo) in [
        ("/v1/chat/completions", json!({"model": a, "messages": [], "max_tokens": 1})),
        ("/v1/embeddings", json!({"model": e, "input": "olá"})),
        ("/motor/modelos/carregar", json!({"id": a})),
    ] {
        let r = c.post(format!("{base}{rota}")).json(&corpo).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 503, "{rota}");
        let erro: Value = r.json().await.unwrap();
        assert_eq!(erro["error"]["code"], "placa_reservada", "{rota}: {erro}");
        assert!(erro["error"]["message"].as_str().unwrap().contains("15 min"), "{erro}");
    }
    assert!(m.carregados().is_empty());

    let st: Value = c.get(format!("{base}/motor/estado")).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["reserva"]["ativa"], json!(true));
    let g: Value = c.get(format!("{base}/motor/reservar")).send().await.unwrap().json().await.unwrap();
    assert_eq!(g["ativa"], json!(true));

    // valores inválidos
    let r = c.post(format!("{base}/motor/reservar")).json(&json!({"mb": "muito", "minutos": 5})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);
    let r = c.post(format!("{base}/motor/reservar")).json(&json!({"minutos": 100000})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);

    // liberar antes do prazo
    let v: Value = c.post(format!("{base}/motor/reservar/liberar")).send().await.unwrap().json().await.unwrap();
    assert_eq!(v["liberada"], json!(true));
    assert_eq!(v["reserva"]["ativa"], json!(false));
    let r = c.post(format!("{base}/v1/chat/completions")).json(&json!({"model": a, "messages": [], "max_tokens": 1})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);

    // "minutos": 0 também libera
    c.post(format!("{base}/motor/reservar")).json(&json!({"minutos": 1})).send().await.unwrap();
    assert!(m.reserva_ativa().is_some());
    let v: Value = c.post(format!("{base}/motor/reservar")).json(&json!({"minutos": 0})).send().await.unwrap().json().await.unwrap();
    assert_eq!(v["liberada"], json!(true));
    assert!(m.reserva_ativa().is_none());
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn estado_lista_aceleradores_e_memoria_da_placa() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    let base = subir_api(m).await;
    let c = cliente();
    let st: Value = c.get(format!("{base}/motor/estado")).send().await.unwrap().json().await.unwrap();
    let ac = &st["aceleradores"];
    // placa simulada: AMD RX 6600 com ROCm e Vulkan
    assert_eq!(ac["rocm"]["existe"], json!(true), "{ac}");
    assert_eq!(ac["rocm"]["disponivel"], json!(true));
    assert_eq!(ac["vulkan"]["disponivel"], json!(true));
    assert_eq!(ac["cuda"]["existe"], json!(false));
    assert_eq!(ac["cuda"]["disponivel"], json!(false));
    assert_eq!(ac["processador"]["disponivel"], json!(true));
    assert_eq!(ac["rocm"]["motor_instalado"], json!(true));
    assert_eq!(ac["em_uso"], Value::Null);
    assert_eq!(st["vram"]["total_mb"], json!(8176));
    assert_eq!(st["vram"]["livre_mb"], json!(7800));
    assert!(st["vram"]["placa"].as_str().unwrap().contains("RX 6600"));
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uso_comercial_em_cada_modelo() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    let livre = m.import_file(&modelo_com_licenca(amb.dir.path(), "livre", "apache-2.0")).unwrap();
    let pesquisa = m.import_file(&modelo_com_licenca(amb.dir.path(), "pesquisa", "cc-by-nc-4.0")).unwrap();
    let meta = m.import_file(&modelo_com_licenca(amb.dir.path(), "meta", "llama3.1")).unwrap();
    let sem = m.import_file(&modelo_gguf(amb.dir.path(), "sem-licenca", false)).unwrap();
    let base = subir_api(m).await;
    let c = cliente();
    let models: Value = c.get(format!("{base}/v1/models")).send().await.unwrap().json().await.unwrap();
    let uso = |id: &str| models["data"].as_array().unwrap().iter().find(|x| x["id"] == json!(id)).unwrap()["ignis"].clone();
    assert_eq!(uso(&livre)["uso_comercial"], json!("sim"));
    assert_eq!(uso(&livre)["licenca"], json!("apache-2.0"));
    assert_eq!(uso(&pesquisa)["uso_comercial"], json!("nao"));
    assert_eq!(uso(&meta)["uso_comercial"], json!("condicional"));
    assert_eq!(uso(&sem)["uso_comercial"], json!("condicional"));
    assert!(uso(&sem)["uso_comercial_nota"].as_str().unwrap().contains("não informada"));

    let lista: Value = c.get(format!("{base}/motor/modelos")).send().await.unwrap().json().await.unwrap();
    let inst = |id: &str| lista["instalados"].as_array().unwrap().iter().find(|x| x["id"] == json!(id)).unwrap().clone();
    assert_eq!(inst(&pesquisa)["uso_comercial"], json!("nao"));
    // o catálogo (de teste) também traz o campo
    for c in lista["catalogo"].as_array().unwrap() {
        assert!(["sim", "nao", "condicional"].contains(&c["uso_comercial"].as_str().unwrap()), "{c}");
    }
    m.shutdown_all().await;
}
