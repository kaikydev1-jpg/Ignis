//! Motores (llama-server oficial do llama.cpp): manifesto, candidatos e instalação.

pub mod candidates;
pub mod install;
pub mod manifest;

pub use candidates::{build as build_candidates, Candidate, VARIANTE_PROPRIA};
pub use install::{EngineStore, InstalledEngine};
pub use manifest::{Backend, EngineManifest, Variant};
