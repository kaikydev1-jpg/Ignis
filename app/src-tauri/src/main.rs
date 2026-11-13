//! Ignis — aplicativo com ícone na bandeja (perto do relógio).
//!
//! Dois processos, o mesmo executável:
//! - **Núcleo** (`ignis` / `ignis --minimizado`): bandeja + motor + API. Nunca
//!   abre WebView, por isso fica leve em repouso.
//! - **Janela** (`ignis --janela`, aberto pelo núcleo): só existe enquanto a
//!   janela está aberta. Fechar a janela encerra esse processo e devolve toda a
//!   memória; o Ignis continua na bandeja. "Sair" fica no menu do ícone.
//!
//! A janela fala com o núcleo por um canal local exclusivo (porta aleatória em
//! 127.0.0.1 + senha aleatória), que funciona mesmo se a porta da API estiver ocupada.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ignis_core::autostart::ARG_MINIMIZADO;
use ignis_core::consts::APP_NAME;
use ignis_core::paths::Paths;
use ignis_core::service::{Fase, Motor, MotorOptions};
use serde_json::{json, Value};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Notify;

const JANELA: &str = "principal";
const BANDEJA: &str = "bandeja";
const ARG_JANELA: &str = "--janela";
const ENV_PORTA: &str = "IGNIS_JANELA_PORTA";
const ENV_SENHA: &str = "IGNIS_JANELA_SENHA";
const HEADER_SENHA: &str = "x-ignis-janela";

fn iniciar_registro(paths: &Paths, nome: &str) {
    let _ = std::fs::create_dir_all(paths.logs_dir());
    let arquivo = paths.logs_dir().join(nome);
    if std::fs::metadata(&arquivo).map(|m| m.len() > 5 * 1024 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&arquivo, paths.logs_dir().join(format!("{nome}.1")));
    }
    let filtro = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    match std::fs::OpenOptions::new().create(true).append(true).open(&arquivo) {
        Ok(f) => {
            let _ = tracing_subscriber::fmt().with_env_filter(filtro).with_ansi(false).with_writer(Mutex::new(f)).try_init();
        }
        Err(_) => {
            let _ = tracing_subscriber::fmt().with_env_filter(filtro).try_init();
        }
    }
}

fn senha_aleatoria() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut s = String::new();
    for _ in 0..4 {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        h.write_u32(std::process::id());
        s.push_str(&format!("{:016x}", h.finish()));
    }
    s
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let paths = Paths::from_env();
    if args.iter().any(|a| a == ARG_JANELA) {
        iniciar_registro(&paths, "janela.log");
        janela::rodar(paths);
    } else {
        iniciar_registro(&paths, "ignis.log");
        nucleo::rodar(paths, args.iter().any(|a| a == ARG_MINIMIZADO));
    }
}

// =====================================================================
// Núcleo: bandeja + motor + API
// =====================================================================
mod nucleo {
    use super::*;

    pub struct Estado {
        pub motor: Arc<Motor>,
        pub janela: Mutex<Option<Child>>,
        pub foco: Arc<Notify>,
        pub porta_janela: Mutex<Option<u16>>,
        pub senha: String,
    }

    /// Abre a janela (processo separado) ou traz para a frente se já estiver aberta.
    pub fn abrir_janela(app: &AppHandle) {
        let st = app.state::<Estado>();
        let mut j = st.janela.lock().unwrap();
        if let Some(c) = j.as_mut() {
            if c.try_wait().ok().flatten().is_none() {
                st.foco.notify_waiters();
                return;
            }
        }
        let Some(porta) = *st.porta_janela.lock().unwrap() else {
            tracing::warn!("canal da janela ainda não está pronto");
            return;
        };
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("não sei onde está o executável: {e}");
                return;
            }
        };
        match Command::new(exe).arg(ARG_JANELA).env(ENV_PORTA, porta.to_string()).env(ENV_SENHA, &st.senha).spawn() {
            Ok(c) => *j = Some(c),
            Err(e) => tracing::error!("não consegui abrir a janela: {e}"),
        }
    }

    fn fechar_janela(app: &AppHandle) {
        if let Some(mut c) = app.state::<Estado>().janela.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    async fn checar_senha(State(senha): State<Arc<String>>, req: Request, next: Next) -> Response {
        let ok = req.headers().get(HEADER_SENHA).and_then(|v| v.to_str().ok()) == Some(senha.as_str());
        if !ok {
            return (StatusCode::FORBIDDEN, "senha da janela inválida").into_response();
        }
        next.run(req).await
    }

    async fn esperar_foco(State(foco): State<Arc<Notify>>) -> Response {
        tokio::select! {
            _ = foco.notified() => axum::Json(json!({"foco": true})).into_response(),
            _ = tokio::time::sleep(Duration::from_secs(25)) => axum::Json(json!({"foco": false})).into_response(),
        }
    }

    /// Canal exclusivo da janela: mesma API + rota de foco, com senha.
    async fn servir_canal_janela(app: AppHandle, motor: Arc<Motor>) {
        let st = app.state::<Estado>();
        let senha = Arc::new(st.senha.clone());
        let foco = st.foco.clone();
        let rotas = ignis_core::api::router(motor)
            .merge(axum::Router::new().route("/janela/esperar-foco", get(esperar_foco)).with_state(foco))
            .layer(middleware::from_fn_with_state(senha, checar_senha));
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", 0)).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("canal da janela: {e}");
                return;
            }
        };
        let porta = listener.local_addr().map(|a| a.port()).ok();
        *st.porta_janela.lock().unwrap() = porta;
        tracing::info!("canal da janela na porta {porta:?}");
        let _ = axum::serve(listener, rotas).await;
    }

    struct ItensBandeja {
        modelo: MenuItem<tauri::Wry>,
        pausar: MenuItem<tauri::Wry>,
        descarregar: MenuItem<tauri::Wry>,
    }

    fn montar_bandeja(app: &AppHandle) -> tauri::Result<ItensBandeja> {
        let abrir = MenuItem::with_id(app, "abrir", "Abrir Ignis", true, None::<&str>)?;
        let modelo = MenuItem::with_id(app, "modelo", "Modelo: nenhum", false, None::<&str>)?;
        let pausar = MenuItem::with_id(app, "pausar", "Pausar", true, None::<&str>)?;
        let descarregar = MenuItem::with_id(app, "descarregar", "Descarregar modelo", false, None::<&str>)?;
        let sair = MenuItem::with_id(app, "sair", "Sair", true, None::<&str>)?;
        let sep1 = PredefinedMenuItem::separator(app)?;
        let sep2 = PredefinedMenuItem::separator(app)?;
        let menu = Menu::with_items(app, &[&abrir, &sep1, &modelo, &pausar, &descarregar, &sep2, &sair])?;
        let icone = app.default_window_icon().cloned().ok_or_else(|| tauri::Error::AssetNotFound("ícone".into()))?;
        TrayIconBuilder::with_id(BANDEJA)
            .icon(icone)
            .tooltip(APP_NAME)
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, ev| {
                let motor = app.state::<Estado>().motor.clone();
                match ev.id().as_ref() {
                    "abrir" => abrir_janela(app),
                    "pausar" => {
                        tauri::async_runtime::spawn(async move {
                            if motor.is_paused() {
                                motor.resume();
                            } else {
                                motor.pause().await;
                            }
                        });
                    }
                    "descarregar" => {
                        tauri::async_runtime::spawn(async move {
                            motor.unload().await;
                        });
                    }
                    "sair" => {
                        let app = app.clone();
                        fechar_janela(&app);
                        tauri::async_runtime::spawn(async move {
                            motor.shutdown_all().await;
                            app.exit(0);
                        });
                    }
                    _ => {}
                }
            })
            .on_tray_icon_event(|tray, ev| {
                // clique simples abre a janela (no Linux só o menu aparece)
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                    abrir_janela(tray.app_handle());
                }
            })
            .build(app)?;
        Ok(ItensBandeja { modelo, pausar, descarregar })
    }

    /// Mantém o menu e a dica do ícone em dia (espera mudanças, sem ficar consultando).
    fn vigiar_bandeja(app: AppHandle, motor: Arc<Motor>, itens: ItensBandeja) {
        tauri::async_runtime::spawn(async move {
            let mut rx = motor.subscribe();
            loop {
                let carregado = motor.loaded_info();
                let (fase, texto) = motor.fase();
                let modelo = match &carregado {
                    Some(i) => format!("Modelo: {} ({})", i.modelo_nome, i.motor_nome),
                    None if fase == Fase::Carregando => "Modelo: carregando…".to_string(),
                    None => "Modelo: nenhum".to_string(),
                };
                let _ = itens.modelo.set_text(modelo);
                let _ = itens.pausar.set_text(if motor.is_paused() { "Retomar" } else { "Pausar" });
                let _ = itens.descarregar.set_enabled(carregado.is_some());
                let dica = match fase {
                    Fase::Pausado => format!("{APP_NAME} — pausado"),
                    Fase::Pronto => format!("{APP_NAME} — {}", carregado.map(|i| i.modelo_nome).unwrap_or_default()),
                    Fase::Parado => format!("{APP_NAME} — pronto, sem modelo carregado"),
                    _ => format!("{APP_NAME} — {texto}"),
                };
                if let Some(t) = app.tray_by_id(BANDEJA) {
                    let _ = t.set_tooltip(Some(dica));
                }
                if rx.changed().await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
                rx.borrow_and_update();
            }
        });
    }

    pub fn rodar(paths: Paths, minimizado: bool) {
        tracing::info!("{APP_NAME} {} iniciando (minimizado: {minimizado})", ignis_core::consts::APP_VERSION);
        let app = tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| abrir_janela(app)))
            .setup(move |app| {
                let extras = app.path().resource_dir().ok().map(|d| d.join("vcrt")).filter(|d| d.is_dir());
                let motor = Motor::new(paths.clone(), MotorOptions { extras_motor: extras, ..Default::default() });
                app.manage(Estado {
                    motor: motor.clone(),
                    janela: Mutex::new(None),
                    foco: Arc::new(Notify::new()),
                    porta_janela: Mutex::new(None),
                    senha: senha_aleatoria(),
                });
                let m2 = motor.clone();
                tauri::async_runtime::spawn(async move {
                    m2.start_background();
                    ignis_core::api::serve(m2).await;
                });
                let h = app.handle().clone();
                let m3 = motor.clone();
                tauri::async_runtime::spawn(async move { servir_canal_janela(h, m3).await });
                let itens = montar_bandeja(app.handle())?;
                vigiar_bandeja(app.handle().clone(), motor, itens);
                if !minimizado {
                    let h = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        // espera o canal da janela ficar pronto
                        for _ in 0..50 {
                            if h.state::<Estado>().porta_janela.lock().unwrap().is_some() {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                        abrir_janela(&h);
                    });
                }
                Ok(())
            })
            .build(tauri::generate_context!())
            .expect("erro ao iniciar o Ignis");

        app.run(|app, ev| match ev {
            // sem janelas no núcleo: só "Sair" encerra
            RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
            RunEvent::Exit => {
                fechar_janela(app);
                let motor = app.state::<Estado>().motor.clone();
                tauri::async_runtime::block_on(async move { motor.shutdown_all().await });
            }
            _ => {}
        });
    }
}

// =====================================================================
// Janela: processo separado que só vive enquanto a janela está aberta
// =====================================================================
mod janela {
    use super::*;

    struct Canal {
        base: String,
        senha: String,
        http: reqwest::Client,
        paths: Paths,
    }

    /// A janela fala com o núcleo pelo canal exclusivo.
    #[tauri::command]
    async fn api(canal: tauri::State<'_, Canal>, method: String, path: String, body: Option<Value>) -> Result<Value, String> {
        if !path.starts_with("/motor/") && !path.starts_with("/v1/models") {
            return Err("rota não permitida pela janela".into());
        }
        let metodo = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
        let mut req = canal.http.request(metodo, format!("{}{}", canal.base, path)).header(HEADER_SENHA, &canal.senha);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let r = req.send().await.map_err(|_| "Sem conexão com o núcleo do Ignis.".to_string())?;
        let status = r.status().as_u16();
        let corpo: Value = r.json().await.unwrap_or(Value::Null);
        Ok(json!({"status": status, "corpo": corpo}))
    }

    /// Abre um link no navegador (só endereços https conhecidos).
    #[tauri::command]
    fn abrir_link(app: AppHandle, url: String) -> Result<(), String> {
        let permitido = ["https://huggingface.co/", "https://github.com/", "https://ai.google.dev/", "https://extensions.gnome.org/"];
        if !permitido.iter().any(|p| url.starts_with(p)) {
            return Err("link não permitido".into());
        }
        app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
    }

    /// Abre a pasta de modelos ou de registros no gerenciador de arquivos.
    #[tauri::command]
    fn abrir_pasta(app: AppHandle, canal: tauri::State<'_, Canal>, qual: String) -> Result<(), String> {
        let p = match qual.as_str() {
            "modelos" => canal.paths.models_dir(),
            "registros" => canal.paths.logs_dir(),
            _ => return Err("pasta desconhecida".into()),
        };
        app.opener().open_path(p.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
    }

    /// O que o sistema diz sobre os botões da janela (lado, ordem, tema).
    struct SistemaJanela(ignis_core::janela::Sistema);

    fn sistema_da_janela() -> ignis_core::janela::Sistema {
        let so = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "mac"
        } else {
            "linux"
        };
        #[allow(unused_mut)]
        let mut s = ignis_core::janela::Sistema {
            so: so.into(),
            desktop: std::env::var("XDG_CURRENT_DESKTOP").ok().filter(|d| !d.is_empty()),
            ..Default::default()
        };
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::*;
            // O GNOME e o KDE (e o portal, no Flatpak) preenchem estas configurações do GTK.
            if let Some(cfg) = gtk::Settings::default() {
                s.layout = cfg.property::<Option<String>>("gtk-decoration-layout");
                s.tema = cfg.property::<Option<String>>("gtk-theme-name");
            }
            // Matsu OS (Fedora Atomic do criador): bolinhas com as cores exatas dele.
            s.matsu = ignis_core::distro::eh_matsu(&ignis_core::distro::ler_os_release());
        }
        s
    }

    #[tauri::command]
    fn botoes_janela(sis: tauri::State<'_, SistemaJanela>, preferido: Option<String>) -> ignis_core::janela::BotoesJanela {
        ignis_core::janela::escolher(&sis.0, preferido.as_deref().unwrap_or("auto"))
    }

    pub fn rodar(paths: Paths) {
        let (Ok(porta), Ok(senha)) = (std::env::var(ENV_PORTA), std::env::var(ENV_SENHA)) else {
            eprintln!("A janela do Ignis é aberta pelo próprio Ignis (ícone perto do relógio).");
            std::process::exit(2);
        };
        ignis_core::install_crypto_provider();
        let base = format!("http://127.0.0.1:{porta}");
        let http = ignis_core::download::plain_client();
        let canal = Canal { base: base.clone(), senha: senha.clone(), http: http.clone(), paths };
        tauri::Builder::default()
            .plugin(tauri_plugin_opener::init())
            .manage(canal)
            .invoke_handler(tauri::generate_handler![api, abrir_link, abrir_pasta, botoes_janela])
            .setup(move |app| {
                // Na thread principal, com o GTK já iniciado.
                let sistema = sistema_da_janela();
                tracing::info!(?sistema, "botões da janela");
                app.manage(SistemaJanela(sistema));
                let w = WebviewWindowBuilder::new(app, JANELA, WebviewUrl::App("index.html".into()))
                    .title(APP_NAME)
                    .inner_size(1000.0, 720.0)
                    .min_inner_size(760.0, 520.0)
                    // Barra de título integrada: o topo do próprio Ignis faz o papel
                    // da barra (arrastar, duplo clique, botões). No Windows a sombra
                    // e os cantos arredondados continuam sendo do sistema.
                    .decorations(false)
                    .shadow(true)
                    .center()
                    .build()?;
                let _ = w.set_focus();
                // Espera pedidos de foco do núcleo; se o núcleo sumir, fecha.
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut falhas = 0;
                    loop {
                        let r = http
                            .get(format!("{base}/janela/esperar-foco"))
                            .header(HEADER_SENHA, &senha)
                            .timeout(Duration::from_secs(30))
                            .send()
                            .await;
                        match r {
                            Ok(resp) if resp.status().is_success() => {
                                falhas = 0;
                                let v: Value = resp.json().await.unwrap_or(Value::Null);
                                if v["foco"] == json!(true) {
                                    if let Some(w) = h.get_webview_window(JANELA) {
                                        let _ = w.unminimize();
                                        let _ = w.show();
                                        let _ = w.set_focus();
                                    }
                                }
                            }
                            _ => {
                                falhas += 1;
                                if falhas >= 5 {
                                    tracing::warn!("núcleo não responde; fechando a janela");
                                    h.exit(0);
                                    break;
                                }
                                tokio::time::sleep(Duration::from_secs(2)).await;
                            }
                        }
                    }
                });
                Ok(())
            })
            .run(tauri::generate_context!())
            .expect("erro ao abrir a janela do Ignis");
    }
}
