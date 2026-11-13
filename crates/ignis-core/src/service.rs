//! O coração do Ignis: estado central compartilhado entre a API, a janela e a
//! bandeja. Carrega/descarrega modelos, cai para o próximo motor quando um
//! falha, descarrega sozinho depois de um tempo parado, cuida dos downloads,
//! da preparação (motores + modelo de teste) e do teste de velocidade.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::autostart::{self, Launcher};
use crate::benchmark::{self, BenchResult};
use crate::budget::{self, Plano, Recursos};
use crate::catalog::Catalog;
use crate::config::{Settings, SettingsPatch};
use crate::consts::{API_VERSION, APP_NAME, APP_VERSION, URL_ATUALIZACOES};
use crate::download::{self, DownloadError, Progress};
use crate::engine::{build_candidates, Backend, Candidate, EngineManifest, EngineStore, VARIANTE_PROPRIA};
use crate::gpu::{self, Detection};
use crate::models::{InstalledModel, Origem, Registry, RemoveOutcome};
use crate::ollama;
use crate::paths::Paths;
use crate::supervisor::{self, Server};
use crate::updates::{self, UpdateCheck};
use crate::util::{now_millis, now_unix};

#[path = "service_audio.rs"]
mod audio_svc;
pub use audio_svc::{AudioInfo, AudioUso};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fase {
    Parado,
    Carregando,
    Pronto,
    Pausado,
    Testando,
    Preparando,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Aviso {
    pub quando: u64,
    /// "info", "aviso" ou "erro"
    pub nivel: String,
    pub texto: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Velocidade {
    pub leitura_tps: Option<f64>,
    pub geracao_tps: Option<f64>,
    pub quando: u64,
}

/// Maior modelo usado no teste de velocidade geral.
const TAMANHO_MAX_TESTE: u64 = 2_200_000_000;

/// Respostas mais curtas que isso não entram na conta de velocidade do modelo.
const MIN_TOKENS_MEDIDA: u64 = 16;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TesteSalvo {
    pub fingerprint: String,
    pub llama_cpp: String,
    pub quando: u64,
    pub resultados: Vec<BenchResult>,
    pub ordem: Vec<String>,
    /// Tamanho do modelo usado no teste: o teste só vale para modelos parecidos.
    #[serde(default)]
    pub tamanho_modelo: Option<u64>,
    #[serde(default)]
    pub modelo_nome: Option<String>,
}

/// Velocidades medidas com um modelo específico, por motor (uso real ou teste com o modelo).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MedidasModelo {
    pub fingerprint: String,
    /// tokens/s escrevendo, por variante.
    pub por_motor: HashMap<String, f64>,
    pub quando: u64,
    /// Resultado completo do último "Testar com este modelo".
    pub teste: Option<Vec<BenchResult>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EstadoPersistido {
    pub teste: Option<TesteSalvo>,
    pub falhas: HashMap<String, u32>,
    pub ultima_verificacao: u64,
    /// Velocidade de cada modelo em cada motor.
    pub medidas: HashMap<String, MedidasModelo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CarregadoInfo {
    pub modelo_id: String,
    pub modelo_nome: String,
    pub variante: String,
    pub backend: Backend,
    pub motor_nome: String,
    pub base_url: String,
    pub plano: Plano,
    pub embeddings: bool,
    pub desde: u64,
    /// Por que este motor foi escolhido para este modelo.
    pub motivo: String,
}

struct Loaded {
    server: Server,
    info: CarregadoInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tarefa {
    pub id: String,
    /// "modelo", "motor", "importacao", "atualizacao"
    pub tipo: String,
    pub nome: String,
    pub baixado: u64,
    pub total: Option<u64>,
    /// "andamento", "pausada", "erro", "concluida"
    pub estado: String,
    pub erro: Option<String>,
    pub atualizado: u64,
    #[serde(skip)]
    cancel: CancellationToken,
}

#[derive(Debug, thiserror::Error)]
pub enum MotorError {
    #[error("O Ignis está pausado. Clique em Retomar no ícone perto do relógio.")]
    Pausado,
    #[error("O modelo \"{0}\" não está instalado. Veja os modelos instalados em /v1/models.")]
    ModeloNaoEncontrado(String),
    #[error("O arquivo do modelo \"{0}\" sumiu do disco. Remova-o da lista e baixe de novo.")]
    ArquivoSumiu(String),
    #[error("Nenhum motor instalado ainda. Abra o Ignis e clique em \"Preparar\".")]
    SemMotor,
    #[error("Nenhum modelo instalado. Abra o Ignis e baixe um modelo na seção Modelos.")]
    SemModelo,
    #[error("{0}")]
    Falhou(String),
    #[error("{0}")]
    Ocupado(String),
    #[error("{0}")]
    Invalido(String),
    #[error("{0}")]
    AudioNaoPreparado(String),
}

impl MotorError {
    pub fn status(&self) -> u16 {
        match self {
            MotorError::ModeloNaoEncontrado(_) => 404,
            MotorError::Invalido(_) => 400,
            MotorError::Ocupado(_) => 409,
            MotorError::Pausado | MotorError::SemMotor | MotorError::SemModelo | MotorError::AudioNaoPreparado(_) => 503,
            MotorError::ArquivoSumiu(_) | MotorError::Falhou(_) => 500,
        }
    }
    pub fn codigo(&self) -> &'static str {
        match self {
            MotorError::Pausado => "ignis_pausado",
            MotorError::ModeloNaoEncontrado(_) => "model_not_found",
            MotorError::ArquivoSumiu(_) => "arquivo_sumiu",
            MotorError::SemMotor => "sem_motor",
            MotorError::SemModelo => "sem_modelo",
            MotorError::Falhou(_) => "motor_falhou",
            MotorError::Ocupado(_) => "ocupado",
            MotorError::Invalido(_) => "invalid_request_error",
            MotorError::AudioNaoPreparado(_) => "audio_nao_preparado",
        }
    }
}

/// Opções de criação (os testes usam para simular placa e acelerar o relógio).
pub struct MotorOptions {
    /// Usa esta detecção em vez de olhar o sistema (placa simulada).
    pub detection_override: Option<Detection>,
    /// Intervalo da vigia (quedas, descarregar por tempo).
    pub tick: Duration,
    /// Como abrir o Ignis no login (None = não mexe no autostart).
    pub launcher: Option<Launcher>,
    /// Tempo máximo para um modelo carregar.
    pub load_timeout: Duration,
    /// Catálogo alternativo (testes).
    pub catalog_override: Option<Catalog>,
    /// Manifesto de motores alternativo (testes).
    pub manifest_override: Option<EngineManifest>,
    /// Testar a velocidade sozinho quando a placa/driver mudar (desligado nos testes).
    pub auto_teste: bool,
    /// Arquivos copiados para cada pasta de motor (Windows: DLLs do Visual C++).
    pub extras_motor: Option<PathBuf>,
    /// Onde procurar modelos já baixados por outros programas (None = pastas do sistema).
    pub pastas_locais: Option<PastasLocais>,
    /// Manifesto de voz e transcrição alternativo (testes).
    pub audio_manifest_override: Option<crate::audio::ManifestoAudio>,
}

/// Pastas de modelos de outros programas (Ollama, Hugging Face, LM Studio).
#[derive(Debug, Clone, Default)]
pub struct PastasLocais {
    pub ollama: Vec<PathBuf>,
    pub hugging_face: Option<PathBuf>,
    pub lm_studio: Vec<PathBuf>,
}

impl PastasLocais {
    pub fn do_sistema() -> Self {
        Self {
            ollama: crate::ollama::default_dirs(),
            hugging_face: crate::locais::pasta_hugging_face(),
            lm_studio: crate::locais::pastas_lm_studio(),
        }
    }
}

impl Default for MotorOptions {
    fn default() -> Self {
        Self {
            detection_override: None,
            tick: Duration::from_secs(1),
            launcher: Launcher::detect(),
            load_timeout: Duration::from_secs(600),
            catalog_override: None,
            manifest_override: None,
            auto_teste: true,
            extras_motor: None,
            pastas_locais: None,
            audio_manifest_override: None,
        }
    }
}

pub struct Motor {
    pub paths: Paths,
    settings: RwLock<Settings>,
    http: reqwest::Client,
    local: reqwest::Client,
    manifest: RwLock<EngineManifest>,
    catalog: RwLock<Catalog>,
    store: EngineStore,
    detection: RwLock<Option<Detection>>,
    detection_override: Option<Detection>,
    pastas_locais: Option<PastasLocais>,
    audio_man: crate::audio::ManifestoAudio,
    audio_store: crate::audio::AudioStore,
    audio_voz: tokio::sync::Mutex<Option<audio_svc::AudioRodando>>,
    audio_transcricao: tokio::sync::Mutex<Option<audio_svc::AudioRodando>>,
    audio_info: RwLock<HashMap<crate::audio::Familia, AudioInfo>>,
    audio_falhas: Mutex<HashSet<String>>,
    audio_uso: Mutex<HashMap<crate::audio::Familia, (std::time::Instant, usize)>>,
    estado: Mutex<EstadoPersistido>,
    registry: Mutex<Registry>,
    loaded: tokio::sync::Mutex<Option<Loaded>>,
    snapshot: RwLock<Option<CarregadoInfo>>,
    fase: RwLock<(Fase, String)>,
    paused: AtomicBool,
    avisos: Mutex<VecDeque<Aviso>>,
    velocidade: RwLock<Option<Velocidade>>,
    /// Quanto tempo levou o último carregamento, etapa por etapa.
    ultimo_carregamento: RwLock<Option<Value>>,
    last_activity: AtomicU64,
    ativos: AtomicUsize,
    requests: Mutex<HashMap<u64, CancellationToken>>,
    next_req: AtomicU64,
    tarefas: Mutex<HashMap<String, Tarefa>>,
    session_failed: Mutex<HashSet<String>>,
    update_info: RwLock<Option<UpdateCheck>>,
    api_status: RwLock<Result<u16, String>>,
    porta_tx: watch::Sender<u16>,
    mudancas: watch::Sender<u64>,
    launcher: Option<Launcher>,
    tick: Duration,
    load_timeout: Duration,
    background: AtomicBool,
    bench_running: AtomicBool,
    auto_teste: bool,
    hf_token: RwLock<Option<String>>,
    hf_nome: RwLock<Option<String>>,
    pub shutdown: CancellationToken,
}

/// Guarda de uma requisição: conta conversas ativas e marca atividade.
pub struct RequestGuard {
    motor: Arc<Motor>,
    pub id: u64,
    pub cancel: CancellationToken,
    ativo: bool,
}

impl RequestGuard {
    /// Marca que a conversa começou a usar o motor carregado.
    pub fn activate(&mut self) {
        if !self.ativo {
            self.ativo = true;
            self.motor.ativos.fetch_add(1, Ordering::SeqCst);
            self.motor.touch();
        }
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        if self.ativo {
            self.motor.ativos.fetch_sub(1, Ordering::SeqCst);
        }
        self.motor.requests.lock().unwrap().remove(&self.id);
        self.motor.touch();
        self.motor.bump();
    }
}

fn is_model_specific(motivo: &str) -> bool {
    motivo.contains("arquivo do modelo") || motivo.contains("não reconhece")
}

impl Motor {
    pub fn new(paths: Paths, opts: MotorOptions) -> Arc<Self> {
        crate::install_crypto_provider();
        let _ = paths.ensure();
        let mut settings: Settings = crate::paths::read_json(&paths.config_file()).unwrap_or_default();
        settings.sanitize();
        let manifest = opts.manifest_override.unwrap_or_else(|| EngineManifest::load(&paths.engines_manifest_override()));
        let catalog = opts.catalog_override.unwrap_or_else(|| Catalog::load(&paths.catalog_override()));
        let estado: EstadoPersistido = crate::paths::read_json(&paths.state_file()).unwrap_or_default();
        let registry = Registry::load(paths.models_registry());
        let (porta_tx, _) = watch::channel(settings.porta);
        let (mudancas, _) = watch::channel(0);
        Arc::new(Self {
            store: EngineStore::new(paths.engines_dir(), paths.downloads_dir()).with_extras(opts.extras_motor.clone()),
            http: download::http_client(),
            local: supervisor::local_client(),
            settings: RwLock::new(settings.clone()),
            manifest: RwLock::new(manifest),
            catalog: RwLock::new(catalog),
            detection: RwLock::new(opts.detection_override.clone()),
            detection_override: opts.detection_override,
            pastas_locais: opts.pastas_locais,
            audio_man: opts.audio_manifest_override.unwrap_or_else(crate::audio::ManifestoAudio::embutido),
            audio_store: crate::audio::AudioStore::new(paths.data_dir.join("audio"), paths.downloads_dir(), opts.extras_motor.clone()),
            audio_voz: tokio::sync::Mutex::new(None),
            audio_transcricao: tokio::sync::Mutex::new(None),
            audio_info: RwLock::new(HashMap::new()),
            audio_falhas: Mutex::new(HashSet::new()),
            audio_uso: Mutex::new(HashMap::new()),
            estado: Mutex::new(estado),
            registry: Mutex::new(registry),
            loaded: tokio::sync::Mutex::new(None),
            snapshot: RwLock::new(None),
            fase: RwLock::new((Fase::Parado, String::new())),
            paused: AtomicBool::new(false),
            avisos: Mutex::new(VecDeque::new()),
            velocidade: RwLock::new(None),
            ultimo_carregamento: RwLock::new(None),
            last_activity: AtomicU64::new(now_millis()),
            ativos: AtomicUsize::new(0),
            requests: Mutex::new(HashMap::new()),
            next_req: AtomicU64::new(1),
            tarefas: Mutex::new(HashMap::new()),
            session_failed: Mutex::new(HashSet::new()),
            update_info: RwLock::new(None),
            api_status: RwLock::new(Err("iniciando".into())),
            porta_tx,
            mudancas,
            launcher: opts.launcher,
            tick: opts.tick,
            load_timeout: opts.load_timeout,
            background: AtomicBool::new(false),
            bench_running: AtomicBool::new(false),
            auto_teste: opts.auto_teste,
            hf_token: RwLock::new(crate::hub::load_token(&paths.config_dir)),
            hf_nome: RwLock::new(None),
            shutdown: CancellationToken::new(),
            paths,
        })
    }

    // ---------- pequenos utilitários ----------

    fn bump(&self) {
        self.mudancas.send_modify(|v| *v = v.wrapping_add(1));
    }

    /// Avisa quem quiser saber (bandeja) que algo mudou.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.mudancas.subscribe()
    }

    pub fn porta_rx(&self) -> watch::Receiver<u16> {
        self.porta_tx.subscribe()
    }

    fn touch(&self) {
        self.last_activity.store(now_millis(), Ordering::SeqCst);
    }

    pub fn aviso(&self, nivel: &str, texto: impl Into<String>) {
        let texto = texto.into();
        match nivel {
            "erro" => tracing::error!("{texto}"),
            "aviso" => tracing::warn!("{texto}"),
            _ => tracing::info!("{texto}"),
        }
        let mut a = self.avisos.lock().unwrap();
        if a.len() >= 30 {
            a.pop_front();
        }
        a.push_back(Aviso { quando: now_unix(), nivel: nivel.into(), texto });
        drop(a);
        self.bump();
    }

    pub fn avisos(&self) -> Vec<Aviso> {
        self.avisos.lock().unwrap().iter().cloned().collect()
    }

    fn set_fase(&self, f: Fase, texto: impl Into<String>) {
        *self.fase.write().unwrap() = (f, texto.into());
        self.bump();
    }

    pub fn fase(&self) -> (Fase, String) {
        let f = self.fase.read().unwrap().clone();
        if self.paused.load(Ordering::SeqCst) && matches!(f.0, Fase::Parado | Fase::Pronto) {
            return (Fase::Pausado, "Pausado".into());
        }
        f
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn manifest(&self) -> EngineManifest {
        self.manifest.read().unwrap().clone()
    }

    pub fn catalog(&self) -> Catalog {
        self.catalog.read().unwrap().clone()
    }

    pub fn loaded_info(&self) -> Option<CarregadoInfo> {
        self.snapshot.read().unwrap().clone()
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn ativos(&self) -> usize {
        self.ativos.load(Ordering::SeqCst)
    }

    pub fn set_api_status(&self, s: Result<u16, String>) {
        *self.api_status.write().unwrap() = s;
        self.bump();
    }

    pub fn api_status(&self) -> Result<u16, String> {
        self.api_status.read().unwrap().clone()
    }

    pub fn installed_models(&self) -> Vec<InstalledModel> {
        self.registry.lock().unwrap().modelos.clone()
    }

    fn save_estado(&self) {
        let e = self.estado.lock().unwrap().clone();
        if let Err(err) = crate::paths::write_json(&self.paths.state_file(), &e) {
            tracing::warn!("não consegui gravar estado: {err}");
        }
    }

    // ---------- inicialização e vigia ----------

    /// Inicia as tarefas de fundo (detecção, vigia de quedas e de tempo parado).
    pub fn start_background(self: &Arc<Self>) {
        if self.background.swap(true, Ordering::SeqCst) {
            return;
        }
        // "abrir com o computador": ligado por padrão; a cada início regrava a
        // entrada com o caminho atual (o AppImage pode ter mudado de lugar).
        {
            let mut s = self.settings.write().unwrap();
            if s.abrir_com_computador {
                if let Some(l) = &self.launcher {
                    if let Err(e) = autostart::set(true, Some(l)) {
                        tracing::warn!("autostart: {e}");
                    }
                }
            }
            if !s.primeira_execucao_feita {
                s.primeira_execucao_feita = true;
                let _ = crate::paths::write_json(&self.paths.config_file(), &*s);
            }
        }
        let me = self.clone();
        tokio::spawn(async move {
            let det = me.detect_now().await;
            me.after_detection(&det).await;
        });
        let me = self.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(me.tick);
            iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = me.shutdown.cancelled() => break,
                    _ = iv.tick() => me.watch_once().await,
                }
            }
        });
    }

    /// Depois de detectar: se a placa/driver mudou e já temos tudo, testa de novo sozinho.
    async fn after_detection(self: &Arc<Self>, det: &Detection) {
        if !self.auto_teste {
            return;
        }
        let teste_atual = self.bench_is_current(det);
        let tem_teste_antigo = self.estado.lock().unwrap().teste.is_some();
        if teste_atual {
            return;
        }
        let plan = self.preparation_plan();
        if plan["total"].as_u64() == Some(0) && plan["motores"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            if tem_teste_antigo {
                self.aviso("info", "A placa ou o driver mudou. Testando os motores de novo…");
            }
            self.run_benchmark();
        } else if tem_teste_antigo {
            self.aviso("aviso", "A placa ou o driver mudou. Clique em \"Preparar\" para baixar o motor certo e testar de novo.");
        }
    }

    async fn watch_once(self: &Arc<Self>) {
        // 0) voz e transcrição: queda e descarregar quando parado
        self.vigiar_audio().await;
        // 1) queda do motor
        let mut crashed: Option<(CarregadoInfo, Vec<String>, Option<i32>)> = None;
        if let Ok(mut g) = self.loaded.try_lock() {
            if let Some(l) = g.as_mut() {
                if let Some(status) = l.server.exited() {
                    crashed = Some((l.info.clone(), l.server.log_tail(80), status.code()));
                    *g = None;
                }
            }
        }
        if let Some((info, log, code)) = crashed {
            *self.snapshot.write().unwrap() = None;
            self.set_fase(Fase::Parado, "");
            let motivo = supervisor::classify_failure(&log, code);
            self.mark_failed(&info.variante);
            let proximo = self.engine_order().into_iter().map(|(c, _)| c).find(|c| c.variante != info.variante);
            match &proximo {
                Some(p) => self.aviso("aviso", format!("O motor {} parou de repente ({motivo}) Passei para {}.", info.motor_nome, p.nome)),
                None => self.aviso("erro", format!("O motor {} parou de repente ({motivo})", info.motor_nome)),
            }
            if proximo.is_some() && !self.is_paused() {
                let me = self.clone();
                tokio::spawn(async move {
                    if let Err(e) = me.ensure_loaded(&info.modelo_id).await {
                        me.aviso("erro", format!("Não consegui recarregar o modelo: {e}"));
                    }
                });
            }
        }
        // 2) descarregar depois de um tempo parado
        let limite = self.settings().descarregar_apos_seg;
        if limite > 0 && self.ativos() == 0 && self.snapshot.read().unwrap().is_some() {
            let parado_ms = now_millis().saturating_sub(self.last_activity.load(Ordering::SeqCst));
            if parado_ms >= limite * 1000 {
                if let Ok(mut g) = self.loaded.try_lock() {
                    if let Some(mut l) = g.take() {
                        l.server.stop().await;
                        *self.snapshot.write().unwrap() = None;
                        self.set_fase(Fase::Parado, "");
                        let min = limite / 60;
                        let quanto = if min >= 1 { format!("{min} min") } else { format!("{limite} s") };
                        self.aviso(
                            "info",
                            format!("Modelo {} descarregado depois de {quanto} parado, para liberar a placa.", l.info.modelo_nome),
                        );
                    }
                }
            }
        }
        // 3) verificação semanal de atualizações (só se a pessoa ligou)
        if self.settings().procurar_atualizacoes_sozinho {
            let ultima = self.estado.lock().unwrap().ultima_verificacao;
            if now_unix().saturating_sub(ultima) > 7 * 24 * 3600 {
                self.estado.lock().unwrap().ultima_verificacao = now_unix();
                self.save_estado();
                let me = self.clone();
                tokio::spawn(async move {
                    let _ = me.check_updates().await;
                });
            }
        }
        // 4) limpa tarefas concluídas antigas
        let agora = now_unix();
        self.tarefas.lock().unwrap().retain(|_, t| !(t.estado == "concluida" && agora.saturating_sub(t.atualizado) > 120));
    }

    // ---------- detecção e motores ----------

    pub async fn detect_now(&self) -> Detection {
        if let Some(d) = &self.detection_override {
            return d.clone();
        }
        let libs = self.manifest().libs_to_probe(crate::engine::manifest::current_os());
        let mut det = gpu::detect(&libs).await;
        // VRAM desconhecida (ex.: NVIDIA no Flatpak): pergunta ao próprio motor instalado
        if det.placas.iter().any(|g| g.vram_total_mb.is_none() && g.fabricante != gpu::Vendor::Outro) {
            let exe = self
                .candidates_for(&det)
                .into_iter()
                .filter(|c| c.disponivel && c.backend.usa_placa() && c.backend != Backend::Proprio)
                .find_map(|c| self.engine_exe(&c.variante));
            if let Some(exe) = exe {
                let exe_s = exe.to_string_lossy().into_owned();
                if let Some(out) = gpu::run_tool_all(&exe_s, &["--list-devices"], Duration::from_secs(15)).await {
                    gpu::apply_llama_devices(&mut det.placas, &gpu::parse::parse_list_devices(&out));
                }
            }
        }
        *self.detection.write().unwrap() = Some(det.clone());
        self.bump();
        det
    }

    pub fn detection(&self) -> Option<Detection> {
        self.detection.read().unwrap().clone()
    }

    pub fn candidates(&self) -> Vec<Candidate> {
        let Some(det) = self.detection() else { return Vec::new() };
        self.candidates_for(&det)
    }

    fn candidates_for(&self, det: &Detection) -> Vec<Candidate> {
        let proprio = self.settings().llama_server_proprio;
        build_candidates(det, &self.manifest(), proprio.as_deref())
    }

    fn bench_is_current(&self, det: &Detection) -> bool {
        let fp = det.fingerprint();
        let versao = self.manifest().llama_cpp.versao;
        self.estado.lock().unwrap().teste.as_ref().map(|t| t.fingerprint == fp && t.llama_cpp == versao).unwrap_or(false)
    }

    fn mark_failed(&self, variante: &str) {
        self.session_failed.lock().unwrap().insert(variante.to_string());
        *self.estado.lock().unwrap().falhas.entry(variante.to_string()).or_insert(0) += 1;
        self.save_estado();
    }

    /// Executável de uma variante, se estiver instalada.
    pub fn engine_exe(&self, variante: &str) -> Option<PathBuf> {
        if variante == VARIANTE_PROPRIA {
            return self.settings().llama_server_proprio.filter(|p| p.is_file());
        }
        let m = self.manifest();
        let v = m.variant(variante)?;
        self.store.installed(&m, v).map(|e| e.executavel)
    }

    /// Candidatos disponíveis e instalados, na ordem em que serão tentados.
    pub fn engine_order(&self) -> Vec<(Candidate, PathBuf)> {
        let Some(det) = self.detection() else { return Vec::new() };
        let s = self.settings();
        let cands: Vec<Candidate> =
            build_candidates(&det, &self.manifest(), s.llama_server_proprio.as_deref()).into_iter().filter(|c| c.disponivel).collect();
        let ordem_teste =
            if self.bench_is_current(&det) { self.estado.lock().unwrap().teste.as_ref().map(|t| t.ordem.clone()) } else { None };
        let failed = self.session_failed.lock().unwrap().clone();
        type Chave = (bool, bool, u32, u8);
        let mut v: Vec<(Candidate, PathBuf, Chave)> = cands
            .into_iter()
            .filter_map(|c| {
                let exe = self.engine_exe(&c.variante)?;
                let pos = match &ordem_teste {
                    Some(o) => o.iter().position(|x| *x == c.variante).map(|p| p as u32).unwrap_or(2000),
                    None => c.backend.prioridade() as u32,
                };
                let forcado = s.motor_forcado.as_deref() == Some(c.variante.as_str());
                let key = (failed.contains(&c.variante), !forcado, pos, c.backend.prioridade());
                Some((c, exe, key))
            })
            .collect();
        v.sort_by_key(|x| x.2);
        v.into_iter().map(|(c, e, _)| (c, e)).collect()
    }

    /// Velocidades já medidas com este modelo nesta máquina (vazio se a placa mudou).
    pub fn medidas_do_modelo(&self, model_id: &str) -> HashMap<String, f64> {
        let Some(det) = self.detection() else { return HashMap::new() };
        let fp = det.fingerprint();
        self.estado.lock().unwrap().medidas.get(model_id).filter(|m| m.fingerprint == fp).map(|m| m.por_motor.clone()).unwrap_or_default()
    }

    /// Ordem dos motores para um modelo específico (veja `escolha`): a placa vem
    /// antes quando o modelo cabe nela, mesmo que o teste pequeno tenha dado o
    /// processador; medidas com o próprio modelo valem mais que tudo.
    pub fn engine_order_for(&self, model: &InstalledModel, det: &Detection) -> (Vec<(Candidate, PathBuf)>, crate::escolha::Motivo) {
        let base = self.engine_order();
        let s = self.settings();
        let teste = if self.bench_is_current(det) { self.bench() } else { None };
        let failed = self.session_failed.lock().unwrap().clone();
        let principal = det.principal().cloned();
        let shape = model.shape();
        let opcoes: Vec<crate::escolha::Opcao> = base
            .iter()
            .map(|(c, _)| {
                let pos = match &teste {
                    Some(t) => t.ordem.iter().position(|x| *x == c.variante).map(|p| p as u32).unwrap_or(2000),
                    None => c.backend.prioridade() as u32,
                };
                let usa_placa = c.backend.usa_placa() && principal.is_some();
                let fracao_na_placa = if !usa_placa {
                    0.0
                } else if c.backend == Backend::Proprio {
                    1.0
                } else {
                    let rec = Recursos {
                        usa_placa: true,
                        vram_livre_mb: principal.as_ref().and_then(|g| g.vram_livre_mb.or(g.vram_total_mb)),
                        ram_livre_mb: det.ram_livre_mb,
                    };
                    let p = budget::plan(&shape, &rec, s.contexto_preferido);
                    if p.todas_na_placa {
                        1.0
                    } else {
                        p.camadas_na_placa as f32 / shape.camadas.max(1) as f32
                    }
                };
                crate::escolha::Opcao {
                    variante: c.variante.clone(),
                    usa_placa,
                    forcado: s.motor_forcado.as_deref() == Some(c.variante.as_str()),
                    falhou: failed.contains(&c.variante),
                    pos_teste: pos,
                    prioridade: c.backend.prioridade(),
                    fracao_na_placa,
                }
            })
            .collect();
        let tamanho_teste =
            teste.as_ref().map(|t| t.tamanho_modelo.or_else(|| self.catalog().test_model().map(|m| m.tamanho)).unwrap_or(0));
        let (ordem, motivo) = crate::escolha::ordenar(&opcoes, model.tamanho, tamanho_teste, &self.medidas_do_modelo(&model.id));
        let mut v = base;
        v.sort_by_key(|(c, _)| ordem.iter().position(|x| *x == c.variante).unwrap_or(usize::MAX));
        (v, motivo)
    }

    pub fn force_engine(&self, variante: Option<String>) -> Result<(), MotorError> {
        if let Some(v) = &variante {
            if !self.candidates().iter().any(|c| &c.variante == v) {
                return Err(MotorError::Invalido(format!("Motor \"{v}\" não existe para esta máquina.")));
            }
        }
        {
            let mut s = self.settings.write().unwrap();
            s.motor_forcado = variante.clone();
            let _ = crate::paths::write_json(&self.paths.config_file(), &*s);
        }
        if let Some(v) = &variante {
            self.session_failed.lock().unwrap().remove(v);
        }
        self.bump();
        Ok(())
    }

    // ---------- carregar e descarregar ----------

    /// Garante que `model_id` esteja carregado, tentando os motores em ordem.
    pub async fn ensure_loaded(self: &Arc<Self>, model_id: &str) -> Result<CarregadoInfo, MotorError> {
        if self.is_paused() {
            return Err(MotorError::Pausado);
        }
        // durante o teste de velocidade a placa está ocupada: espera terminar
        let t0 = std::time::Instant::now();
        while self.bench_running.load(Ordering::SeqCst) {
            if t0.elapsed() > Duration::from_secs(240) {
                return Err(MotorError::Ocupado("O teste de velocidade ainda está rodando. Tente de novo em instantes.".into()));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let mut guard = self.loaded.lock().await;
        if self.is_paused() {
            return Err(MotorError::Pausado);
        }
        if let Some(l) = guard.as_mut() {
            // motor escolhido à mão diferente do que está rodando: troca no próximo pedido
            let forcado = self.settings().motor_forcado;
            let trocar_motor = forcado
                .as_deref()
                .is_some_and(|f| f != l.info.variante && self.engine_exe(f).is_some() && !self.session_failed.lock().unwrap().contains(f));
            if l.info.modelo_id == model_id && l.server.exited().is_none() && !trocar_motor {
                self.touch();
                return Ok(l.info.clone());
            }
            // outro modelo (ou outro motor): espera as conversas em andamento terminarem
            let t0 = std::time::Instant::now();
            while self.ativos() > 0 {
                if t0.elapsed() > Duration::from_secs(120) {
                    return Err(MotorError::Ocupado("Outro modelo está respondendo agora. Tente de novo em instantes.".into()));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        if let Some(mut l) = guard.take() {
            l.server.stop().await;
            *self.snapshot.write().unwrap() = None;
        }
        let model = self.registry.lock().unwrap().get(model_id).cloned().ok_or_else(|| MotorError::ModeloNaoEncontrado(model_id.into()))?;
        if !model.existe() {
            return Err(MotorError::ArquivoSumiu(model.nome));
        }
        let inicio = std::time::Instant::now();
        let det = self.detect_now().await;
        let deteccao_s = inicio.elapsed().as_secs_f64();
        let (order, motivo) = self.engine_order_for(&model, &det);
        if order.is_empty() {
            return Err(MotorError::SemMotor);
        }
        tracing::info!(
            "carregando {}: ordem {:?} ({}), detecção {:.1}s",
            model.id,
            order.iter().map(|(c, _)| c.variante.as_str()).collect::<Vec<_>>(),
            motivo.texto(),
            deteccao_s
        );
        let mut etapas: Vec<Value> = Vec::new();
        let registrar = |me: &Self, ok: bool, variante: Option<&str>, etapas: &[Value]| {
            let total = inicio.elapsed().as_secs_f64();
            let r = json!({
                "modelo_id": model.id, "modelo": model.nome, "ok": ok, "variante": variante,
                "segundos_total": (total * 10.0).round() / 10.0,
                "segundos_deteccao": (deteccao_s * 10.0).round() / 10.0,
                "tentativas": etapas, "quando": now_unix(),
            });
            tracing::info!("carregamento de {}: {}", model.id, r);
            *me.ultimo_carregamento.write().unwrap() = Some(r);
        };
        let principal = det.principal().cloned();
        let contexto = self.settings().contexto_preferido;
        let mut tentativas: Vec<String> = Vec::new();
        for (i, (cand, exe)) in order.iter().enumerate() {
            let rec = Recursos {
                usa_placa: cand.backend.usa_placa() && principal.is_some(),
                vram_livre_mb: principal.as_ref().and_then(|g| g.vram_livre_mb.or(g.vram_total_mb)),
                ram_livre_mb: det.ram_livre_mb,
            };
            let mut plano = budget::plan(&model.shape(), &rec, contexto);
            if cand.backend == Backend::Proprio {
                // motor próprio: não sabemos se usa placa; manda tudo e deixa o motor decidir
                plano.todas_na_placa = rec.vram_livre_mb.is_some();
            }
            let mut reduziu = false;
            loop {
                self.set_fase(Fase::Carregando, format!("Carregando {} no motor {}…", model.nome, cand.nome));
                let spec = supervisor::LaunchSpec {
                    exe: exe.clone(),
                    modelo: model.arquivo.clone(),
                    alias: model.id.clone(),
                    contexto: plano.contexto,
                    ngl: plano.ngl_arg(),
                    embeddings: model.embeddings,
                    backend: cand.backend,
                    variante: cand.variante.clone(),
                    args_extras: vec![],
                    mmproj: model.mmproj.clone().filter(|p| p.is_file()),
                    args_proprios: None,
                };
                let t_tentativa = std::time::Instant::now();
                let resultado = supervisor::start(&spec, &self.local, self.load_timeout).await;
                let seg = (t_tentativa.elapsed().as_secs_f64() * 10.0).round() / 10.0;
                etapas.push(json!({
                    "variante": cand.variante, "motor": cand.nome, "segundos": seg, "ok": resultado.is_ok(),
                    "camadas_na_placa": plano.ngl_arg(), "contexto": plano.contexto,
                    "erro": resultado.as_ref().err().map(|e| e.to_string()),
                }));
                match resultado {
                    Ok(server) => {
                        let info = CarregadoInfo {
                            modelo_id: model.id.clone(),
                            modelo_nome: model.nome.clone(),
                            variante: cand.variante.clone(),
                            backend: cand.backend,
                            motor_nome: cand.nome.clone(),
                            base_url: server.base_url.clone(),
                            plano: plano.clone(),
                            embeddings: model.embeddings,
                            desde: now_unix(),
                            motivo: if i == 0 { motivo.texto().to_string() } else { format!("o motor {} não funcionou", order[0].0.nome) },
                        };
                        registrar(self, true, Some(&cand.variante), &etapas);
                        *guard = Some(Loaded { server, info: info.clone() });
                        *self.snapshot.write().unwrap() = Some(info.clone());
                        self.touch();
                        self.set_fase(Fase::Pronto, format!("{} pronto no motor {}", model.nome, cand.nome));
                        if i > 0 {
                            self.aviso("aviso", format!("O motor {} não funcionou; usando {}.", order[0].0.nome, cand.nome));
                        }
                        if let Some(a) = &plano.aviso {
                            self.aviso("info", a.clone());
                        }
                        return Ok(info);
                    }
                    Err(e) => {
                        let motivo = e.to_string();
                        tracing::warn!("motor {} falhou: {motivo}; registro: {:?}", cand.variante, e.log());
                        if motivo.contains("sem memória") && !reduziu && cand.backend.usa_placa() && plano.camadas_na_placa > 1 {
                            reduziu = true;
                            plano.todas_na_placa = false;
                            plano.camadas_na_placa /= 2;
                            plano.contexto = plano.contexto.min(4096);
                            self.aviso(
                                "aviso",
                                format!("A placa ficou sem memória; tentando com menos camadas na placa ({}).", plano.camadas_na_placa),
                            );
                            continue;
                        }
                        if is_model_specific(&motivo) {
                            self.set_fase(Fase::Parado, "");
                            registrar(self, false, None, &etapas);
                            return Err(MotorError::Falhou(motivo));
                        }
                        self.mark_failed(&cand.variante);
                        tentativas.push(format!("{}: {motivo}", cand.nome));
                        if let Some((next, _)) = order.get(i + 1) {
                            self.aviso("aviso", format!("O motor {} não subiu ({motivo}) Tentando {}…", cand.nome, next.nome));
                        }
                        break;
                    }
                }
            }
        }
        self.set_fase(Fase::Parado, "");
        registrar(self, false, None, &etapas);
        Err(MotorError::Falhou(format!("Nenhum motor conseguiu abrir o modelo. {}", tentativas.join(" | "))))
    }

    /// Descarrega o modelo (libera a placa).
    pub async fn unload(&self) -> bool {
        let mut g = self.loaded.lock().await;
        let had = if let Some(mut l) = g.take() {
            l.server.stop().await;
            true
        } else {
            false
        };
        drop(g);
        *self.snapshot.write().unwrap() = None;
        if !matches!(self.fase.read().unwrap().0, Fase::Testando | Fase::Preparando) {
            self.set_fase(Fase::Parado, "");
        }
        self.bump();
        had
    }

    pub async fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        self.stop_requests(None);
        self.unload().await;
        self.descarregar_audio(None).await;
        self.aviso("info", "Ignis pausado: o modelo foi descarregado e novos pedidos serão recusados até você retomar.");
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.aviso("info", "Ignis retomado.");
    }

    /// Escolhe o modelo para um pedido.
    pub fn resolve_model(&self, pedido: Option<&str>, embeddings: bool) -> Result<String, MotorError> {
        let reg = self.registry.lock().unwrap();
        let pedido = pedido.map(str::trim).filter(|s| !s.is_empty() && *s != "default" && *s != "padrao");
        let m = match pedido {
            Some(p) => reg
                .get(p)
                .or_else(|| reg.modelos.iter().find(|m| m.nome.eq_ignore_ascii_case(p)))
                .or_else(|| reg.modelos.iter().find(|m| m.id.eq_ignore_ascii_case(p)))
                .ok_or_else(|| MotorError::ModeloNaoEncontrado(p.into()))?,
            None => {
                let carregado = self.loaded_info().filter(|i| i.embeddings == embeddings).and_then(|i| reg.get(&i.modelo_id));
                let padrao = self.settings().modelo_padrao.and_then(|id| reg.get(&id).filter(|m| m.embeddings == embeddings));
                carregado.or(padrao).or_else(|| reg.modelos.iter().find(|m| m.embeddings == embeddings && m.existe())).ok_or(
                    if embeddings {
                        MotorError::Invalido(
                            "Nenhum modelo de embeddings instalado. Baixe o \"Qwen3 Embedding 0.6B\" na seção Modelos.".into(),
                        )
                    } else {
                        MotorError::SemModelo
                    },
                )?
            }
        };
        if m.embeddings != embeddings {
            return Err(MotorError::Invalido(if embeddings {
                format!("O modelo \"{}\" não gera embeddings. Use um modelo de embeddings (ex.: Qwen3 Embedding 0.6B).", m.nome)
            } else {
                format!("O modelo \"{}\" só gera embeddings; ele não conversa. Use /v1/embeddings ou escolha outro modelo.", m.nome)
            }));
        }
        Ok(m.id.clone())
    }

    pub fn begin_request(self: &Arc<Self>) -> RequestGuard {
        let id = self.next_req.fetch_add(1, Ordering::SeqCst);
        let cancel = CancellationToken::new();
        self.requests.lock().unwrap().insert(id, cancel.clone());
        self.touch();
        RequestGuard { motor: self.clone(), id, cancel, ativo: false }
    }

    /// Para uma conversa (ou todas). Devolve quantas foram paradas.
    pub fn stop_requests(&self, id: Option<u64>) -> usize {
        let r = self.requests.lock().unwrap();
        let mut n = 0;
        for (k, c) in r.iter() {
            if id.is_none() || id == Some(*k) {
                c.cancel();
                n += 1;
            }
        }
        n
    }

    /// Guarda a velocidade da última resposta. Com `gerados` suficientes, também
    /// anota a velocidade DESTE modelo NESTE motor, que passa a pesar na escolha.
    pub fn record_speed(&self, leitura: Option<f64>, geracao: Option<f64>, gerados: Option<u64>) {
        if leitura.is_none() && geracao.is_none() {
            return;
        }
        *self.velocidade.write().unwrap() = Some(Velocidade {
            leitura_tps: leitura.map(|v| (v * 10.0).round() / 10.0),
            geracao_tps: geracao.map(|v| (v * 10.0).round() / 10.0),
            quando: now_unix(),
        });
        if let (Some(g), Some(n), Some(info), Some(det)) = (geracao, gerados, self.loaded_info(), self.detection()) {
            if n >= MIN_TOKENS_MEDIDA && g > 0.0 && !info.embeddings {
                self.anotar_medida(&info.modelo_id, &det.fingerprint(), &info.variante, g, None);
            }
        }
        self.bump();
    }

    fn anotar_medida(&self, model_id: &str, fingerprint: &str, variante: &str, tps: f64, teste: Option<Vec<BenchResult>>) {
        {
            let mut e = self.estado.lock().unwrap();
            let m = e.medidas.entry(model_id.to_string()).or_default();
            if m.fingerprint != fingerprint {
                *m = MedidasModelo { fingerprint: fingerprint.to_string(), ..Default::default() };
            }
            let novo = match m.por_motor.get(variante) {
                // suaviza: uma resposta curta não muda tudo de uma vez
                Some(antigo) if teste.is_none() => 0.6 * antigo + 0.4 * tps,
                _ => tps,
            };
            m.por_motor.insert(variante.to_string(), (novo * 10.0).round() / 10.0);
            m.quando = now_unix();
            if teste.is_some() {
                m.teste = teste;
            }
        }
        self.save_estado();
    }

    /// Últimas linhas do registro do llama-server em uso.
    pub fn server_log(&self, n: usize) -> Vec<String> {
        match self.loaded.try_lock() {
            Ok(g) => g.as_ref().map(|l| l.server.log_tail(n)).unwrap_or_default(),
            Err(_) => vec!["(o motor está carregando um modelo agora)".into()],
        }
    }

    pub fn http_local(&self) -> &reqwest::Client {
        &self.local
    }

    // ---------- tarefas (downloads, importações) ----------

    fn task_begin(&self, id: &str, tipo: &str, nome: &str, total: Option<u64>) -> Result<CancellationToken, MotorError> {
        let mut t = self.tarefas.lock().unwrap();
        if let Some(x) = t.get(id) {
            if x.estado == "andamento" {
                return Err(MotorError::Ocupado(format!("{nome} já está em andamento.")));
            }
        }
        let cancel = CancellationToken::new();
        t.insert(
            id.to_string(),
            Tarefa {
                id: id.into(),
                tipo: tipo.into(),
                nome: nome.into(),
                baixado: 0,
                total,
                estado: "andamento".into(),
                erro: None,
                atualizado: now_unix(),
                cancel: cancel.clone(),
            },
        );
        drop(t);
        self.bump();
        Ok(cancel)
    }

    fn task_progress(&self, id: &str, p: Progress) {
        if let Some(t) = self.tarefas.lock().unwrap().get_mut(id) {
            t.baixado = p.baixado;
            if p.total.is_some() {
                t.total = p.total;
            }
            t.atualizado = now_unix();
        }
        self.bump();
    }

    fn task_finish(&self, id: &str, estado: &str, erro: Option<String>) {
        if let Some(t) = self.tarefas.lock().unwrap().get_mut(id) {
            t.estado = estado.into();
            t.erro = erro;
            t.atualizado = now_unix();
        }
        self.bump();
    }

    pub fn tarefas(&self) -> Vec<Tarefa> {
        let mut v: Vec<Tarefa> = self.tarefas.lock().unwrap().values().cloned().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    pub fn cancel_task(&self, id: &str) -> bool {
        let t = self.tarefas.lock().unwrap();
        // aceita o id da tarefa ou o id do modelo do catálogo
        let key = if t.contains_key(id) { id.to_string() } else { format!("modelo:{id}") };
        if let Some(x) = t.get(&key) {
            x.cancel.cancel();
            return true;
        }
        false
    }

    /// Baixa um modelo do catálogo (retoma se já houver parte baixada).
    fn pastas_locais(&self) -> PastasLocais {
        self.pastas_locais.clone().unwrap_or_else(PastasLocais::do_sistema)
    }

    /// Modelos GGUF que outros programas já baixaram neste computador.
    pub fn modelos_no_computador(&self) -> Vec<crate::locais::ModeloLocal> {
        let p = self.pastas_locais();
        let mut v: Vec<crate::locais::ModeloLocal> = crate::ollama::scan_all(&p.ollama)
            .into_iter()
            .map(|o| crate::locais::ModeloLocal {
                fonte: crate::locais::Fonte::Ollama,
                tamanho: o.tamanho,
                nome: o.referencia,
                caminho: o.caminho,
            })
            .collect();
        if let Some(hf) = &p.hugging_face {
            v.extend(crate::locais::ggufs_hugging_face(hf));
        }
        v.extend(crate::locais::ggufs_lm_studio(&p.lm_studio));
        let usados: Vec<PathBuf> = self.registry.lock().unwrap().modelos.iter().map(|m| m.arquivo.clone()).collect();
        v.retain(|m| !usados.contains(&m.caminho));
        v
    }

    /// Se o mesmo arquivo de um modelo do catálogo já foi baixado por outro
    /// programa (mesmo SHA-256), registra ele em vez de baixar de novo.
    /// Tenta um link físico na pasta do Ignis (sem gastar espaço e sem depender
    /// do outro programa); se não der (outro disco, Flatpak), lê no lugar.
    fn reaproveitar_do_computador(&self, m: &crate::catalog::CatalogModel) -> Option<String> {
        let p = self.pastas_locais();
        let local = crate::locais::achar_por_sha256(&m.sha256, &p.ollama, p.hugging_face.as_deref())?;
        if local.tamanho != m.tamanho {
            return None;
        }
        let dest = self.paths.models_dir().join(&m.arquivo);
        let (arquivo, origem) = if !dest.exists() && std::fs::hard_link(&local.caminho, &dest).is_ok() {
            (dest, Origem::Catalogo { id: m.id.clone() })
        } else {
            (local.caminho.clone(), Origem::NoComputador { fonte: local.fonte.nome().to_string() })
        };
        let mut im = InstalledModel::from_file(m.id.clone(), Some(m.nome.clone()), arquivo, origem, Some(m.sha256.clone())).ok()?;
        im.licenca = Some(m.licenca.clone());
        im.embeddings = im.embeddings || m.embeddings();
        self.registry.lock().unwrap().add(im).ok()?;
        self.bump();
        self.aviso("info", format!("{} já estava no computador ({}): usei de lá, sem baixar de novo.", m.nome, local.fonte.nome()));
        Some(m.id.clone())
    }

    /// Registra (lido no lugar) um GGUF que outro programa já baixou.
    fn registrar_local(&self, l: &crate::locais::ModeloLocal) -> Result<InstalledModel, MotorError> {
        // "dono/repo · arquivo.gguf" ou "pasta/arquivo.gguf": o nome do arquivo identifica o modelo
        let arquivo = l.nome.rsplit(['·', '/']).next().unwrap_or(&l.nome).trim();
        let stem = arquivo.trim_end_matches(".gguf");
        let id = format!("local/{}", crate::util::safe_file_name(stem));
        let im = InstalledModel::from_file(
            id,
            Some(stem.to_string()),
            l.caminho.clone(),
            Origem::NoComputador { fonte: l.fonte.nome().into() },
            None,
        )
        .map_err(|e| MotorError::Falhou(e.to_string()))?;
        self.registry.lock().unwrap().add(im.clone()).map_err(|e| MotorError::Falhou(e.to_string()))?;
        self.bump();
        Ok(im)
    }

    /// Registra um GGUF pequeno que já está no computador para o teste de velocidade.
    fn usar_modelo_pequeno_do_computador(self: &Arc<Self>) -> Option<String> {
        let mut locais: Vec<crate::locais::ModeloLocal> =
            self.modelos_no_computador().into_iter().filter(|m| m.tamanho >= 50_000_000 && m.tamanho <= TAMANHO_MAX_TESTE).collect();
        locais.sort_by_key(|m| m.tamanho);
        for l in locais {
            // modelos de embeddings não servem para o teste
            if crate::gguf::ModelInfo::read(&l.caminho).map(|i| i.embeddings).unwrap_or(true) {
                continue;
            }
            if let Ok(im) = self.registrar_local(&l) {
                self.aviso(
                    "info",
                    format!("Para o teste, usei {} que já estava no computador ({}), sem baixar nada.", l.nome, l.fonte.nome()),
                );
                return Some(im.id);
            }
        }
        None
    }

    pub fn start_model_download(self: &Arc<Self>, catalog_id: &str) -> Result<String, MotorError> {
        let m = self
            .catalog()
            .get(catalog_id)
            .cloned()
            .ok_or_else(|| MotorError::Invalido(format!("\"{catalog_id}\" não está no catálogo.")))?;
        if self.registry.lock().unwrap().get(&m.id).map(|x| x.existe()).unwrap_or(false) {
            return Err(MotorError::Invalido(format!("{} já está instalado.", m.nome)));
        }
        if let Some(id) = self.reaproveitar_do_computador(&m) {
            return Ok(id);
        }
        let tid = format!("modelo:{}", m.id);
        let cancel = self.task_begin(&tid, "modelo", &m.nome, Some(m.tamanho))?;
        let me = self.clone();
        let tid2 = tid.clone();
        tokio::spawn(async move {
            let r = me.download_catalog_model(&m, &tid2, &cancel).await;
            match r {
                Ok(()) => {
                    me.task_finish(&tid2, "concluida", None);
                    me.aviso("info", format!("{} baixado e pronto para usar.", m.nome));
                }
                Err(DownloadError::Cancelled) => me.task_finish(&tid2, "pausada", None),
                Err(e) => {
                    me.task_finish(&tid2, "erro", Some(e.to_string()));
                    me.aviso("erro", format!("Download de {} falhou: {e}", m.nome));
                }
            }
        });
        Ok(tid)
    }

    async fn download_catalog_model(
        &self,
        m: &crate::catalog::CatalogModel,
        tid: &str,
        cancel: &CancellationToken,
    ) -> Result<(), DownloadError> {
        let dest = self.paths.models_dir().join(&m.arquivo);
        let cb = |p: Progress| self.task_progress(tid, p);
        let token = self.hf_token();
        download::download_file(&self.http, &m.download_url(), &dest, Some(&m.sha256), Some(m.tamanho), cancel, &cb, token.as_deref())
            .await?;
        let mut im = InstalledModel::from_file(
            m.id.clone(),
            Some(m.nome.clone()),
            dest,
            Origem::Catalogo { id: m.id.clone() },
            Some(m.sha256.clone()),
        )
        .map_err(|e| DownloadError::Io(std::io::Error::other(e.to_string())))?;
        im.licenca = Some(m.licenca.clone());
        im.embeddings = im.embeddings || m.embeddings();
        self.registry.lock().unwrap().add(im)?;
        self.bump();
        Ok(())
    }

    /// Remove um modelo (ou um download incompleto do catálogo).
    pub async fn remove_model(&self, id: &str) -> Result<String, MotorError> {
        if self.loaded_info().map(|i| i.modelo_id == id).unwrap_or(false) {
            self.unload().await;
        }
        let r = self.registry.lock().unwrap().remove(id, &self.paths.models_dir()).map_err(|e| MotorError::Falhou(e.to_string()))?;
        let msg = match r {
            Some(RemoveOutcome::Apagado) => "Modelo apagado do disco.".to_string(),
            Some(RemoveOutcome::SoDaLista) => "Modelo tirado da lista (o arquivo original foi mantido).".to_string(),
            None if id.starts_with("hf:") => {
                // download incompleto de um repositório do Hugging Face
                self.cancel_task(id);
                let resto = &id[3..];
                let mut it = resto.splitn(3, '/');
                let (Some(dono), Some(nome), Some(arquivo)) = (it.next(), it.next(), it.next()) else {
                    return Err(MotorError::ModeloNaoEncontrado(id.into()));
                };
                let pasta = self.paths.models_dir().join("hf").join(format!("{dono}__{nome}"));
                let base = arquivo.rsplit('/').next().unwrap_or(arquivo).trim_end_matches(".gguf").to_string();
                let prefixo = crate::util::safe_file_name(&base);
                let prefixo = prefixo.split("-00001-of-").next().unwrap_or(&prefixo).to_string();
                if let Ok(rd) = std::fs::read_dir(&pasta) {
                    for e in rd.flatten() {
                        let n = e.file_name().to_string_lossy().into_owned();
                        if n.starts_with(&prefixo) && n.ends_with(".part") {
                            let _ = std::fs::remove_file(e.path());
                        }
                    }
                }
                self.tarefas.lock().unwrap().remove(id);
                "Download incompleto apagado.".to_string()
            }
            None => {
                // download incompleto do catálogo?
                let cat = self.catalog();
                let Some(m) = cat.get(id) else { return Err(MotorError::ModeloNaoEncontrado(id.into())) };
                self.cancel_task(&format!("modelo:{id}"));
                download::remove_partial(&self.paths.models_dir().join(&m.arquivo));
                self.tarefas.lock().unwrap().remove(&format!("modelo:{id}"));
                "Download incompleto apagado.".to_string()
            }
        };
        if self.settings().modelo_padrao.as_deref() == Some(id) {
            let mut s = self.settings.write().unwrap();
            s.modelo_padrao = None;
            let _ = crate::paths::write_json(&self.paths.config_file(), &*s);
        }
        self.bump();
        Ok(msg)
    }

    // ---------- Hugging Face: qualquer modelo ----------

    pub fn hf_token(&self) -> Option<String> {
        self.hf_token.read().unwrap().clone()
    }

    pub fn hf_conta(&self) -> Value {
        json!({"configurado": self.hf_token().is_some(), "nome": *self.hf_nome.read().unwrap()})
    }

    /// Salva (ou remove, com texto vazio) o token da pessoa, conferindo antes.
    pub async fn set_hf_token(&self, token: &str) -> Result<Value, MotorError> {
        let t = token.trim();
        if t.is_empty() {
            crate::hub::save_token(&self.paths.config_dir, None).map_err(|e| MotorError::Falhou(e.to_string()))?;
            *self.hf_token.write().unwrap() = None;
            *self.hf_nome.write().unwrap() = None;
            self.bump();
            return Ok(self.hf_conta());
        }
        let nome = crate::hub::whoami(&self.http, t).await.map_err(|e| MotorError::Invalido(e.to_string()))?;
        crate::hub::save_token(&self.paths.config_dir, Some(t)).map_err(|e| MotorError::Falhou(e.to_string()))?;
        *self.hf_token.write().unwrap() = Some(t.to_string());
        *self.hf_nome.write().unwrap() = Some(nome.clone());
        self.aviso("info", format!("Conta do Hugging Face conectada ({nome})."));
        Ok(self.hf_conta())
    }

    fn hub_err(e: crate::hub::HubError) -> MotorError {
        match e {
            crate::hub::HubError::NaoEncontrado => MotorError::ModeloNaoEncontrado("(Hugging Face)".into()),
            crate::hub::HubError::PedeAceite { .. } | crate::hub::HubError::TokenRecusado => MotorError::Invalido(e.to_string()),
            other => MotorError::Falhou(other.to_string()),
        }
    }

    pub async fn hub_search(&self, q: &str) -> Result<Vec<crate::hub::HubModel>, MotorError> {
        if q.trim().len() < 2 {
            return Err(MotorError::Invalido("Escreva pelo menos 2 letras para buscar.".into()));
        }
        crate::hub::search(&self.http, self.hf_token().as_deref(), q, 30).await.map_err(Self::hub_err)
    }

    pub async fn hub_repo(&self, repo: &str) -> Result<crate::hub::HubRepo, MotorError> {
        crate::hub::files(&self.http, self.hf_token().as_deref(), repo).await.map_err(Self::hub_err)
    }

    /// Lê o cabeçalho do arquivo no Hugging Face e diz se cabe na placa.
    pub async fn hub_info(&self, repo: &str, revisao: &str, arquivo: &str, rec: &Recursos) -> Result<Value, MotorError> {
        let info = crate::hub::header_info(&self.http, self.hf_token().as_deref(), repo, revisao, arquivo).await.map_err(Self::hub_err)?;
        let r = self.hub_repo(repo).await?;
        let f = r.arquivos.iter().find(|f| f.partes.first().map(|p| p.nome.as_str()) == Some(arquivo) || f.nome == arquivo);
        let tamanho = f.map(|f| f.tamanho).unwrap_or(0);
        let shape = budget::ModelShape {
            tamanho_arquivo: tamanho,
            camadas: info.camadas,
            kv_bytes_por_token: info.kv_bytes_por_token(),
            contexto_max: info.contexto_treino,
            embd: info.embd,
        };
        let plano = budget::plan(&shape, rec, self.settings().contexto_preferido);
        Ok(json!({"info": info, "plano": plano, "rotulo": plano.encaixe.rotulo()}))
    }

    /// Baixa um modelo (todas as partes) de qualquer repositório, com o mmproj se pedido.
    pub fn start_hub_download(self: &Arc<Self>, repo: String, arquivo: String, com_mmproj: Option<String>) -> Result<String, MotorError> {
        if !crate::hub::valid_repo(&repo) {
            return Err(MotorError::Invalido("Repositório inválido.".into()));
        }
        let tid = format!("hf:{repo}/{arquivo}");
        let nome_curto = arquivo.rsplit('/').next().unwrap_or(&arquivo).trim_end_matches(".gguf").to_string();
        let cancel = self.task_begin(&tid, "modelo", &nome_curto, None)?;
        let tid_out = tid.clone();
        let me = self.clone();
        tokio::spawn(async move {
            let r = me.hub_download_inner(&repo, &arquivo, com_mmproj.as_deref(), &tid, &cancel).await;
            match r {
                Ok(nome) => {
                    me.task_finish(&tid, "concluida", None);
                    me.aviso("info", format!("{nome} baixado e pronto para usar."));
                }
                Err(MotorError::Ocupado(_)) => me.task_finish(&tid, "pausada", None),
                Err(e) => {
                    me.task_finish(&tid, "erro", Some(e.to_string()));
                    me.aviso("erro", format!("Download de {nome_curto} falhou: {e}"));
                }
            }
        });
        Ok(tid_out)
    }

    async fn hub_download_inner(
        &self,
        repo: &str,
        arquivo: &str,
        com_mmproj: Option<&str>,
        tid: &str,
        cancel: &CancellationToken,
    ) -> Result<String, MotorError> {
        let r = self.hub_repo(repo).await?;
        let f = r
            .arquivos
            .iter()
            .find(|f| f.nome == arquivo || f.partes.first().map(|p| p.nome.as_str()) == Some(arquivo))
            .cloned()
            .ok_or_else(|| MotorError::Invalido(format!("O arquivo {arquivo} não está mais no repositório.")))?;
        let mm = com_mmproj.and_then(|n| r.mmproj.iter().find(|p| p.nome == n).cloned());
        let pasta = self.paths.models_dir().join("hf").join(repo.replace('/', "__"));
        let total: u64 = f.tamanho + mm.as_ref().map(|p| p.tamanho).unwrap_or(0);
        let token = self.hf_token();
        let mut feito = 0u64;
        let mut locais: Vec<PathBuf> = Vec::new();
        for parte in f.partes.iter().chain(mm.iter()) {
            let base = parte.nome.rsplit('/').next().unwrap_or(&parte.nome).to_string();
            let dest = pasta.join(crate::util::safe_file_name(&base));
            let antes = feito;
            let cb = |p: Progress| self.task_progress(tid, Progress { baixado: antes + p.baixado, total: Some(total) });
            let url = crate::hub::file_url(repo, &r.revisao, &parte.nome);
            if !dest.exists() {
                match download::download_file(
                    &self.http,
                    &url,
                    &dest,
                    parte.sha256.as_deref(),
                    Some(parte.tamanho),
                    cancel,
                    &cb,
                    token.as_deref(),
                )
                .await
                {
                    Ok(()) => {}
                    Err(DownloadError::Cancelled) => return Err(MotorError::Ocupado("cancelado".into())),
                    Err(DownloadError::Http(401 | 403)) => {
                        return Err(MotorError::Invalido(crate::hub::HubError::PedeAceite { repo: repo.into() }.to_string()))
                    }
                    Err(e) => return Err(MotorError::Falhou(e.to_string())),
                }
            }
            feito += parte.tamanho;
            locais.push(dest);
        }
        let mmproj_local = if mm.is_some() { locais.pop() } else { None };
        let principal = locais.first().cloned().ok_or_else(|| MotorError::Falhou("nada baixado".into()))?;
        let stem = f.nome.rsplit('/').next().unwrap_or(&f.nome).trim_end_matches(".gguf").to_string();
        let id = format!("hf/{repo}/{stem}");
        let nome = match &f.quantizacao {
            Some(q) => format!("{} ({q})", repo.rsplit('/').next().unwrap_or(repo).trim_end_matches("-GGUF").trim_end_matches("-gguf")),
            None => stem.clone(),
        };
        let mut im = InstalledModel::from_file(
            id,
            Some(nome.clone()),
            principal,
            Origem::HuggingFace { repo: repo.into(), arquivo: f.nome.clone(), revisao: r.revisao.clone() },
            f.partes.first().and_then(|p| p.sha256.clone()),
        )
        .map_err(|e| MotorError::Falhou(e.to_string()))?;
        im.partes = locais.into_iter().skip(1).collect();
        im.mmproj = mmproj_local;
        im.tamanho = total;
        im.licenca = r.licenca.clone();
        self.registry.lock().unwrap().add(im).map_err(|e| MotorError::Falhou(e.to_string()))?;
        self.bump();
        Ok(nome)
    }

    // ---------- Ollama e arquivos locais ----------

    pub fn ollama_scan(&self) -> Vec<Value> {
        let reg = self.registry.lock().unwrap();
        ollama::scan_all(&[])
            .into_iter()
            .map(|m| {
                let id = ollama::model_id(&m.referencia);
                let ja = reg.get(&id).is_some();
                json!({
                    "referencia": m.referencia,
                    "id": id,
                    "caminho": m.caminho,
                    "tamanho": m.tamanho,
                    "pasta": m.pasta_ollama,
                    "ja_importado": ja,
                })
            })
            .collect()
    }

    /// Importa um modelo do Ollama: "usar" (lê no lugar) ou "copiar".
    pub fn import_ollama(self: &Arc<Self>, referencia: &str, modo: &str, pastas_extras: &[PathBuf]) -> Result<String, MotorError> {
        let m = ollama::scan_all(pastas_extras)
            .into_iter()
            .find(|m| m.referencia == referencia)
            .ok_or_else(|| MotorError::Invalido(format!("Não achei \"{referencia}\" na pasta do Ollama.")))?;
        let id = ollama::model_id(&m.referencia);
        match modo {
            "usar" => {
                let im = InstalledModel::from_file(
                    id.clone(),
                    Some(m.referencia.clone()),
                    m.caminho.clone(),
                    Origem::OllamaReferencia { referencia: m.referencia.clone() },
                    Some(m.sha256.clone()),
                )
                .map_err(|e| MotorError::Falhou(e.to_string()))?;
                self.registry.lock().unwrap().add(im).map_err(|e| MotorError::Falhou(e.to_string()))?;
                self.aviso("info", format!("{} importado do Ollama (lido direto da pasta do Ollama).", m.referencia));
                Ok(id)
            }
            "copiar" => {
                let tid = format!("importacao:{}", m.referencia);
                let cancel = self.task_begin(&tid, "importacao", &m.referencia, Some(m.tamanho))?;
                let me = self.clone();
                let dest =
                    self.paths.models_dir().join(format!("{}.gguf", crate::util::safe_file_name(&format!("ollama-{}", m.referencia))));
                tokio::spawn(async move {
                    let me2 = me.clone();
                    let tid2 = tid.clone();
                    let src = m.caminho.clone();
                    let sha = m.sha256.clone();
                    let d2 = dest.clone();
                    let c2 = cancel.clone();
                    let total = m.tamanho;
                    let r = tokio::task::spawn_blocking(move || {
                        ollama::copy_verified(&src, &d2, &sha, &|| c2.is_cancelled(), &|n| {
                            me2.task_progress(&tid2, Progress { baixado: n, total: Some(total) })
                        })
                    })
                    .await;
                    match r {
                        Ok(Ok(true)) => {
                            match InstalledModel::from_file(
                                id.clone(),
                                Some(m.referencia.clone()),
                                dest,
                                Origem::OllamaCopia { referencia: m.referencia.clone() },
                                Some(m.sha256.clone()),
                            ) {
                                Ok(im) => {
                                    let _ = me.registry.lock().unwrap().add(im);
                                    me.task_finish(&tid, "concluida", None);
                                    me.aviso("info", format!("{} copiado do Ollama.", m.referencia));
                                }
                                Err(e) => me.task_finish(&tid, "erro", Some(e.to_string())),
                            }
                        }
                        Ok(Ok(false)) => me.task_finish(
                            &tid,
                            "erro",
                            Some("A cópia não conferiu com o original (SHA-256). Nada foi alterado no Ollama.".into()),
                        ),
                        Ok(Err(e)) if e.kind() == std::io::ErrorKind::Interrupted => me.task_finish(&tid, "pausada", None),
                        Ok(Err(e)) => me.task_finish(&tid, "erro", Some(e.to_string())),
                        Err(e) => me.task_finish(&tid, "erro", Some(e.to_string())),
                    }
                });
                Ok(ollama::model_id(referencia))
            }
            _ => Err(MotorError::Invalido("modo precisa ser \"usar\" ou \"copiar\".".into())),
        }
    }

    /// Adiciona um arquivo GGUF qualquer (lido no lugar, nunca apagado).
    pub fn import_file(&self, caminho: &Path) -> Result<String, MotorError> {
        if !crate::gguf::is_gguf(caminho) {
            return Err(MotorError::Invalido("Este arquivo não é um modelo GGUF.".into()));
        }
        // arquivo de outro programa (o Hugging Face guarda pelo hash): usa o nome de verdade
        if let Some(l) = self.modelos_no_computador().into_iter().find(|l| l.caminho == caminho) {
            if l.fonte != crate::locais::Fonte::Ollama {
                return self.registrar_local(&l).map(|im| im.id);
            }
        }
        let stem = caminho.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "modelo".into());
        let id = format!("arquivo/{}", crate::util::safe_file_name(&stem));
        let im = InstalledModel::from_file(id.clone(), Some(stem), caminho.to_path_buf(), Origem::Arquivo, None)
            .map_err(|e| MotorError::Falhou(e.to_string()))?;
        self.registry.lock().unwrap().add(im).map_err(|e| MotorError::Falhou(e.to_string()))?;
        self.bump();
        Ok(id)
    }

    // ---------- preparação e teste de velocidade ----------

    fn bench_model(&self) -> Option<InstalledModel> {
        let reg = self.registry.lock().unwrap();
        let cat = self.catalog();
        let teste = cat.test_model().map(|m| m.id.clone());
        reg.modelos
            .iter()
            .filter(|m| !m.embeddings && m.existe() && m.tamanho <= TAMANHO_MAX_TESTE)
            .min_by_key(|m| (Some(&m.id) != teste.as_ref(), m.tamanho))
            .cloned()
    }

    /// O que falta baixar para preparar esta máquina.
    pub fn preparation_plan(&self) -> Value {
        let m = self.manifest();
        let mut total = 0u64;
        let motores: Vec<Value> = self
            .candidates()
            .into_iter()
            .filter(|c| c.disponivel && c.variante != VARIANTE_PROPRIA)
            .map(|c| {
                let instalado = self.engine_exe(&c.variante).is_some();
                let falta = if instalado {
                    0
                } else {
                    c.tamanho_download.saturating_sub(m.variant(&c.variante).map(|v| self.store.partial_bytes(v)).unwrap_or(0))
                };
                total += falta;
                json!({"variante": c.variante, "nome": c.nome, "tamanho": c.tamanho_download, "instalado": instalado})
            })
            .collect();
        let modelo_teste = if self.bench_model().is_some() {
            Value::Null
        } else {
            let cat = self.catalog();
            let p = self.pastas_locais();
            match cat.test_model() {
                Some(t) => match crate::locais::achar_por_sha256(&t.sha256, &p.ollama, p.hugging_face.as_deref()) {
                    Some(l) => json!({"id": t.id, "nome": t.nome, "tamanho": t.tamanho, "ja_no_computador": l.fonte.nome()}),
                    None => match self
                        .modelos_no_computador()
                        .into_iter()
                        .filter(|m| m.tamanho >= 50_000_000 && m.tamanho <= TAMANHO_MAX_TESTE)
                        .min_by_key(|m| m.tamanho)
                    {
                        Some(l) => json!({"id": null, "nome": l.nome, "tamanho": l.tamanho, "ja_no_computador": l.fonte.nome()}),
                        None => {
                            total += t.tamanho;
                            json!({"id": t.id, "nome": t.nome, "tamanho": t.tamanho})
                        }
                    },
                },
                None => Value::Null,
            }
        };
        json!({"motores": motores, "modelo_teste": modelo_teste, "total": total})
    }

    pub fn needs_preparation(&self) -> bool {
        let Some(det) = self.detection() else { return false };
        let algum_motor = self.candidates().iter().any(|c| c.disponivel && self.engine_exe(&c.variante).is_some());
        !algum_motor || !self.bench_is_current(&det)
    }

    /// Baixa os motores da placa + modelo de teste e roda o teste de velocidade.
    pub fn prepare(self: &Arc<Self>) -> Result<(), MotorError> {
        if matches!(self.fase.read().unwrap().0, Fase::Preparando | Fase::Testando) {
            return Err(MotorError::Ocupado("A preparação já está em andamento.".into()));
        }
        self.set_fase(Fase::Preparando, "Preparando o Ignis para a sua placa…");
        let me = self.clone();
        tokio::spawn(async move {
            if me.detection().is_none() {
                me.detect_now().await;
            }
            let m = me.manifest();
            for c in me.candidates().into_iter().filter(|c| c.disponivel && c.variante != VARIANTE_PROPRIA) {
                if me.engine_exe(&c.variante).is_some() {
                    continue;
                }
                let Some(v) = m.variant(&c.variante).cloned() else { continue };
                let tid = format!("motor:{}", v.id);
                let Ok(cancel) = me.task_begin(&tid, "motor", &format!("Motor {}", c.nome), Some(v.tamanho_total())) else { continue };
                me.set_fase(Fase::Preparando, format!("Baixando o motor {}…", c.nome));
                let cb = |p: Progress| me.task_progress(&tid, p);
                match me.store.install(&me.http, &m, &v, &cancel, &cb).await {
                    Ok(_) => me.task_finish(&tid, "concluida", None),
                    Err(crate::engine::install::InstallError::Download(DownloadError::Cancelled)) => {
                        me.task_finish(&tid, "pausada", None);
                        me.set_fase(Fase::Parado, "");
                        me.aviso("info", "Preparação pausada. Clique em Preparar para continuar de onde parou.");
                        return;
                    }
                    Err(e) => {
                        me.task_finish(&tid, "erro", Some(e.to_string()));
                        me.aviso("erro", format!("Não consegui instalar o motor {}: {e}", c.nome));
                    }
                }
            }
            if me.bench_model().is_none() {
                // antes de baixar: o mesmo arquivo, ou outro modelo pequeno, já pode estar no computador
                let cat = me.catalog();
                if let Some(t) = cat.test_model().cloned() {
                    if me.reaproveitar_do_computador(&t).is_none() {
                        me.usar_modelo_pequeno_do_computador();
                    }
                }
            }
            if me.bench_model().is_none() {
                let cat = me.catalog();
                if let Some(t) = cat.test_model().cloned() {
                    me.set_fase(Fase::Preparando, format!("Baixando o modelo de teste ({})…", t.nome));
                    let tid = format!("modelo:{}", t.id);
                    if let Ok(cancel) = me.task_begin(&tid, "modelo", &t.nome, Some(t.tamanho)) {
                        match me.download_catalog_model(&t, &tid, &cancel).await {
                            Ok(()) => me.task_finish(&tid, "concluida", None),
                            Err(DownloadError::Cancelled) => {
                                me.task_finish(&tid, "pausada", None);
                                me.set_fase(Fase::Parado, "");
                                return;
                            }
                            Err(e) => {
                                me.task_finish(&tid, "erro", Some(e.to_string()));
                                me.aviso("erro", format!("Não consegui baixar o modelo de teste: {e}"));
                            }
                        }
                    }
                }
            }
            me.set_fase(Fase::Parado, "");
            me.benchmark_inner().await;
        });
        Ok(())
    }

    /// Roda o teste de velocidade em segundo plano.
    pub fn run_benchmark(self: &Arc<Self>) {
        if self.bench_running.load(Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move { me.benchmark_inner().await });
    }

    /// Mede todos os motores com um modelo instalado (do jeito que ele seria
    /// carregado: camadas na placa e contexto do orçamento). O resultado passa a
    /// decidir o motor desse modelo.
    pub fn run_benchmark_model(self: &Arc<Self>, model_id: &str) -> Result<(), MotorError> {
        let model = self.registry.lock().unwrap().get(model_id).cloned().ok_or_else(|| MotorError::ModeloNaoEncontrado(model_id.into()))?;
        if model.embeddings {
            return Err(MotorError::Invalido(
                "Modelos de embeddings não escrevem texto; o teste de velocidade é para modelos de conversa.".into(),
            ));
        }
        if !model.existe() {
            return Err(MotorError::ArquivoSumiu(model.nome));
        }
        if self.bench_running.load(Ordering::SeqCst) {
            return Err(MotorError::Ocupado("Já tem um teste de velocidade rodando.".into()));
        }
        let me = self.clone();
        tokio::spawn(async move { me.benchmark_model_inner(model).await });
        Ok(())
    }

    async fn benchmark_model_inner(self: &Arc<Self>, model: InstalledModel) {
        if self.bench_running.swap(true, Ordering::SeqCst) {
            return;
        }
        struct Release<'a>(&'a AtomicBool);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        let _release = Release(&self.bench_running);
        while self.ativos() > 0 {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        self.unload().await;
        let det = self.detect_now().await;
        let principal = det.principal().cloned();
        let ctx = self.settings().contexto_preferido.min(4096);
        let cands: Vec<(Candidate, PathBuf)> =
            self.candidates().into_iter().filter(|c| c.disponivel).filter_map(|c| self.engine_exe(&c.variante).map(|e| (c, e))).collect();
        if cands.is_empty() {
            self.aviso("erro", "Nenhum motor instalado para testar. Clique em Preparar.");
            return;
        }
        let n = cands.len();
        let mut resultados = Vec::new();
        for (i, (c, exe)) in cands.iter().enumerate() {
            self.set_fase(Fase::Testando, format!("Testando {} no motor {} ({} de {n})…", model.nome, c.nome, i + 1));
            let rec = Recursos {
                usa_placa: c.backend.usa_placa() && principal.is_some(),
                vram_livre_mb: principal.as_ref().and_then(|g| g.vram_livre_mb.or(g.vram_total_mb)),
                ram_livre_mb: det.ram_livre_mb,
            };
            let plano = budget::plan(&model.shape(), &rec, ctx);
            let r = benchmark::bench_one_com(
                exe,
                &c.variante,
                c.backend,
                &model.arquivo,
                &self.local,
                &plano.ngl_arg(),
                plano.contexto.max(1024),
                Duration::from_secs(300),
            )
            .await;
            resultados.push(r);
        }
        let fp = det.fingerprint();
        {
            let mut e = self.estado.lock().unwrap();
            let por_motor: HashMap<String, f64> =
                resultados.iter().filter(|r| r.ok).filter_map(|r| r.geracao_tps.map(|g| (r.variante.clone(), g))).collect();
            e.medidas.insert(
                model.id.clone(),
                MedidasModelo { fingerprint: fp, por_motor, quando: now_unix(), teste: Some(resultados.clone()) },
            );
        }
        self.save_estado();
        self.set_fase(Fase::Parado, "");
        let ordem = benchmark::rank(&resultados);
        match ordem.first().and_then(|v| resultados.iter().find(|r| &r.variante == v)) {
            Some(best) => self.aviso(
                "info",
                format!(
                    "Teste com {}: o motor {} foi o mais rápido ({} tokens/s). O Ignis vai usar ele para este modelo.",
                    model.nome,
                    best.nome,
                    format!("{:.1}", best.geracao_tps.unwrap_or(0.0)).replace('.', ",")
                ),
            ),
            None => self.aviso("erro", format!("Nenhum motor conseguiu rodar {} no teste.", model.nome)),
        }
    }

    async fn benchmark_inner(self: &Arc<Self>) {
        if self.bench_running.swap(true, Ordering::SeqCst) {
            return;
        }
        struct Release<'a>(&'a AtomicBool);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        let _release = Release(&self.bench_running);
        if self.ativos() > 0 {
            self.aviso("info", "O teste de velocidade espera as conversas em andamento terminarem…");
            while self.ativos() > 0 {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        self.unload().await;
        let Some(modelo) = self.bench_model() else {
            self.aviso("erro", "Não há modelo pequeno para o teste. Clique em Preparar para baixar o modelo de teste.");
            return;
        };
        let det = match self.detection() {
            Some(d) => d,
            None => self.detect_now().await,
        };
        let cands: Vec<(Candidate, PathBuf)> =
            self.candidates().into_iter().filter(|c| c.disponivel).filter_map(|c| self.engine_exe(&c.variante).map(|e| (c, e))).collect();
        if cands.is_empty() {
            self.aviso("erro", "Nenhum motor instalado para testar. Clique em Preparar.");
            return;
        }
        let n = cands.len();
        let mut resultados = Vec::new();
        for (i, (c, exe)) in cands.iter().enumerate() {
            self.set_fase(Fase::Testando, format!("Testando o motor {} ({} de {n})…", c.nome, i + 1));
            let r = benchmark::bench_one(exe, &c.variante, c.backend, &modelo.arquivo, &self.local).await;
            resultados.push(r);
        }
        let ordem = benchmark::rank(&resultados);
        {
            let mut sf = self.session_failed.lock().unwrap();
            for v in &ordem {
                sf.remove(v);
            }
        }
        let teste = TesteSalvo {
            fingerprint: det.fingerprint(),
            llama_cpp: self.manifest().llama_cpp.versao,
            quando: now_unix(),
            resultados: resultados.clone(),
            ordem: ordem.clone(),
            tamanho_modelo: Some(modelo.tamanho),
            modelo_nome: Some(modelo.nome.clone()),
        };
        self.estado.lock().unwrap().teste = Some(teste);
        self.save_estado();
        self.set_fase(Fase::Parado, "");
        match ordem.first().and_then(|v| resultados.iter().find(|r| &r.variante == v)) {
            Some(best) => self.aviso(
                "info",
                format!(
                    "Teste concluído: o motor {} foi o mais rápido ({} tokens/s escrevendo).",
                    best.nome,
                    format!("{:.1}", best.geracao_tps.unwrap_or(0.0)).replace('.', ",")
                ),
            ),
            None => self.aviso("erro", "Nenhum motor funcionou no teste. Veja os detalhes na seção Placa."),
        }
    }

    pub fn bench_running(&self) -> bool {
        self.bench_running.load(Ordering::SeqCst)
    }

    pub fn bench(&self) -> Option<TesteSalvo> {
        self.estado.lock().unwrap().teste.clone()
    }

    // ---------- configurações ----------

    pub fn update_settings(&self, patch: SettingsPatch) -> Result<Settings, MotorError> {
        let old = self.settings();
        let mut new = old.clone();
        patch.apply(&mut new);
        if let Some(p) = &new.llama_server_proprio {
            if !p.is_file() {
                return Err(MotorError::Invalido(format!("Não achei o arquivo {}.", p.display())));
            }
        }
        crate::paths::write_json(&self.paths.config_file(), &new).map_err(|e| MotorError::Falhou(e.to_string()))?;
        *self.settings.write().unwrap() = new.clone();
        if new.abrir_com_computador != old.abrir_com_computador {
            if let Err(e) = autostart::set(new.abrir_com_computador, self.launcher.as_ref()) {
                self.aviso("erro", format!("Não consegui mudar o \"abrir com o computador\": {e}"));
            }
        }
        if new.porta != old.porta {
            let _ = self.porta_tx.send(new.porta);
        }
        self.bump();
        Ok(new)
    }

    // ---------- atualizações ----------

    pub async fn check_updates(&self) -> Result<UpdateCheck, MotorError> {
        let url = std::env::var("IGNIS_URL_ATUALIZACOES").unwrap_or_else(|_| URL_ATUALIZACOES.to_string());
        let remoto = updates::fetch(&self.http, &url).await.map_err(MotorError::Falhou)?;
        let r = updates::compare(remoto, APP_VERSION, &self.manifest(), &self.catalog()).map_err(MotorError::Falhou)?;
        self.estado.lock().unwrap().ultima_verificacao = now_unix();
        self.save_estado();
        if r.tem_algo() {
            let mut partes = Vec::new();
            if let Some(a) = &r.app {
                partes.push(format!("aplicativo {}", a.versao));
            }
            if let Some(m) = &r.motor {
                partes.push(format!("motor llama.cpp {m}"));
            }
            if r.catalogo.is_some() {
                partes.push("catálogo de modelos".into());
            }
            self.aviso("info", format!("Atualização disponível: {}. Veja em Motor → Atualizações.", partes.join(", ")));
        }
        *self.update_info.write().unwrap() = Some(r.clone());
        self.bump();
        Ok(r)
    }

    pub fn update_info(&self) -> Option<UpdateCheck> {
        self.update_info.read().unwrap().clone()
    }

    /// Aplica a atualização aceita: "motor" ou "catalogo".
    pub fn apply_update(self: &Arc<Self>, item: &str) -> Result<(), MotorError> {
        let info = self.update_info().ok_or_else(|| MotorError::Invalido("Procure atualizações primeiro.".into()))?;
        let remoto = info.remoto.clone().ok_or_else(|| MotorError::Invalido("Procure atualizações primeiro.".into()))?;
        match item {
            "catalogo" => {
                crate::paths::write_json(&self.paths.catalog_override(), &remoto.catalogo)
                    .map_err(|e| MotorError::Falhou(e.to_string()))?;
                *self.catalog.write().unwrap() = remoto.catalogo.clone();
                self.aviso("info", format!("Catálogo de modelos atualizado (versão {}).", remoto.catalogo.versao));
                Ok(())
            }
            "motor" => {
                let novo = remoto.motores.clone();
                let me = self.clone();
                tokio::spawn(async move {
                    let atual = me.manifest();
                    let instaladas: Vec<String> =
                        atual.variantes.iter().filter(|v| me.store.installed(&atual, v).is_some()).map(|v| v.id.clone()).collect();
                    for id in &instaladas {
                        let Some(v) = novo.variant(id).cloned() else { continue };
                        let tid = format!("atualizacao:{id}");
                        let Ok(cancel) = me.task_begin(
                            &tid,
                            "atualizacao",
                            &format!("Motor {} ({})", v.backend.nome(), novo.llama_cpp.versao),
                            Some(v.tamanho_total()),
                        ) else {
                            continue;
                        };
                        let cb = |p: Progress| me.task_progress(&tid, p);
                        match me.store.install(&me.http, &novo, &v, &cancel, &cb).await {
                            Ok(_) => me.task_finish(&tid, "concluida", None),
                            Err(e) => {
                                me.task_finish(&tid, "erro", Some(e.to_string()));
                                me.aviso("erro", format!("A atualização do motor falhou: {e}. O motor atual continua funcionando."));
                                return;
                            }
                        }
                    }
                    // nunca no meio de uma conversa
                    while me.ativos() > 0 {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    if let Err(e) = crate::paths::write_json(&me.paths.engines_manifest_override(), &novo) {
                        me.aviso("erro", format!("Não consegui gravar o manifesto novo: {e}"));
                        return;
                    }
                    *me.manifest.write().unwrap() = novo.clone();
                    me.unload().await;
                    me.store.remove_other_versions(&novo.llama_cpp.versao);
                    me.aviso(
                        "info",
                        format!("Motor atualizado para o llama.cpp {}. Testando a velocidade de novo…", novo.llama_cpp.versao),
                    );
                    me.benchmark_inner().await;
                });
                Ok(())
            }
            _ => Err(MotorError::Invalido("item precisa ser \"motor\" ou \"catalogo\".".into())),
        }
    }

    // ---------- estado para a janela e a API ----------

    pub fn version_json(&self) -> Value {
        let m = self.manifest();
        let em_uso = self.loaded_info().map(|i| json!({"variante": i.variante, "backend": i.backend}));
        json!({
            "app": APP_VERSION,
            "nome": APP_NAME,
            "api": API_VERSION,
            "llama_cpp": {"versao": m.llama_cpp.versao, "numero": m.llama_cpp.numero, "commit": m.llama_cpp.commit},
            "motor_em_uso": em_uso,
        })
    }

    pub fn status_json(&self) -> Value {
        let (fase, fase_texto) = self.fase();
        let det = self.detection();
        let s = self.settings();
        let api = self.api_status();
        let porta = api.clone().unwrap_or(s.porta);
        let bench = self.bench();
        let teste_atual = det.as_ref().map(|d| self.bench_is_current(d)).unwrap_or(false);
        let carregado = self.loaded_info();
        let motor_info = carregado.as_ref().map(|i| json!({"variante": i.variante, "backend": i.backend, "nome": i.motor_nome}));
        let modelo_info = carregado.as_ref().map(|i| {
            json!({
                "id": i.modelo_id, "nome": i.modelo_nome, "contexto": i.plano.contexto,
                "camadas_na_placa": i.plano.camadas_na_placa, "todas_na_placa": i.plano.todas_na_placa,
                "encaixe": i.plano.encaixe, "embeddings": i.embeddings, "desde": i.desde,
                "memoria_placa_mb": i.plano.memoria_placa_mb, "memoria_ram_mb": i.plano.memoria_ram_mb,
                "motivo_motor": i.motivo,
            })
        });
        let proximo_motor = self.engine_order().first().map(|(c, _)| json!({"variante": c.variante, "nome": c.nome, "backend": c.backend}));
        json!({
            "app": {"nome": APP_NAME, "versao": APP_VERSION, "api": API_VERSION},
            "fase": fase,
            "fase_texto": fase_texto,
            "pausado": self.is_paused(),
            "api": {"rodando": api.is_ok(), "porta": porta, "erro": api.err(), "endereco": format!("http://127.0.0.1:{porta}/v1")},
            "porta": porta,
            "sistema": det.as_ref().map(|d| &d.sistema),
            "placa": det.as_ref().and_then(|d| d.principal()),
            "placas": det.as_ref().map(|d| &d.placas),
            "ram": det.as_ref().map(|d| json!({"total_mb": d.ram_total_mb, "livre_mb": d.ram_livre_mb})),
            "motor": motor_info,
            "proximo_motor": proximo_motor,
            "motor_forcado": s.motor_forcado,
            "llama_cpp": self.manifest().llama_cpp.versao,
            "modelo": modelo_info,
            "ultimo_carregamento": self.ultimo_carregamento.read().unwrap().clone(),
            "velocidade": self.velocidade.read().unwrap().clone(),
            "teste": {
                "feito": bench.is_some(),
                "atual": teste_atual,
                "quando": bench.as_ref().map(|b| b.quando),
                "resultados": bench.as_ref().map(|b| b.resultados.clone()).unwrap_or_default(),
                "ordem": bench.as_ref().map(|b| b.ordem.clone()).unwrap_or_default(),
                "modelo_nome": bench.as_ref().and_then(|b| b.modelo_nome.clone()),
            },
            "precisa_preparar": self.needs_preparation(),
            "audio": self.audio_status_json(),
            "tarefas": self.tarefas(),
            "avisos": self.avisos(),
            "atualizacao": self.update_info(),
            "requisicoes_ativas": self.ativos(),
            "detectado": det.is_some(),
        })
    }

    /// Encerra tudo (ao sair do aplicativo).
    pub async fn shutdown_all(&self) {
        self.shutdown.cancel();
        for t in self.tarefas.lock().unwrap().values() {
            t.cancel.cancel();
        }
        self.stop_requests(None);
        self.unload().await;
        self.descarregar_audio(None).await;
    }
}
