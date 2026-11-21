//! Supervisor do `llama-server`: sobe o processo numa porta interna,
//! espera ficar pronto, guarda o registro (log) e encerra.
//!
//! O processo filho morre junto com o Ignis mesmo se o Ignis for fechado à
//! força (Linux: PR_SET_PDEATHSIG; Windows: Job Object).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::engine::Backend;

const LOG_LINES: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSpec {
    pub exe: PathBuf,
    pub modelo: PathBuf,
    pub alias: String,
    pub contexto: u32,
    pub ngl: String,
    pub embeddings: bool,
    pub backend: Backend,
    pub variante: String,
    #[serde(default)]
    pub args_extras: Vec<String>,
    /// Arquivo de visão/áudio (mmproj).
    #[serde(default)]
    pub mmproj: Option<PathBuf>,
    /// Argumentos de outro servidor (voz: tts-server; transcrição: whisper-server).
    /// `{porta}` vira a porta escolhida. Sem isto, os argumentos são os do llama-server.
    #[serde(default)]
    pub args_proprios: Option<Vec<String>>,
    /// Tipo do cache de contexto (f16 é o padrão do llama.cpp).
    #[serde(default)]
    pub cache_kv: crate::budget::CacheKv,
    /// Atenção rápida (flash attention): None = automático (o llama.cpp liga onde funciona).
    #[serde(default)]
    pub atencao_rapida: Option<bool>,
    /// Rota que responde 200 quando o servidor está pronto (padrão: `/health`).
    #[serde(default)]
    pub rota_saude: Option<String>,
}

impl LaunchSpec {
    pub fn args(&self, porta: u16) -> Vec<String> {
        if let Some(a) = &self.args_proprios {
            return a.iter().map(|x| x.replace("{porta}", &porta.to_string())).collect();
        }
        let mut a: Vec<String> = vec![
            "-m".into(),
            self.modelo.to_string_lossy().into_owned(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            porta.to_string(),
            "-c".into(),
            self.contexto.to_string(),
            "-ngl".into(),
            if self.backend.usa_placa() { self.ngl.clone() } else { "0".into() },
            "--alias".into(),
            self.alias.clone(),
            "--no-webui".into(),
            "-np".into(),
            "1".into(),
            // nível 4: o registro mostra a atenção rápida e o tamanho do cache (umas 20 linhas por pedido)
            "-lv".into(),
            "4".into(),
        ];
        if self.embeddings {
            a.push("--embeddings".into());
        }
        if let Some(mm) = &self.mmproj {
            a.push("--mmproj".into());
            a.push(mm.to_string_lossy().into_owned());
        }
        if self.cache_kv != crate::budget::CacheKv::F16 {
            // o cache V em 8 bits exige atenção rápida; no automático o llama.cpp liga sozinho
            for flag in ["-ctk", "-ctv"] {
                a.push(flag.into());
                a.push(self.cache_kv.nome().into());
            }
        }
        if let Some(fa) = self.atencao_rapida {
            a.push("-fa".into());
            a.push(if fa { "on" } else { "off" }.into());
        }
        a.extend(self.args_extras.iter().cloned());
        a
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("não consegui iniciar o motor: {0}")]
    Spawn(String),
    #[error("{motivo}")]
    Exited { motivo: String, log: Vec<String> },
    #[error("o motor demorou demais para ficar pronto")]
    Timeout { log: Vec<String> },
}

impl StartError {
    pub fn log(&self) -> &[String] {
        match self {
            StartError::Exited { log, .. } | StartError::Timeout { log } => log,
            StartError::Spawn(_) => &[],
        }
    }
}

pub type LogBuf = Arc<Mutex<VecDeque<String>>>;

pub struct Server {
    child: Child,
    pub porta: u16,
    pub base_url: String,
    pub spec: LaunchSpec,
    pub iniciado: Instant,
    log: LogBuf,
    /// Linhas do registro sobre atenção rápida e cache (guardadas mesmo que o registro gire).
    marcas: LogBuf,
}

impl Server {
    /// Atenção rápida ligada? Lido do registro do llama-server (None = não disse).
    pub fn atencao_rapida(&self) -> Option<bool> {
        atencao_rapida_do_registro(&self.marcas.lock().unwrap().iter().cloned().collect::<Vec<_>>())
    }

    /// Memória do cache de contexto (MiB), lida do registro do llama-server.
    pub fn memoria_cache_mb(&self) -> Option<f64> {
        memoria_cache_do_registro(&self.marcas.lock().unwrap().iter().cloned().collect::<Vec<_>>())
    }

    pub fn log_tail(&self, n: usize) -> Vec<String> {
        tail(&self.log, n)
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }

    /// Já terminou? (sem bloquear)
    pub fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// Espera o processo terminar (para vigiar quedas).
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    pub async fn stop(&mut self) {
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(10), self.child.wait()).await;
    }
}

fn tail(log: &LogBuf, n: usize) -> Vec<String> {
    let l = log.lock().unwrap();
    l.iter().rev().take(n).rev().cloned().collect()
}

/// O que o llama-server diz sobre a atenção rápida, na ordem em que diz.
pub fn atencao_rapida_do_registro(linhas: &[String]) -> Option<bool> {
    let mut r = None;
    for l in linhas {
        let l = l.to_lowercase();
        if !l.contains("flash") {
            continue;
        }
        if l.contains("not supported") || l.contains("set to disabled") || l.contains("disabling flash") || l.contains("forcing off") {
            r = Some(false);
        } else if l.contains("flash attention enabled")
            || l.contains("enabling flash_attn")
            || (l.contains("flash_attn") && l.contains("= enabled"))
        {
            r = Some(true);
        } else if l.contains("flash_attn") && l.contains("= disabled") {
            r = Some(false);
        }
    }
    r
}

/// "llama_kv_cache: size =  448.00 MiB (  4096 cells, …)" → 448.0 (soma se houver mais de uma).
pub fn memoria_cache_do_registro(linhas: &[String]) -> Option<f64> {
    let mut total = None;
    for l in linhas {
        let Some(i) = l.find("kv_cache: size =") else { continue };
        let resto = l[i + "kv_cache: size =".len()..].trim_start();
        if let Some(mib) = resto.split_whitespace().next().and_then(|n| n.parse::<f64>().ok()) {
            *total.get_or_insert(0.0) += mib;
        }
    }
    total
}

fn eh_marca(line: &str) -> bool {
    let l = line.to_lowercase();
    l.contains("flash") || l.contains("type_k") || l.contains("n_ctx") || l.contains("kv_cache: size")
}

fn push_log(log: &LogBuf, line: String) {
    let mut l = log.lock().unwrap();
    if l.len() >= LOG_LINES {
        l.pop_front();
    }
    l.push_back(line);
}

/// Explica em português, a partir do registro, por que o motor falhou.
pub fn classify_failure(log: &[String], exit_code: Option<i32>) -> String {
    let all = log.join("\n").to_lowercase();
    let has = |pats: &[&str]| pats.iter().any(|p| all.contains(p));
    if has(&[
        "out of memory",
        "failed to allocate",
        "erroroutofdevicememory",
        "cudamalloc failed",
        "hiperroroutofmemory",
        "unable to allocate",
        "outofdevicememory",
    ]) {
        return "A placa ficou sem memória para este modelo. Tente um modelo menor ou feche jogos e outros programas.".into();
    }
    if has(&["hiperrornobinaryforgpu", "no kernel image is available", "invalid device function", "unsupported gpu", "no compatible gpu"]) {
        return "Este motor não tem suporte para a sua placa.".into();
    }
    if exit_code == Some(-1073741515) {
        // 0xC0000135: DLL não encontrada (Windows)
        return "Faltam bibliotecas do Windows (Visual C++) para o motor. Reinstale o Ignis ou instale o \"Microsoft Visual C++ Redistributable 2015-2022 (x64)\".".into();
    }
    if has(&["error while loading shared libraries", "cannot open shared object", "could not load library"]) {
        return "Faltam bibliotecas do sistema para este motor (driver desatualizado ou incompleto).".into();
    }
    if has(&["unknown model architecture", "unknown architecture", "unsupported model architecture"]) {
        return "Este motor não reconhece este modelo (arquitetura nova demais para esta versão do motor).".into();
    }
    if has(&["failed to load model", "error loading model", "failed to open", "gguf_init_from_file", "invalid magic"]) {
        return "O motor não conseguiu abrir o arquivo do modelo. Ele pode estar incompleto ou corrompido; tente baixar de novo.".into();
    }
    if has(&["address already in use", "couldn't bind", "failed to bind"]) {
        return "A porta interna do motor estava ocupada.".into();
    }
    if has(&["vk::", "vulkan error", "vk_error", "ggml_vulkan: error"]) {
        return "O driver Vulkan deu erro. Atualizar o driver da placa costuma resolver.".into();
    }
    if has(&["cuda error", "rocm error", "hip error"]) {
        return "O driver da placa deu erro ao rodar o modelo.".into();
    }
    match exit_code {
        Some(c) => format!("O motor parou sem explicar (código {c}). Veja o registro em Status."),
        None => "O motor parou sem explicar. Veja o registro em Status.".into(),
    }
}

fn prepare_command(spec: &LaunchSpec, porta: u16) -> Command {
    let mut cmd = Command::new(&spec.exe);
    cmd.args(spec.args(porta)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    if let Some(dir) = spec.exe.parent() {
        cmd.current_dir(dir);
        if cfg!(target_os = "linux") {
            let mut paths = vec![dir.to_path_buf()];
            paths.extend(crate::gpu::extra_lib_dirs());
            if let Some(old) = std::env::var_os("LD_LIBRARY_PATH") {
                paths.extend(std::env::split_paths(&old));
            }
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("LD_LIBRARY_PATH", joined);
            }
        }
    }
    crate::util::hide_console(&mut cmd);
    #[cfg(target_os = "linux")]
    unsafe {
        cmd.pre_exec(|| {
            // morre junto com o Ignis
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    cmd
}

#[cfg(windows)]
fn attach_to_job(child: &Child) {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    struct Job(HANDLE);
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}
    static JOB: OnceLock<Option<Job>> = OnceLock::new();
    let job = JOB.get_or_init(|| unsafe {
        let h = CreateJobObjectW(None, None).ok()?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            h,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .ok()?;
        Some(Job(h))
    });
    if let (Some(job), Some(raw)) = (job, child.raw_handle()) {
        unsafe {
            let _ = AssignProcessToJobObject(job.0, HANDLE(raw));
        }
    }
}

#[cfg(not(windows))]
fn attach_to_job(_child: &Child) {}

/// Sobe o llama-server e espera `/health` responder 200.
pub async fn start(spec: &LaunchSpec, client: &reqwest::Client, timeout: Duration) -> Result<Server, StartError> {
    let mut last_err = None;
    // Tenta de novo se a porta escolhida for tomada no meio do caminho.
    for _ in 0..3 {
        match start_once(spec, client, timeout).await {
            Err(StartError::Exited { motivo, log }) if motivo.contains("porta interna") => {
                last_err = Some(StartError::Exited { motivo, log });
            }
            other => return other,
        }
    }
    Err(last_err.unwrap())
}

async fn start_once(spec: &LaunchSpec, client: &reqwest::Client, timeout: Duration) -> Result<Server, StartError> {
    let porta = crate::util::free_local_port().map_err(|e| StartError::Spawn(e.to_string()))?;
    let mut cmd = prepare_command(spec, porta);
    tracing::info!("iniciando motor {} ({}) porta {porta}: {:?}", spec.variante, spec.backend.nome(), spec.args(porta));
    let mut child = cmd.spawn().map_err(|e| StartError::Spawn(format!("{} ({e})", spec.exe.display())))?;
    attach_to_job(&child);
    let log: LogBuf = Arc::new(Mutex::new(VecDeque::new()));
    let marcas: LogBuf = Arc::new(Mutex::new(VecDeque::new()));
    for stream in [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let log = log.clone();
        let marcas = marcas.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "llama", "{line}");
                if eh_marca(&line) {
                    push_log(&marcas, line.clone());
                }
                push_log(&log, line);
            }
        });
    }
    let base_url = format!("http://127.0.0.1:{porta}");
    let started = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            // dá um instante para o log terminar de chegar
            tokio::time::sleep(Duration::from_millis(150)).await;
            let l = tail(&log, 80);
            return Err(StartError::Exited { motivo: classify_failure(&l, status.code()), log: l });
        }
        if started.elapsed() > timeout {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(StartError::Timeout { log: tail(&log, 80) });
        }
        let saude = spec.rota_saude.as_deref().unwrap_or("/health");
        if let Ok(r) = client.get(format!("{base_url}{saude}")).timeout(Duration::from_secs(2)).send().await {
            if r.status().is_success() {
                tracing::info!("motor pronto em {:.1}s", started.elapsed().as_secs_f32());
                // as últimas linhas do início podem chegar logo depois do /health
                tokio::time::sleep(Duration::from_millis(50)).await;
                return Ok(Server { child, porta, base_url, spec: spec.clone(), iniciado: Instant::now(), log, marcas });
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Cliente HTTP local (sem proxy) para falar com o llama-server.
pub fn local_client() -> reqwest::Client {
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .tls_certs_only(Vec::new())
        .connect_timeout(Duration::from_secs(5))
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explica_falhas() {
        let l = |s: &str| vec![s.to_string()];
        assert!(classify_failure(&l("ggml_vulkan: Device memory allocation of size 123 failed. ErrorOutOfDeviceMemory"), Some(1))
            .contains("sem memória"));
        assert!(classify_failure(&l("rocBLAS error: hipErrorNoBinaryForGpu"), Some(1)).contains("suporte para a sua placa"));
        assert!(
            classify_failure(&l("./llama-server: error while loading shared libraries: libvulkan.so.1"), Some(127)).contains("bibliotecas")
        );
        assert!(classify_failure(&l("llama_model_load: error loading model architecture: unknown model architecture: 'xyz'"), Some(1))
            .contains("não reconhece"));
        assert!(classify_failure(&l("gguf_init_from_file: invalid magic characters"), Some(1)).contains("corrompido"));
        assert!(classify_failure(&l("nada útil"), Some(3)).contains("código 3"));
        assert!(classify_failure(&[], Some(-1073741515)).contains("Visual C++"));
    }

    #[test]
    fn argumentos() {
        let s = LaunchSpec {
            exe: "llama-server".into(),
            modelo: "/m/q.gguf".into(),
            alias: "qwen3-4b".into(),
            contexto: 8192,
            ngl: "999".into(),
            embeddings: false,
            backend: Backend::Cpu,
            variante: "linux-cpu".into(),
            args_extras: vec![],
            mmproj: None,
            args_proprios: None,
            cache_kv: crate::budget::CacheKv::F16,
            atencao_rapida: None,
            rota_saude: None,
        };
        let a = s.args(5555);
        assert!(a.windows(2).any(|w| w == ["--host", "127.0.0.1"]));
        assert!(a.windows(2).any(|w| w == ["-ngl", "0"]), "processador nunca manda camadas para a placa");
        assert!(!a.contains(&"--embeddings".to_string()));
        assert!(!a.iter().any(|x| x == "-ctk" || x == "-fa"), "sem pedir, fica o padrão do llama.cpp: {a:?}");
        let q8 = LaunchSpec { cache_kv: crate::budget::CacheKv::Q8_0, atencao_rapida: Some(false), ..s };
        let a = q8.args(5555);
        assert!(a.windows(2).any(|w| w == ["-ctk", "q8_0"]) && a.windows(2).any(|w| w == ["-ctv", "q8_0"]), "{a:?}");
        assert!(a.windows(2).any(|w| w == ["-fa", "off"]));
    }

    #[test]
    fn atencao_rapida_pelo_registro() {
        let l = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            atencao_rapida_do_registro(&l(&["llama_context: flash_attn            = auto", "sched_reserve: Flash Attention enabled"])),
            Some(true)
        );
        assert_eq!(
            atencao_rapida_do_registro(&l(&["flash_attn = auto", "sched_reserve: Flash Attention not supported, set to disabled"])),
            Some(false)
        );
        assert_eq!(
            atencao_rapida_do_registro(&l(&["llama_init_from_model: enabling flash_attn since it is required for quantized V cache"])),
            Some(true)
        );
        assert_eq!(atencao_rapida_do_registro(&l(&["llama_context: flash_attn            = disabled"])), Some(false));
        assert_eq!(atencao_rapida_do_registro(&l(&["nada sobre isso"])), None);
        // linhas reais do llama-server b12271 com -lv 4
        let real = l(&[
            "0.00.583.108 I llama_context: flash_attn            = auto",
            "0.00.806.441 I llama_kv_cache: size =  448.00 MiB (  4096 cells,  28 layers,  1/1 seqs), K (f16):  224.00 MiB, V (f16):  224.00 MiB",
            "0.00.807.409 I resolve_fused_ops: Flash Attention enabled",
        ]);
        assert_eq!(atencao_rapida_do_registro(&real), Some(true));
        assert_eq!(memoria_cache_do_registro(&real), Some(448.0));
        assert_eq!(memoria_cache_do_registro(&l(&["sem cache"])), None);
    }
}
