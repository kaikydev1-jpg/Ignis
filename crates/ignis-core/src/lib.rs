//! Núcleo do Ignis.
//!
//! Tudo o que não é janela nem bandeja mora aqui, para poder ser testado sem
//! interface gráfica e sem placa de vídeo.

pub mod api;
pub mod autostart;
pub mod benchmark;
pub mod budget;
pub mod catalog;
pub mod config;
pub mod consts;
pub mod distro;
pub mod download;
pub mod engine;
pub mod gguf;
pub mod gpu;
pub mod models;
pub mod ollama;
pub mod paths;
pub mod service;
pub mod supervisor;
pub mod updates;
pub mod util;

/// Instala o provedor de criptografia do rustls (ring) uma vez.
pub fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
