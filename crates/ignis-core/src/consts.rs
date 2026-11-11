//! Nome e constantes do aplicativo. Para renomear o projeto, troque aqui,
//! em `app/src-tauri/tauri.conf.json` e nos nomes de pacote.

/// Nome mostrado para a pessoa.
pub const APP_NAME: &str = "Ignis";
/// Nome usado em pastas, chave Run do Windows e arquivo .desktop.
pub const APP_ID: &str = "ignis";
/// Versão do aplicativo (vem do Cargo).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Versão do contrato da API (/motor/*). Sobe só quando algo quebra a compatibilidade.
pub const API_VERSION: u32 = 1;
/// Porta padrão da API local (configurável).
pub const PORTA_PADRAO: u16 = 11470;
/// Endereço do manifesto de atualizações (só é consultado por ação da pessoa
/// ou se ela ligar a verificação automática).
pub const URL_ATUALIZACOES: &str =
    "https://github.com/kaikydev1-jpg/Ignis/releases/latest/download/atualizacoes.json";
/// Página de downloads do aplicativo.
pub const URL_PAGINA_RELEASES: &str = "https://github.com/kaikydev1-jpg/Ignis/releases";
/// User-Agent genérico (sem dados da pessoa).
pub const USER_AGENT: &str = concat!("Ignis/", env!("CARGO_PKG_VERSION"));
