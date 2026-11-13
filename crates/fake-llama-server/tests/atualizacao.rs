//! Atualização do Ignis com um clique, contra uma "versão oficial" simulada
//! (servidor local com atualizacoes.json, SHA256SUMS.txt e o arquivo novo).

mod comum;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path as UrlPath, State};
use axum::routing::get;
use axum::Router;
use comum::*;
use ignis_core::atualizador::Instalacao;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const NOVO: &[u8] = b"AppImage da versao nova";

#[derive(Clone)]
struct Versao {
    manifesto: Arc<Mutex<Value>>,
}

async fn subir_servidor(manifesto: Value) -> (String, Versao) {
    let estado = Versao { manifesto: Arc::new(Mutex::new(manifesto)) };
    let app = Router::new()
        .route("/atualizacoes.json", get(|State(v): State<Versao>| async move { axum::Json(v.manifesto.lock().unwrap().clone()) }))
        .route(
            "/download/{tag}/{arquivo}",
            get(|UrlPath((tag, arquivo)): UrlPath<(String, String)>| async move {
                let sha = hex::encode(Sha256::digest(NOVO));
                // a 9.9.8 publica uma soma errada: o download tem de ser recusado
                let sha = if tag == "v9.9.8" { "0".repeat(64) } else { sha };
                match arquivo.as_str() {
                    "SHA256SUMS.txt" => Ok(format!("{sha}  Ignis-Linux.AppImage\n{}  Ignis-Fedora.rpm\n", "1".repeat(64)).into_bytes()),
                    "Ignis-Linux.AppImage" => Ok(NOVO.to_vec()),
                    _ => Err(axum::http::StatusCode::NOT_FOUND),
                }
            }),
        )
        .with_state(estado.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (base, estado)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn atualiza_com_um_clique_e_confere_o_arquivo() {
    let pasta = tempfile::tempdir().unwrap();
    let appimage = pasta.path().join("Ignis.AppImage");
    std::fs::write(&appimage, b"versao velha").unwrap();
    let amb = ambiente(Opcoes { instalacao: Some(Instalacao::AppImage { caminho: appimage.clone() }), ..Default::default() });
    let m = &amb.motor;
    let manifesto = |versao: &str| json!({"versao_manifesto": 1, "app": {"versao": versao, "pagina": "https://exemplo/x", "notas": ""}, "motores": m.manifest(), "catalogo": m.catalog()});
    let (base, servidor) = subir_servidor(manifesto("9.9.8")).await;
    // só este teste roda neste binário: pode trocar as variáveis do processo
    std::env::set_var("IGNIS_URL_ATUALIZACOES", format!("{base}/atualizacoes.json"));
    std::env::set_var("IGNIS_URL_RELEASES", &base);
    let api = subir_api(m).await;
    let c = cliente();
    let mut saida = m.pedido_de_saida();

    // o estado diz como o Ignis está instalado
    let st: Value = c.get(format!("{api}/motor/estado")).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["instalacao"]["tipo"], "appimage", "{st}");
    assert_eq!(st["instalacao"]["automatica"], json!(true));

    // 1) soma errada na lista: não instala nada e avisa
    let r: Value = c.post(format!("{api}/motor/atualizacoes/procurar")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["info"]["app"]["versao"], "9.9.8", "{r}");
    assert!(avisos_texto(m).contains("Atualização disponível"));
    let r = c.post(format!("{api}/motor/atualizacoes/aplicar")).json(&json!({"item": "app"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    esperar("recusar o arquivo", Duration::from_secs(10), || avisos_texto(m).contains("Não consegui baixar o Ignis 9.9.8")).await;
    assert_eq!(std::fs::read(&appimage).unwrap(), b"versao velha");
    assert!(saida.borrow().is_none());

    // 2) versão certa: baixa, confere, troca o AppImage e pede para fechar e reabrir
    *servidor.manifesto.lock().unwrap() = manifesto("9.9.9");
    m.check_updates().await.unwrap();
    c.post(format!("{api}/motor/atualizacoes/aplicar")).json(&json!({"item": "app"})).send().await.unwrap().error_for_status().unwrap();
    tokio::time::timeout(Duration::from_secs(10), saida.wait_for(|c| c.is_some())).await.unwrap().unwrap();
    let comando = saida.borrow().clone().unwrap();
    assert_eq!(std::fs::read(&appimage).unwrap(), NOVO);
    assert_eq!(comando.programa, "sh");
    assert!(comando.args[1].contains(&format!("while kill -0 {}", std::process::id())), "{comando:?}");
    assert!(comando.args[1].contains(appimage.to_str().unwrap()), "{comando:?}");
    assert!(avisos_texto(m).contains("vai fechar e abrir de novo"));
}
