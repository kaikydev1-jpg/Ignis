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
