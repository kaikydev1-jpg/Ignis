//! Ajudantes para os testes com a placa simulada.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ignis_core::catalog::Catalog;
use ignis_core::distro::{Familia, Sistema};
use ignis_core::engine::manifest::{current_os, AssetFile, LlamaCppInfo};
use ignis_core::engine::{Backend, EngineManifest, EngineStore, Variant};
use ignis_core::gguf::{write_test_gguf, Value as GValue};
use ignis_core::gpu::{Detection, Gpu, RawSources, Vendor};
use ignis_core::paths::Paths;
use ignis_core::service::{Motor, MotorOptions};
use serde_json::{json, Value};

pub const FAKE: &str = env!("CARGO_BIN_EXE_fake-llama-server");

/// Placa simulada: AMD RX 6600 (gfx1032), 8 GB, com ROCm e Vulkan disponíveis.
pub fn deteccao_simulada() -> Detection {
    let familia = if cfg!(windows) { Familia::Windows } else { Familia::Fedora };
    Detection {
        sistema: Sistema { familia, nome: "Teste".into(), versao: None, flatpak: false, appimage: false },
        arq: std::env::consts::ARCH.to_string(),
        placas: vec![Gpu {
            fabricante: Vendor::Amd,
            nome: "AMD Radeon RX 6600 (simulada)".into(),
            pci_dispositivo: Some(0x73ff),
            vram_total_mb: Some(8176),
            vram_livre_mb: Some(7800),
            driver: Some("amdgpu".into()),
            versao_driver: Some("Mesa 25.2.4 (radv)".into()),
            alvo_gfx: Some("gfx1032".into()),
            integrada: false,
            fontes: vec!["simulada".into()],
        }],
        ram_total_mb: 32000,
        ram_livre_mb: 24000,
        libs: BTreeMap::new(),
        kfd_acessivel: true,
        ferramentas: vec![],
        fontes_brutas: RawSources::default(),
    }
}

fn variante(id: &str, backend: Backend) -> Variant {
    Variant {
        id: id.into(),
        so: current_os().into(),
        arq: std::env::consts::ARCH.into(),
        backend,
        arquivos: vec![AssetFile { nome: format!("{id}.zip"), sha256: "a".repeat(64), tamanho: 1000 }],
        libs_do_sistema: vec![],
        alvos_gfx: vec!["gfx1032".into()],
        alvos_gfx_oficiais: None,
        driver_minimo: None,
        dica_falta: None,
        pacotes: None,
        nota: None,
    }
}

/// Manifesto de teste com três motores: ROCm, Vulkan e Processador.
pub fn manifesto_simulado() -> EngineManifest {
    EngineManifest {
        versao_manifesto: 1,
        llama_cpp: LlamaCppInfo { versao: "b1".into(), numero: 1, commit: "teste".into(), licenca: "MIT".into() },
        url_base: "https://exemplo.invalid/".into(),
        variantes: vec![variante("sim-rocm", Backend::Rocm), variante("sim-vulkan", Backend::Vulkan), variante("sim-cpu", Backend::Cpu)],
    }
}

pub struct Ambiente {
    pub dir: tempfile::TempDir,
    pub motor: Arc<Motor>,
    pub paths: Paths,
}

/// Comportamento de um motor simulado (vira o fake.json).
pub fn cfg(tps_geracao: f64) -> Value {
    json!({"modo": "ok", "tps_leitura": tps_geracao * 10.0, "tps_geracao": tps_geracao, "demora_subir_ms": 150})
}

pub struct Opcoes {
    pub motores: Vec<(&'static str, Value)>,
    pub config: Value,
    pub tick_ms: u64,
    pub catalog: Option<Catalog>,
}

impl Default for Opcoes {
    fn default() -> Self {
        Self {
            motores: vec![("sim-rocm", cfg(80.0)), ("sim-vulkan", cfg(60.0)), ("sim-cpu", cfg(10.0))],
            config: json!({}),
            tick_ms: 200,
            catalog: None,
        }
    }
}

pub fn ambiente(op: Opcoes) -> Ambiente {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    paths.ensure().unwrap();
    let mut config = json!({"primeira_execucao_feita": true, "porta": free_port()});
    if let (Some(base), Some(extra)) = (config.as_object_mut(), op.config.as_object()) {
        for (k, v) in extra {
            base.insert(k.clone(), v.clone());
        }
    }
    std::fs::write(paths.config_file(), serde_json::to_vec(&config).unwrap()).unwrap();
    let manifest = manifesto_simulado();
    let store = EngineStore::new(paths.engines_dir(), paths.downloads_dir());
    for (id, c) in &op.motores {
        let mut c = c.clone();
        // contador de tokens por motor (teste do Parar)
        c["arquivo_contador"] = json!(dir.path().join(format!("{id}.tokens")));
        let v = manifest.variant(id).unwrap();
        store.install_from_files(&manifest, v, Path::new(FAKE), &[("fake.json", serde_json::to_vec(&c).unwrap().as_slice())]).unwrap();
    }
    let motor = Motor::new(
        paths.clone(),
        MotorOptions {
            detection_override: Some(deteccao_simulada()),
            tick: Duration::from_millis(op.tick_ms),
            launcher: None,
            load_timeout: Duration::from_secs(20),
            catalog_override: op.catalog,
            manifest_override: Some(manifest),
            auto_teste: false,
        },
    );
    Ambiente { dir, motor, paths }
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port()
}

/// Cria um GGUF pequeno (de conversa ou de embeddings).
pub fn modelo_gguf(dir: &Path, nome: &str, embeddings: bool) -> PathBuf {
    let p = dir.join(format!("{nome}.gguf"));
    let mut kv = vec![
        ("general.architecture", GValue::Str("qwen3".into())),
        ("general.name", GValue::Str(nome.into())),
        ("qwen3.block_count", GValue::Int(28)),
        ("qwen3.context_length", GValue::Int(40960)),
        ("qwen3.embedding_length", GValue::Int(1024)),
        ("qwen3.attention.head_count", GValue::Int(16)),
        ("qwen3.attention.head_count_kv", GValue::Int(8)),
    ];
    if embeddings {
        kv.push(("qwen3.pooling_type", GValue::Int(3)));
    }
    write_test_gguf(&p, &kv).unwrap();
    p
}

/// Espera uma condição (com tempo-limite).
pub async fn esperar<F: Fn() -> bool>(o_que: &str, limite: Duration, f: F) {
    let t0 = Instant::now();
    while !f() {
        if t0.elapsed() > limite {
            panic!("tempo esgotado esperando: {o_que}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub fn avisos_texto(m: &Motor) -> String {
    m.avisos().iter().map(|a| a.texto.clone()).collect::<Vec<_>>().join("\n")
}

/// Sobe a API numa porta livre e devolve o endereço base.
pub async fn subir_api(m: &Arc<Motor>) -> String {
    let mm = m.clone();
    tokio::spawn(async move { ignis_core::api::serve(mm).await });
    let porta = m.settings().porta;
    let base = format!("http://127.0.0.1:{porta}");
    let c = cliente();
    let t0 = Instant::now();
    loop {
        if c.get(format!("{base}/motor/versao")).send().await.map(|r| r.status().is_success()).unwrap_or(false) {
            return base;
        }
        if t0.elapsed() > Duration::from_secs(10) {
            panic!("API não subiu");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub fn cliente() -> reqwest::Client {
    ignis_core::install_crypto_provider();
    reqwest::Client::builder().no_proxy().build().unwrap()
}

pub fn contador(amb: &Ambiente, variante: &str) -> u64 {
    std::fs::read_to_string(amb.dir.path().join(format!("{variante}.tokens"))).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}
