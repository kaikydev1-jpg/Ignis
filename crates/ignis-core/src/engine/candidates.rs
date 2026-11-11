//! Monta a lista de motores candidatos para a placa da pessoa.
//!
//! AMD → ROCm/HIP (se a placa e o sistema suportarem) e Vulkan
//! NVIDIA → CUDA e Vulkan
//! Intel e outras → Vulkan
//! Sempre → Processador

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::manifest::{Backend, EngineManifest, Variant};
use crate::distro::{Familia, PACOTES_ROCM, PACOTES_VULKAN};
use crate::gpu::{Detection, Gpu, Vendor};

pub const VARIANTE_PROPRIA: &str = "proprio";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub variante: String,
    pub backend: Backend,
    pub nome: String,
    /// Pode ser testado/usado nesta máquina.
    pub disponivel: bool,
    /// Explicação curta (por que entra ou por que não).
    pub motivo: String,
    /// O que fazer para ficar disponível.
    pub dica: Option<String>,
    /// Tamanho do download (0 = já vem com o sistema / próprio).
    pub tamanho_download: u64,
}

/// Compara versões "580.95.05" ≥ "580.00".
pub fn version_ge(have: &str, min: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.split(['.', '-', ' ']).map(|p| p.parse::<u64>().unwrap_or(0)).collect() };
    let (a, b) = (parse(have), parse(min));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    true
}

/// No Windows sem nvidia-smi, a versão da NVIDIA sai da versão do driver DXGI:
/// 32.0.15.8157 → 581.57
pub fn nvidia_version_from_windows_umd(umd: &str) -> Option<String> {
    let parts: Vec<&str> = umd.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let digits = format!("{}{:0>4}", parts[2], parts[3]);
    if digits.len() < 5 {
        return None;
    }
    let last5 = &digits[digits.len() - 5..];
    Some(format!("{}.{}", &last5[..3], &last5[3..]))
}

fn nvidia_driver(g: &Gpu, windows: bool) -> Option<String> {
    let v = g.versao_driver.clone()?;
    if windows && v.split('.').count() == 4 {
        return nvidia_version_from_windows_umd(&v);
    }
    Some(v)
}

fn hint_for(det: &Detection, v: &Variant) -> Option<String> {
    let base = v.dica_falta.clone();
    let pacotes = match v.pacotes.as_deref() {
        Some("vulkan") => Some(&PACOTES_VULKAN),
        Some("rocm") => Some(&PACOTES_ROCM),
        _ => None,
    };
    let cmd = pacotes.and_then(|p| det.sistema.comando_instalar(p));
    match (base, cmd) {
        (Some(b), Some(c)) => Some(format!("{b} Para instalar: {c}")),
        (Some(b), None) => Some(b),
        (None, Some(c)) => Some(format!("Para instalar: {c}")),
        (None, None) => None,
    }
}

fn missing_libs(det: &Detection, v: &Variant) -> Vec<String> {
    v.libs_do_sistema.iter().filter(|l| !det.lib(l)).cloned().collect()
}

pub fn build(det: &Detection, manifest: &EngineManifest, proprio: Option<&Path>) -> Vec<Candidate> {
    let windows = det.sistema.familia == Familia::Windows;
    let so = match det.sistema.familia {
        Familia::Windows => "windows",
        Familia::Mac => "macos",
        _ => "linux",
    };
    let variants: Vec<&Variant> = manifest.for_platform(so, &det.arq).collect();
    let find = |b: Backend| variants.iter().filter(|v| v.backend == b).copied().collect::<Vec<_>>();
    let principal = det.principal();
    let tem = |vendor: Vendor| det.placas.iter().any(|g| g.fabricante == vendor && (!g.integrada || principal.map(|p| p.integrada).unwrap_or(false)));
    let mut out: Vec<Candidate> = Vec::new();

    let mk = |v: &Variant, disponivel: bool, motivo: String, dica: Option<String>| Candidate {
        variante: v.id.clone(),
        backend: v.backend,
        nome: v.backend.nome().to_string(),
        disponivel,
        motivo,
        dica,
        tamanho_download: v.tamanho_total(),
    };

    // CUDA
    if tem(Vendor::Nvidia) {
        let g = det.placas.iter().find(|g| g.fabricante == Vendor::Nvidia).unwrap();
        let driver = nvidia_driver(g, windows);
        let mut cuda: Vec<&Variant> = find(Backend::Cuda);
        // mais nova primeiro
        cuda.sort_by(|a, b| {
            let (x, y) = (a.driver_minimo.clone().unwrap_or_default(), b.driver_minimo.clone().unwrap_or_default());
            if version_ge(&x, &y) && x != y { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }
        });
        let escolhida = cuda.iter().find(|v| match (&driver, &v.driver_minimo) {
            (Some(d), Some(min)) => version_ge(d, min),
            (None, _) => false,
            (Some(_), None) => true,
        });
        match (escolhida, cuda.last()) {
            (Some(v), _) => {
                let falta = missing_libs(det, v);
                if falta.is_empty() {
                    out.push(mk(v, true, format!("Placa NVIDIA com driver {}", driver.clone().unwrap_or_default()), None));
                } else {
                    out.push(mk(v, false, format!("Falta a biblioteca do driver NVIDIA ({})", falta.join(", ")), v.dica_falta.clone()));
                }
            }
            (None, Some(v)) => {
                let motivo = match &driver {
                    Some(d) => format!("Driver NVIDIA {d} é antigo demais para o CUDA"),
                    None => "Não consegui ler a versão do driver NVIDIA".to_string(),
                };
                out.push(mk(v, false, motivo, v.dica_falta.clone()));
            }
            _ => {}
        }
    }

    // ROCm
    if let Some(g) = det.placas.iter().find(|g| g.fabricante == Vendor::Amd && (!g.integrada || principal.map(|p| p.integrada).unwrap_or(false))) {
        if let Some(v) = find(Backend::Rocm).first() {
            let gfx = g.alvo_gfx.clone().unwrap_or_default();
            let compilado = v.alvos_gfx.iter().any(|x| *x == gfx);
            let oficial = v.alvos_gfx_oficiais.as_ref().map(|l| l.iter().any(|x| *x == gfx)).unwrap_or(true);
            let c = if gfx.is_empty() {
                mk(v, false, "Não consegui descobrir o modelo exato da placa AMD (alvo gfx)".into(), None)
            } else if !compilado {
                mk(v, false, format!("Sua placa ({gfx}) não é suportada pelo ROCm; o Vulkan funciona nela"), None)
            } else if windows && !oficial {
                mk(v, false, format!("Sua placa ({gfx}) não está na lista oficial do ROCm para Windows; o Vulkan será usado"), None)
            } else if det.sistema.flatpak {
                mk(
                    v,
                    false,
                    "Dentro do Flatpak o ROCm do sistema não fica visível; o Vulkan será usado".into(),
                    Some("Para usar ROCm, instale o Ignis pelo pacote RPM, .deb ou AppImage.".into()),
                )
            } else if !missing_libs(det, v).is_empty() {
                mk(v, false, "O ROCm do sistema não está instalado (ou é de outra versão)".into(), hint_for(det, v))
            } else if !windows && !det.kfd_acessivel {
                mk(
                    v,
                    false,
                    "Seu usuário não tem acesso à placa para computação (/dev/kfd)".into(),
                    Some("Rode: sudo usermod -aG render,video $USER — depois saia e entre de novo na sessão.".into()),
                )
            } else {
                mk(v, true, format!("Placa AMD {gfx} suportada pelo ROCm"), None)
            };
            out.push(c);
        }
    }

    // Vulkan
    if principal.is_some() {
        if let Some(v) = find(Backend::Vulkan).first() {
            let falta = missing_libs(det, v);
            if falta.is_empty() {
                let quem = principal.map(|g| g.fabricante.nome()).unwrap_or("");
                out.push(mk(v, true, format!("Funciona em placas {quem} com o driver do sistema"), None));
            } else {
                out.push(mk(v, false, format!("Falta {}", falta.join(", ")), hint_for(det, v)));
            }
        }
    }

    // Próprio
    if let Some(p) = proprio {
        let existe = p.is_file();
        out.push(Candidate {
            variante: VARIANTE_PROPRIA.into(),
            backend: Backend::Proprio,
            nome: Backend::Proprio.nome().into(),
            disponivel: existe,
            motivo: if existe { format!("Indicado por você: {}", p.display()) } else { format!("Arquivo não encontrado: {}", p.display()) },
            dica: (!existe).then(|| "Confira o caminho em Motor → llama-server próprio.".to_string()),
            tamanho_download: 0,
        });
    }

    // Processador
    if let Some(v) = find(Backend::Cpu).first() {
        out.push(mk(v, true, "Funciona em qualquer computador (mais lento)".into(), None));
    }

    out.sort_by_key(|c| (c.backend.prioridade(), c.variante.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compara_versoes() {
        assert!(version_ge("580.95.05", "580.00"));
        assert!(!version_ge("575.64", "580.00"));
        assert!(version_ge("527.41", "527.41"));
        assert!(version_ge("531.18", "527.41"));
    }

    #[test]
    fn versao_nvidia_pelo_driver_do_windows() {
        assert_eq!(nvidia_version_from_windows_umd("32.0.15.8157").as_deref(), Some("581.57"));
        assert_eq!(nvidia_version_from_windows_umd("31.0.15.5222").as_deref(), Some("552.22"));
    }
}
