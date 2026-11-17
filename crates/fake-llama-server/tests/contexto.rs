//! Pedido do Valkode (15/11/2026): cache de contexto em 8 bits, atenção rápida,
//! contexto automático e o contexto realmente carregado informado.

mod comum;

use comum::*;
use serde_json::{json, Value};

fn com(mut base: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

/// Argumentos que o Ignis mandou ao motor carregado (o simulado devolve em /props).
async fn args_do_motor(m: &ignis_core::service::Motor) -> Vec<String> {
    let base = m.loaded_info().unwrap().base_url;
    let p: Value = cliente().get(format!("{base}/props")).send().await.unwrap().json().await.unwrap();
    p["args"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

fn tem(args: &[String], par: [&str; 2]) -> bool {
    args.windows(2).any(|w| w == par)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn informa_o_contexto_carregado_e_troca_para_8_bits() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    let id = m.import_file(&modelo_gguf(amb.dir.path(), "conversa", false)).unwrap();
    let base = subir_api(m).await;
    let c = cliente();

    // a) padrão: automático, cache de 16 bits, atenção rápida ligada pelo llama.cpp
    let r: Value = c.post(format!("{base}/motor/carregar")).json(&json!({"modelo": id})).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["cache_kv"], "f16", "{r}");
    assert_eq!(r["atencao_rapida"], json!(true));
    // placa simulada com 7,8 GB livres e modelo pequeno: o automático vai até o teto de 32.768
    assert_eq!(r["contexto_carregado"], json!(32768), "{r}");
    assert_eq!(r["contexto_max"], json!(40960));
    let a = args_do_motor(m).await;
    assert!(tem(&a, ["-c", "32768"]) && !a.iter().any(|x| x == "-ctk" || x == "-fa"), "{a:?}");
    let st: Value = c.get(format!("{base}/motor/estado")).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["modelo"]["contexto_carregado"], json!(32768));
    assert_eq!(st["modelo"]["cache_kv"], "f16");
    assert_eq!(st["modelo"]["atencao_rapida"], json!(true));
    let models: Value = c.get(format!("{base}/v1/models")).send().await.unwrap().json().await.unwrap();
    let item = models["data"].as_array().unwrap().iter().find(|x| x["id"] == json!(id)).unwrap().clone();
    assert_eq!(item["ignis"]["contexto_carregado"], json!(32768), "{item}");
    assert_eq!(item["ignis"]["contexto_max"], json!(40960));
    assert_eq!(item["ignis"]["cache_kv"], "f16");
    assert_eq!(item["ignis"]["atencao_rapida"], json!(true));

    // b) pede 8 bits e 16.384: recarrega com -ctk/-ctv q8_0 e informa
    let r: Value = c
        .post(format!("{base}/motor/carregar"))
        .json(&json!({"modelo": id, "contexto": 16384, "cache_kv": "q8_0"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["cache_kv"], "q8_0", "{r}");
    assert_eq!(r["atencao_rapida"], json!(true));
    assert_eq!(r["contexto_carregado"], json!(16384));
    let a = args_do_motor(m).await;
    assert!(tem(&a, ["-ctk", "q8_0"]) && tem(&a, ["-ctv", "q8_0"]) && tem(&a, ["-c", "16384"]), "{a:?}");

    // o pedido fica guardado para o modelo: descarregar e conversar recarrega igual
    m.unload().await;
    let r = c
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"model": id, "messages": [{"role": "user", "content": "oi"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let info = m.loaded_info().unwrap();
    assert_eq!((info.cache_kv.nome(), info.contexto_carregado), ("q8_0", 16384));

    // c) pedido maior do que o modelo aguenta: carrega o maior possível, sem erro
    let r: Value = c
        .post(format!("{base}/motor/carregar"))
        .json(&json!({"modelo": id, "contexto": 1_000_000}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["contexto_carregado"], json!(40960), "{r}");
    assert_eq!(r["contexto_possivel"], json!(40960));

    // voltar ao geral (automático, cache das configurações)
    let r: Value = c
        .post(format!("{base}/motor/carregar"))
        .json(&json!({"modelo": id, "contexto": null, "cache_kv": "padrao"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!((r["cache_kv"].as_str(), r["contexto_carregado"].as_u64()), (Some("f16"), Some(32768)), "{r}");
    // pedido errado: erro claro
    let r = c.post(format!("{base}/motor/carregar")).json(&json!({"modelo": id, "cache_kv": "q4"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);
    m.unload().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn contexto_real_menor_que_o_pedido_e_o_que_vale() {
    // o servidor abre menos do que o pedido (ex.: limite do motor): o Ignis informa o que abriu
    let amb = ambiente(Opcoes { motores: vec![("sim-vulkan", com(cfg(60.0), json!({"n_ctx_max": 12288})))], ..Default::default() });
    let m = &amb.motor;
    let id = m.import_file(&modelo_gguf(amb.dir.path(), "conversa", false)).unwrap();
    let info = m.ensure_loaded(&id).await.unwrap();
    assert_eq!(info.plano.contexto, 32768);
    assert_eq!(info.contexto_carregado, 12288);
    m.unload().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn placa_que_nao_aceita_8_bits_volta_para_16_e_avisa() {
    // d) cache de 8 bits recusado: sobe com 16 bits no mesmo motor, sem trocar de motor
    let amb = ambiente(Opcoes {
        motores: vec![("sim-vulkan", com(cfg(60.0), json!({"recusa_q8": true}))), ("sim-cpu", cfg(10.0))],
        config: json!({"cache_kv": "q8_0"}),
        ..Default::default()
    });
    let m = &amb.motor;
    let id = m.import_file(&modelo_gguf(amb.dir.path(), "conversa", false)).unwrap();
    let info = m.ensure_loaded(&id).await.unwrap();
    assert_eq!(info.variante, "sim-vulkan");
    assert_eq!(info.cache_kv.nome(), "f16");
    assert_eq!(info.pedido.cache.nome(), "q8_0");
    let av = avisos_texto(m);
    assert!(av.contains("não aceitou o cache de 8 bits"), "{av}");
    assert!(!av.contains("não subiu"), "não troca de motor por causa do cache: {av}");
    let a = args_do_motor(m).await;
    assert!(!a.iter().any(|x| x == "-ctk"), "{a:?}");
    // recarregar não tenta de novo o que já falhou nesta sessão
    m.unload().await;
    let info = m.ensure_loaded(&id).await.unwrap();
    assert_eq!(info.cache_kv.nome(), "f16");
    m.unload().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn atencao_rapida_que_quebra_e_desligada_e_avisada() {
    let amb = ambiente(Opcoes {
        motores: vec![("sim-vulkan", com(cfg(60.0), json!({"recusa_fa": true}))), ("sim-cpu", cfg(10.0))],
        ..Default::default()
    });
    let m = &amb.motor;
    let id = m.import_file(&modelo_gguf(amb.dir.path(), "conversa", false)).unwrap();
    let info = m.ensure_loaded(&id).await.unwrap();
    assert_eq!(info.variante, "sim-vulkan");
    assert_eq!(info.atencao_rapida, Some(false));
    assert!(tem(&args_do_motor(m).await, ["-fa", "off"]));
    let av = avisos_texto(m);
    assert!(av.contains("atenção rápida falhou"), "{av}");
    assert!(av.contains("Atenção rápida desligada"), "{av}");
    m.unload().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn atencao_rapida_sem_suporte_fica_desligada_pelo_llama_cpp() {
    // o automático do llama.cpp desliga sozinho; o Ignis só informa e avisa uma vez
    let amb = ambiente(Opcoes { motores: vec![("sim-vulkan", com(cfg(60.0), json!({"fa_suportada": false})))], ..Default::default() });
    let m = &amb.motor;
    let id = m.import_file(&modelo_gguf(amb.dir.path(), "conversa", false)).unwrap();
    let info = m.ensure_loaded(&id).await.unwrap();
    assert_eq!(info.atencao_rapida, Some(false));
    assert!(!args_do_motor(m).await.iter().any(|x| x == "-fa"), "não força nada");
    m.unload().await;
    m.ensure_loaded(&id).await.unwrap();
    assert_eq!(avisos_texto(m).matches("Atenção rápida desligada").count(), 1);
    m.unload().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn configuracao_geral_de_contexto_e_cache() {
    let amb = ambiente(Opcoes { motores: vec![("sim-vulkan", cfg(60.0))], ..Default::default() });
    let m = &amb.motor;
    let id = m.import_file(&modelo_gguf(amb.dir.path(), "conversa", false)).unwrap();
    let base = subir_api(m).await;
    let c = cliente();
    let conf: Value = c.get(format!("{base}/motor/config")).send().await.unwrap().json().await.unwrap();
    assert_eq!((conf["contexto_automatico"].as_bool(), conf["cache_kv"].as_str()), (Some(true), Some("f16")), "{conf}");
    // contexto à mão e cache de 8 bits para todos os modelos
    c.post(format!("{base}/motor/config"))
        .json(&json!({"contexto_automatico": false, "contexto_preferido": 12288, "cache_kv": "q8_0"}))
        .send()
        .await
        .unwrap();
    let info = m.ensure_loaded(&id).await.unwrap();
    assert_eq!((info.contexto_carregado, info.cache_kv.nome()), (12288, "q8_0"));
    // mudar a configuração geral vale no próximo carregamento (o pedido mudou)
    c.post(format!("{base}/motor/config")).json(&json!({"contexto_automatico": true, "cache_kv": "f16"})).send().await.unwrap();
    let info = m.ensure_loaded(&id).await.unwrap();
    assert_eq!((info.contexto_carregado, info.cache_kv.nome()), (32768, "f16"));
    m.unload().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn barra_da_memoria_mostra_quanto_usa_da_placa() {
    let amb = ambiente(Opcoes { motores: vec![("sim-vulkan", cfg(60.0))], ..Default::default() });
    let m = &amb.motor;
    m.detect_now().await;
    let id = m.import_file(&modelo_gguf(amb.dir.path(), "conversa", false)).unwrap();
    let base = subir_api(m).await;
    let c = cliente();
    let r: Value = c.get(format!("{base}/motor/modelos/memoria?id={id}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["onde"], "placa", "{r}");
    assert_eq!(r["memoria_total_mb"], json!(8176));
    let pontos = r["pontos"].as_array().unwrap();
    assert_eq!(pontos.first().unwrap()["contexto"], json!(2048));
    assert_eq!(pontos.last().unwrap()["contexto"], json!(40960), "vai até o máximo do modelo");
    for p in pontos {
        let (f16, q8) = (&p["f16"], &p["q8_0"]);
        assert!(q8["cache_mb"].as_u64() < f16["cache_mb"].as_u64(), "8 bits ocupa menos: {p}");
        assert!(["folga", "apertado", "nao-cabe"].contains(&f16["situacao"].as_str().unwrap()));
    }
    assert_eq!(r["automatico"]["f16"], json!(32768));
    // depois de escolher um tamanho, a barra mostra o escolhido e o carregado
    c.post(format!("{base}/motor/carregar")).json(&json!({"modelo": id, "contexto": 12288, "cache_kv": "q8_0"})).send().await.unwrap();
    let r: Value = c.get(format!("{base}/motor/modelos/memoria?id={id}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["pedido"]["contexto"], json!(12288));
    assert_eq!(r["carregado"]["contexto_carregado"], json!(12288));
    assert_eq!(r["carregado"]["cache_kv"], "q8_0");
    let r = c.get(format!("{base}/motor/modelos/memoria?id=nao-existe")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 404);
    m.unload().await;
}
