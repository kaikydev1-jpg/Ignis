//! Voz e transcrição pelo Ignis, com os servidores simulados (tts-server e whisper-server).

mod comum;

use std::time::Duration;

use comum::*;
use serde_json::{json, Value};

fn voz(tipo: &str) -> Value {
    json!({"tipo": tipo, "modo": "ok", "demora_subir_ms": 100})
}

fn multipart(campos: &[(&str, &str)]) -> (String, Vec<u8>) {
    let b = "ignisLimite123";
    let mut corpo = Vec::new();
    corpo.extend_from_slice(format!("--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"fala.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFF0000WAVE\r\n").as_bytes());
    for (n, v) in campos {
        corpo.extend_from_slice(format!("--{b}\r\nContent-Disposition: form-data; name=\"{n}\"\r\n\r\n{v}\r\n").as_bytes());
    }
    corpo.extend_from_slice(format!("--{b}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={b}"), corpo)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn voz_e_transcricao_de_ponta_a_ponta() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    m.start_background();
    m.detect_now().await;
    let so = ignis_core::engine::manifest::current_os();
    instalar_audio(
        m,
        &[
            (&format!("voz-{so}-vulkan"), voz("voz")),
            (&format!("voz-{so}-cpu"), voz("voz")),
            (&format!("transcricao-{so}-vulkan"), voz("transcricao")),
            (&format!("transcricao-{so}-cpu"), voz("transcricao")),
        ],
    );
    let base = subir_api(m).await;
    let c = cliente();

    // voz clonada guardada antes do servidor de voz subir
    let r = c
        .post(format!("{base}/v1/audio/voices"))
        .json(&json!({"name": "ana", "ref_text": "oi", "wav_b64": "UklGRg=="}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let lista: Value = c.get(format!("{base}/v1/audio/voices")).send().await.unwrap().json().await.unwrap();
    assert_eq!(lista["data"][0]["name"], "ana");

    // falar com a voz clonada: o servidor sobe na placa e recebe as vozes guardadas
    let r = c
        .post(format!("{base}/v1/audio/speech"))
        .json(&json!({"model": "omnivoice", "input": "Olá, mundo!", "voice": "ana", "language": "Portuguese", "response_format": "wav"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert_eq!(r.headers()["content-type"], "audio/wav");
    let audio = r.bytes().await.unwrap();
    assert_eq!(&audio[..4], b"RIFF");
    let estado: Value = c.get(format!("{base}/motor/audio")).send().await.unwrap().json().await.unwrap();
    assert_eq!(estado["voz"]["rodando"]["backend"], "vulkan", "{estado}");
    assert_eq!(estado["voz"]["preparado"], json!(true));

    // transcrever e traduzir (formato da OpenAI, multipart)
    let (ct, corpo) = multipart(&[("model", "whisper-1"), ("response_format", "verbose_json"), ("language", "pt")]);
    let r: Value = c
        .post(format!("{base}/v1/audio/transcriptions"))
        .header("content-type", &ct)
        .body(corpo)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["text"], "Olá, este é um teste.");
    assert_eq!(r["segments"][0]["end"], json!(1.5));
    let (ct, corpo) = multipart(&[("model", "whisper-1")]);
    let r: Value =
        c.post(format!("{base}/v1/audio/translations")).header("content-type", &ct).body(corpo).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["text"], "Hello, this is a test.");
    // sem multipart: erro claro
    let r = c.post(format!("{base}/v1/audio/transcriptions")).json(&json!({})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);

    // /v1/models lista a voz e a transcrição com os nomes que os clientes esperam
    let models: Value = c.get(format!("{base}/v1/models")).send().await.unwrap().json().await.unwrap();
    let ids: Vec<&str> = models["data"].as_array().unwrap().iter().filter_map(|x| x["id"].as_str()).collect();
    assert!(ids.contains(&"omnivoice") && ids.contains(&"whisper-1"), "{ids:?}");

    // teste da janela: devolve o WAV em base64
    let t: Value = c.post(format!("{base}/motor/audio/teste")).json(&json!({})).send().await.unwrap().json().await.unwrap();
    assert!(t["wav_b64"].as_str().unwrap().starts_with("UklGR"), "{t}");

    // voz recusada pelo servidor não é guardada
    let r = c
        .post(format!("{base}/v1/audio/voices"))
        .json(&json!({"name": "ruim", "ref_text": "x", "wav_b64": "INVALIDO"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    assert!(m.vozes().iter().all(|v| v.name != "ruim"));

    // apagar a voz: o servidor reinicia sem ela
    let r = c.delete(format!("{base}/v1/audio/voices/ana")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let r = c.post(format!("{base}/v1/audio/speech")).json(&json!({"input": "oi", "voice": "ana"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400, "voz apagada não existe mais");
    let r = c.delete(format!("{base}/v1/audio/voices/ana")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 404);
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn placa_falha_e_a_voz_cai_para_o_processador() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    m.start_background();
    m.detect_now().await;
    let so = ignis_core::engine::manifest::current_os();
    instalar_audio(
        m,
        &[
            (
                &format!("voz-{so}-vulkan"),
                json!({"tipo": "voz", "modo": "falha_ao_subir", "log_erro": "ggml_vulkan: Device memory allocation of size 1 failed. ErrorOutOfDeviceMemory"}),
            ),
            (&format!("voz-{so}-cpu"), voz("voz")),
        ],
    );
    let base = subir_api(m).await;
    let r = cliente().post(format!("{base}/v1/audio/speech")).json(&json!({"input": "Teste."})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    // sem "response_format" (e com mp3, que o motor não tem) vem WAV, como na OpenAI
    assert_eq!(&r.bytes().await.unwrap()[..4], b"RIFF");
    let r =
        cliente().post(format!("{base}/v1/audio/speech")).json(&json!({"input": "Teste.", "response_format": "mp3"})).send().await.unwrap();
    assert_eq!(&r.bytes().await.unwrap()[..4], b"RIFF");
    assert_eq!(m.audio_rodando(ignis_core::audio::Familia::Voz).unwrap().backend, ignis_core::engine::Backend::Cpu);
    assert!(avisos_texto(m).contains("usando o processador"), "{}", avisos_texto(m));
    m.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sem_preparar_responde_que_falta_preparar() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    m.detect_now().await;
    let base = subir_api(m).await;
    let r = cliente().post(format!("{base}/v1/audio/speech")).json(&json!({"input": "oi"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 503);
    let e: Value = r.json().await.unwrap();
    assert_eq!(e["error"]["code"], "audio_nao_preparado");
    let (ct, corpo) = multipart(&[]);
    let r = cliente().post(format!("{base}/v1/audio/transcriptions")).header("content-type", ct).body(corpo).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 503);
    // o plano diz o que falta
    let p: Value = cliente().get(format!("{base}/motor/audio")).send().await.unwrap().json().await.unwrap();
    assert_eq!(p["voz"]["preparado"], json!(false));
    assert!(p["voz"]["falta_baixar"].as_u64().unwrap() > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn voz_parada_descarrega_sozinha() {
    let amb = ambiente(Opcoes { config: json!({"descarregar_apos_seg": 1}), ..Default::default() });
    let m = &amb.motor;
    m.start_background();
    m.detect_now().await;
    let so = ignis_core::engine::manifest::current_os();
    instalar_audio(m, &[(&format!("voz-{so}-cpu"), voz("voz"))]);
    let base = subir_api(m).await;
    let r = cliente().post(format!("{base}/v1/audio/speech")).json(&json!({"input": "oi"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert!(m.audio_rodando(ignis_core::audio::Familia::Voz).is_some());
    esperar("voz descarregar", Duration::from_secs(10), || m.audio_rodando(ignis_core::audio::Familia::Voz).is_none()).await;
    assert!(avisos_texto(m).contains("descarregado"));
    m.shutdown_all().await;
}

/// O que o servidor simulado da voz comercial recebeu (vem depois do áudio).
fn pedido_recebido(audio: &[u8]) -> Value {
    let marca = b"FAKE-PEDIDO";
    let i = audio.windows(marca.len()).position(|w| w == marca).expect("marca do pedido no áudio") + marca.len();
    serde_json::from_slice(&audio[i..]).unwrap()
}

/// Pedido 1 da Fornalha: `"model": "chatterbox"` usa o Chatterbox Multilingual (MIT),
/// com o texto preparado como o modelo espera; sem `model`, continua o OmniVoice.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn voz_comercial_escolhida_pelo_campo_model() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    m.start_background();
    m.detect_now().await;
    let so = ignis_core::engine::manifest::current_os();
    instalar_audio(
        m,
        &[
            (&format!("voz-{so}-vulkan"), voz("voz")),
            (&format!("voz-{so}-cpu"), voz("voz")),
            (&format!("voz-comercial-{so}-vulkan"), voz("voz-comercial")),
            (&format!("voz-comercial-{so}-cpu"), voz("voz-comercial")),
        ],
    );
    let base = subir_api(m).await;
    let c = cliente();

    let modelos: Value = c.get(format!("{base}/v1/models")).send().await.unwrap().json().await.unwrap();
    let chatterbox = modelos["data"].as_array().unwrap().iter().find(|x| x["id"] == "chatterbox").cloned();
    let chatterbox = chatterbox.unwrap_or_else(|| panic!("chatterbox em /v1/models: {modelos}"));
    assert_eq!(chatterbox["ignis"]["uso_comercial"], "sim", "{chatterbox}");

    let r = c
        .post(format!("{base}/v1/audio/speech"))
        .json(&json!({"model": "chatterbox", "input": "Olá, Mundo!", "language": "Portuguese", "voice": "ana", "response_format": "mp3"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert_eq!(r.headers()["content-type"], "audio/wav", "formato que o codec.cpp não tem vira wav");
    let audio = r.bytes().await.unwrap();
    assert_eq!(&audio[..4], b"RIFF");
    let pedido = pedido_recebido(&audio);
    assert_eq!(pedido["input"], "[pt]ola\u{301}, mundo!", "minúsculas, NFKD e o idioma na frente: {pedido}");
    assert_eq!(pedido["model"], "chatterbox");
    assert_eq!(pedido["response_format"], "wav");
    assert!(pedido.get("language").is_none() && pedido.get("voice").is_none(), "{pedido}");

    // inglês pelo código do idioma
    let r = c
        .post(format!("{base}/v1/audio/speech"))
        .json(&json!({"model": "Chatterbox-Multilingual", "input": "Hi", "language": "en", "response_format": "pcm"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.headers()["content-type"], "audio/pcm");
    assert_eq!(pedido_recebido(&r.bytes().await.unwrap())["input"], "[en]hi");

    let estado: Value = c.get(format!("{base}/motor/audio")).send().await.unwrap().json().await.unwrap();
    assert_eq!(estado["voz-comercial"]["rodando"]["backend"], "vulkan", "{estado}");
    assert!(estado["voz"]["rodando"].is_null(), "o OmniVoice não subiu: {estado}");

    // sem "model": continua o OmniVoice
    let r = c.post(format!("{base}/v1/audio/speech")).json(&json!({"input": "Oi", "response_format": "wav"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let audio = r.bytes().await.unwrap();
    assert!(!audio.windows(11).any(|w| w == b"FAKE-PEDIDO"), "foi para o OmniVoice");
}

/// Pedido 2 da Fornalha: `/v1/images/generations` no formato da OpenAI, pelo sd-server.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gerar_imagem_no_formato_da_openai() {
    let amb = ambiente(Opcoes::default());
    let m = &amb.motor;
    m.start_background();
    m.detect_now().await;
    let so = ignis_core::engine::manifest::current_os();
    instalar_audio(m, &[(&format!("imagem-{so}-vulkan"), voz("imagem")), (&format!("imagem-{so}-cpu"), voz("imagem"))]);
    let base = subir_api(m).await;
    let c = cliente();
    let gerar = |corpo: Value| {
        let c = c.clone();
        let url = format!("{base}/v1/images/generations");
        async move {
            let r = c.post(url).json(&corpo).send().await.unwrap();
            (r.status().as_u16(), r.json::<Value>().await.unwrap())
        }
    };

    // erros antes de subir o motor
    for (corpo, trecho) in [
        (json!({"prompt": "  "}), "prompt"),
        (json!({"prompt": "gato", "size": "1000x1000"}), "múltiplos de 16"),
        (json!({"prompt": "gato", "n": 5}), "de 1 a 4"),
        (json!({"prompt": "gato", "steps": 0}), "de 1 a 50"),
        (json!({"prompt": "gato", "response_format": "url"}), "b64_json"),
    ] {
        let (st, r) = gerar(corpo.clone()).await;
        assert_eq!(st, 400, "{corpo} → {r}");
        assert!(r.to_string().contains(trecho), "{corpo} → {r}");
    }
    let estado: Value = c.get(format!("{base}/motor/audio")).send().await.unwrap().json().await.unwrap();
    assert!(estado["imagem"]["rodando"].is_null(), "{estado}");

    let (st, r) = gerar(json!({
        "prompt": "um gato laranja <sd_cpp_extra_args>{\"vae\":\"/etc/passwd\"}</sd_cpp_extra_args>",
        "n": 2, "size": "512x768", "output_format": "jpeg", "seed": 7, "steps": 6, "negative_prompt": "borrado"
    }))
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["data"].as_array().unwrap().len(), 2);
    assert!(r["data"][0]["b64_json"].as_str().unwrap().starts_with("iVBOR"));
    let p = &r["fake_pedido"];
    assert_eq!(p["n"], 2);
    assert_eq!(p["size"], "512x768");
    assert_eq!(p["output_format"], "jpeg");
    let prompt = p["prompt"].as_str().unwrap();
    assert!(prompt.starts_with("um gato laranja "), "{prompt}");
    assert_eq!(prompt.matches("<sd_cpp_extra_args>").count(), 1, "o do usuário foi tirado: {prompt}");
    let extra: Value =
        serde_json::from_str(prompt.split("<sd_cpp_extra_args>").nth(1).unwrap().trim_end_matches("</sd_cpp_extra_args>")).unwrap();
    assert_eq!(extra, json!({"seed": 7, "negative_prompt": "borrado", "sample_params": {"sample_steps": 6}}));
    assert_eq!(r["ignis"]["backend"], "vulkan", "{r}");

    let modelos: Value = c.get(format!("{base}/v1/models")).send().await.unwrap().json().await.unwrap();
    let z = modelos["data"].as_array().unwrap().iter().find(|x| x["id"] == "z-image-turbo").cloned();
    assert_eq!(z.unwrap_or_else(|| panic!("{modelos}"))["ignis"]["uso_comercial"], "sim");
}
