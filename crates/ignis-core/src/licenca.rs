//! Uso comercial de cada modelo (pedido da Fornalha): "sim", "nao" ou "condicional".
//!
//! - **sim**: a licença deixa usar em produto pago sem condição especial (Apache 2.0, MIT, BSD...).
//! - **nao**: proíbe uso comercial (CC BY-NC, licenças "research"...).
//! - **condicional**: deixa, mas com condições (limite de usuários, lista de usos proibidos,
//!   compartilhar igual...) **ou a licença não foi informada**. Nos dois casos, leia a licença
//!   antes de vender. A `nota` explica qual é o caso.
//!
//! Isto é uma leitura automática do nome da licença, não um parecer jurídico.

use serde::Serialize;

use crate::catalog::CatalogModel;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsoComercial {
    /// "sim", "nao" ou "condicional".
    pub uso_comercial: &'static str,
    /// O nome da licença como veio (catálogo, Hugging Face ou o próprio arquivo GGUF).
    pub licenca: Option<String>,
    /// Explicação curta, em português.
    pub nota: String,
}

pub const SIM: &str = "sim";
pub const NAO: &str = "nao";
pub const CONDICIONAL: &str = "condicional";

/// Converte o antigo `uso_comercial: true/false` (voz e transcrição) para o texto.
pub fn de_bool(b: Option<bool>) -> Option<&'static str> {
    b.map(|b| if b { SIM } else { NAO })
}

fn normalizar(s: &str) -> String {
    s.trim().to_lowercase().replace(['_', ' '], "-")
}

/// Classifica pelo nome da licença (ex.: "apache-2.0", "Apache 2.0", "cc-by-nc-4.0", "llama3.1").
pub fn classificar(licenca: Option<&str>) -> UsoComercial {
    let original = licenca.map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let Some(l) = original.as_deref().map(normalizar) else {
        return UsoComercial {
            uso_comercial: CONDICIONAL,
            licenca: None,
            nota: "Licença não informada no modelo: confira a página dele antes de usar num produto pago.".into(),
        };
    };
    let tem = |p: &str| l.contains(p);
    let (uso, nota): (&'static str, &str) = if tem("non-commercial")
        || tem("noncommercial")
        || tem("nao-comercial")
        || tem("não-comercial")
        || (tem("-nc") && !tem("ncsa"))
        || l.starts_with("nc-")
        || tem("research")
        || tem("mnpl")
        || tem("mrl")
        || tem("nvidia-open-model-license-nc")
    {
        (NAO, "A licença proíbe uso comercial: só para uso pessoal, estudo ou pesquisa.")
    } else if tem("apache")
        || l == "mit"
        || l.starts_with("mit-")
        || l.starts_with("mit/")
        || l.starts_with("bsd")
        || tem("cc0")
        || l == "cc-by-4.0"
        || l == "cc-by-3.0"
        || l == "cc-by-2.0"
        || l == "unlicense"
        || l == "isc"
        || l == "zlib"
        || l.starts_with("mpl-")
        || l == "bsl-1.0"
        || l == "artistic-2.0"
        || l == "postgresql"
    {
        (SIM, "Licença permissiva: pode usar em produto pago, mantendo o aviso de licença.")
    } else if tem("llama") {
        (CONDICIONAL, "Licença da Meta: uso comercial permitido, com limite de usuários e lista de usos proibidos.")
    } else if tem("gemma") {
        (CONDICIONAL, "Termos do Gemma (Google): uso comercial permitido, com lista de usos proibidos.")
    } else if tem("openrail") {
        (CONDICIONAL, "OpenRAIL: uso comercial permitido, com lista de usos proibidos que passa para quem usar.")
    } else if tem("qwen") || tem("tongyi") {
        (CONDICIONAL, "Licença do Qwen: uso comercial permitido, mas serviços muito grandes precisam de autorização.")
    } else if tem("gpl") || tem("-sa") {
        (CONDICIONAL, "Licença copyleft: pode vender, mas o que for derivado precisa sair na mesma licença.")
    } else if l == "other" || l == "unknown" || l == "custom" {
        (CONDICIONAL, "Licença própria do autor: leia a página do modelo antes de usar num produto pago.")
    } else {
        (CONDICIONAL, "Licença que o Ignis não reconhece: leia a página do modelo antes de usar num produto pago.")
    };
    UsoComercial { uso_comercial: uso, licenca: original, nota: nota.into() }
}

/// Do catálogo: usa o campo `uso_comercial` dele quando existir (escolhido a dedo),
/// senão a leitura pelo nome da licença.
pub fn do_catalogo(c: &CatalogModel) -> UsoComercial {
    let auto = classificar(Some(&c.licenca));
    match c.uso_comercial.as_deref() {
        Some(SIM) => UsoComercial { uso_comercial: SIM, ..auto },
        Some(NAO) => UsoComercial { uso_comercial: NAO, ..auto },
        Some(CONDICIONAL) => UsoComercial { uso_comercial: CONDICIONAL, ..auto },
        _ => auto,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licencas_conhecidas() {
        for l in ["apache-2.0", "Apache 2.0", "MIT", "mit", "bsd-3-clause", "cc0-1.0", "cc-by-4.0"] {
            assert_eq!(classificar(Some(l)).uso_comercial, SIM, "{l}");
        }
        for l in ["cc-by-nc-4.0", "CC BY-NC 4.0 (uso não comercial)", "cc-by-nc-sa-4.0", "qwen-research", "mnpl-0.1", "mrl"] {
            assert_eq!(classificar(Some(l)).uso_comercial, NAO, "{l}");
        }
        for l in ["llama3.1", "llama3.2", "gemma", "Termos de uso do Gemma (Google)", "openrail++", "creativeml-openrail-m", "other", "gpl-3.0", "cc-by-sa-4.0", "qwen", "xyz"] {
            assert_eq!(classificar(Some(l)).uso_comercial, CONDICIONAL, "{l}");
        }
        let sem = classificar(None);
        assert_eq!(sem.uso_comercial, CONDICIONAL);
        assert!(sem.licenca.is_none() && sem.nota.contains("não informada"));
        assert_eq!(classificar(Some("  ")).licenca, None);
    }

    #[test]
    fn catalogo_inteiro_tem_uso_comercial() {
        let c = crate::catalog::Catalog::embedded();
        for m in &c.modelos {
            let u = do_catalogo(m);
            assert!(m.uso_comercial.is_some(), "{} sem uso_comercial no catálogo", m.id);
            assert_eq!(Some(u.uso_comercial), m.uso_comercial.as_deref(), "{}", m.id);
        }
        assert_eq!(do_catalogo(c.get("qwen3-8b").unwrap()).uso_comercial, SIM);
        assert_eq!(do_catalogo(c.get("gemma3-4b").unwrap()).uso_comercial, CONDICIONAL);
    }
}
