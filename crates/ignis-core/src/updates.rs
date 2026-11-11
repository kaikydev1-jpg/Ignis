//! Atualizações separadas: aplicativo, motores (llama.cpp) e catálogo de modelos.
//!
//! O manifesto remoto (`atualizacoes.json`, publicado junto com cada versão no
//! GitHub Releases) traz as três coisas. Só é consultado por ação da pessoa
//! (ou se ela ligar a verificação semanal). Nada é aplicado sem ela aceitar,
//! e nunca no meio de uma conversa.

use serde::{Deserialize, Serialize};

use crate::catalog::Catalog;
use crate::engine::EngineManifest;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    pub versao: String,
    pub pagina: String,
    #[serde(default)]
    pub notas: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteManifest {
    pub versao_manifesto: u32,
    pub app: AppInfo,
    pub motores: EngineManifest,
    pub catalogo: Catalog,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateCheck {
    pub verificado_em: u64,
    /// Nova versão do aplicativo (só avisamos e mostramos a página).
    pub app: Option<AppInfo>,
    /// Nova versão do llama.cpp disponível.
    pub motor: Option<String>,
    /// Novo catálogo (número da versão).
    pub catalogo: Option<u32>,
    #[serde(skip)]
    pub remoto: Option<RemoteManifest>,
}

impl UpdateCheck {
    pub fn tem_algo(&self) -> bool {
        self.app.is_some() || self.motor.is_some() || self.catalogo.is_some()
    }
}

/// "0.10.2" > "0.9.9"
pub fn semver_gt(a: &str, b: &str) -> bool {
    let p = |s: &str| -> Vec<u64> { s.trim_start_matches('v').split(['.', '-']).map(|x| x.parse().unwrap_or(0)).collect() };
    let (x, y) = (p(a), p(b));
    for i in 0..x.len().max(y.len()) {
        let (m, n) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if m != n {
            return m > n;
        }
    }
    false
}

/// Compara o remoto com o que está em uso.
pub fn compare(remoto: RemoteManifest, app_atual: &str, motor_atual: &EngineManifest, catalogo_atual: &Catalog) -> Result<UpdateCheck, String> {
    remoto.motores.validate()?;
    remoto.catalogo.validate()?;
    Ok(UpdateCheck {
        verificado_em: crate::util::now_unix(),
        app: semver_gt(&remoto.app.versao, app_atual).then(|| remoto.app.clone()),
        motor: (remoto.motores.llama_cpp.numero > motor_atual.llama_cpp.numero).then(|| remoto.motores.llama_cpp.versao.clone()),
        catalogo: (remoto.catalogo.versao > catalogo_atual.versao).then_some(remoto.catalogo.versao),
        remoto: Some(remoto),
    })
}

pub async fn fetch(client: &reqwest::Client, url: &str) -> Result<RemoteManifest, String> {
    let r = client.get(url).send().await.map_err(|e| format!("não consegui procurar atualizações ({e})"))?;
    if !r.status().is_success() {
        return Err(format!("o servidor de atualizações respondeu {}", r.status().as_u16()));
    }
    r.json::<RemoteManifest>().await.map_err(|e| format!("manifesto de atualização inválido ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versoes() {
        assert!(semver_gt("0.10.0", "0.9.9"));
        assert!(semver_gt("v1.0.0", "0.99.0"));
        assert!(!semver_gt("0.1.0", "0.1.0"));
    }

    #[test]
    fn compara_com_atual() {
        let mut m = EngineManifest::embedded();
        let c = Catalog::embedded();
        let mut remoto = RemoteManifest {
            versao_manifesto: 1,
            app: AppInfo { versao: "0.1.0".into(), pagina: "https://x".into(), notas: String::new() },
            motores: m.clone(),
            catalogo: c.clone(),
        };
        let r = compare(remoto.clone(), "0.1.0", &m, &c).unwrap();
        assert!(!r.tem_algo());
        remoto.app.versao = "0.2.0".into();
        remoto.motores.llama_cpp.numero += 10;
        remoto.motores.llama_cpp.versao = "b99999".into();
        remoto.catalogo.versao += 1;
        let r = compare(remoto.clone(), "0.1.0", &m, &c).unwrap();
        assert_eq!(r.app.unwrap().versao, "0.2.0");
        assert_eq!(r.motor.as_deref(), Some("b99999"));
        assert_eq!(r.catalogo, Some(c.versao + 1));
        // manifesto com hash inválido é recusado
        m.variantes[0].arquivos[0].sha256 = "xyz".into();
        remoto.motores = m.clone();
        assert!(compare(remoto, "0.1.0", &m, &c).is_err());
    }
}
