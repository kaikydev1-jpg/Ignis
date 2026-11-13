//! Em que ordem tentar os motores para um modelo.
//!
//! O teste de velocidade do "Preparar" roda com um modelo pequeno. Para modelos
//! pequenos ele vale; para modelos grandes, não: numa RX 580 o processador ganhou
//! no modelo de 0,6B (68 contra 37 tokens/s), mas no Qwen3 8B a placa (Vulkan) é
//! duas vezes mais rápida. Por isso a ordem leva o modelo em conta:
//!
//! 1. motor escolhido à mão vem primeiro; motor que falhou nesta sessão, por último;
//! 2. se já medimos ESTE modelo em dois ou mais motores (uso real ou "Testar com
//!    este modelo"), vale o mais rápido medido;
//! 3. modelo do tamanho do modelo de teste (até 2x): vale a ordem do teste;
//! 4. modelo maior: a placa vem antes se pelo menos metade das camadas couber nela;
//!    se couber menos que isso, o processador vem antes.

use std::collections::HashMap;

/// Um motor candidato, visto para um modelo específico.
#[derive(Debug, Clone)]
pub struct Opcao {
    pub variante: String,
    pub usa_placa: bool,
    pub forcado: bool,
    pub falhou: bool,
    /// Posição no teste de velocidade (0 = mais rápido). Sem teste: prioridade do backend.
    pub pos_teste: u32,
    pub prioridade: u8,
    /// Fração das camadas deste modelo que cabem na placa com este motor (0 a 1).
    pub fracao_na_placa: f32,
}

/// Por que a ordem ficou assim (mostrado na janela).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motivo {
    Forcado,
    MedidoNesteModelo,
    TestePequenoVale,
    CabeNaPlaca,
    PoucoNaPlaca,
    SemPlaca,
}

impl Motivo {
    pub fn texto(self) -> &'static str {
        match self {
            // completam a frase "escolhido porque …"
            Motivo::Forcado => "você escolheu este motor",
            Motivo::MedidoNesteModelo => "foi o mais rápido medido com este modelo",
            Motivo::TestePequenoVale => "foi o mais rápido no teste de velocidade",
            Motivo::CabeNaPlaca => "o modelo cabe na placa",
            Motivo::PoucoNaPlaca => "pouco do modelo cabe na placa",
            Motivo::SemPlaca => "não há placa de vídeo para este modelo",
        }
    }
}

/// O modelo é parecido com o do teste (o teste vale para ele)?
pub fn parecido_com_o_teste(tamanho_modelo: u64, tamanho_teste: Option<u64>) -> bool {
    tamanho_teste.is_some_and(|t| t > 0 && tamanho_modelo <= t.saturating_mul(2))
}

/// Ordena as opções para um modelo. `medidas`: tokens/s escrevendo, por variante, deste modelo.
pub fn ordenar(opcoes: &[Opcao], tamanho_modelo: u64, tamanho_teste: Option<u64>, medidas: &HashMap<String, f64>) -> (Vec<String>, Motivo) {
    let medidas_validas: HashMap<&str, f64> =
        opcoes.iter().filter(|o| !o.falhou).filter_map(|o| medidas.get(&o.variante).map(|v| (o.variante.as_str(), *v))).collect();
    let usa_medidas = medidas_validas.len() >= 2;
    let pequeno = parecido_com_o_teste(tamanho_modelo, tamanho_teste);
    // grupo 0 vem primeiro: tudo (modelo pequeno, vale o teste) ou as placas onde cabe metade ou mais
    let grupo = |o: &Opcao| -> u8 {
        if pequeno || (o.usa_placa && o.fracao_na_placa >= 0.5) {
            0
        } else if !o.usa_placa {
            1
        } else {
            2
        }
    };
    let mut v: Vec<&Opcao> = opcoes.iter().collect();
    v.sort_by(|a, b| {
        let chave = |o: &Opcao| (o.falhou, !o.forcado);
        chave(a).cmp(&chave(b)).then_with(|| {
            if usa_medidas {
                // medidos primeiro, do mais rápido para o mais lento
                let (ma, mb) = (medidas_validas.get(a.variante.as_str()), medidas_validas.get(b.variante.as_str()));
                match (ma, mb) {
                    (Some(x), Some(y)) => y.partial_cmp(x).unwrap_or(std::cmp::Ordering::Equal),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => (grupo(a), a.pos_teste, a.prioridade).cmp(&(grupo(b), b.pos_teste, b.prioridade)),
                }
            } else {
                (grupo(a), a.pos_teste, a.prioridade).cmp(&(grupo(b), b.pos_teste, b.prioridade))
            }
        })
    });
    let ordem: Vec<String> = v.iter().map(|o| o.variante.clone()).collect();
    let motivo = match v.first() {
        Some(p) if p.forcado => Motivo::Forcado,
        Some(_) if usa_medidas => Motivo::MedidoNesteModelo,
        Some(_) if pequeno => Motivo::TestePequenoVale,
        Some(p) if p.usa_placa && p.fracao_na_placa >= 0.5 => Motivo::CabeNaPlaca,
        Some(_) if opcoes.iter().any(|o| o.usa_placa && !o.falhou) => Motivo::PoucoNaPlaca,
        _ => Motivo::SemPlaca,
    };
    (ordem, motivo)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    fn op(v: &str, placa: bool, pos: u32, fracao: f32) -> Opcao {
        Opcao {
            variante: v.into(),
            usa_placa: placa,
            forcado: false,
            falhou: false,
            pos_teste: pos,
            prioridade: if placa { 1 } else { 9 },
            fracao_na_placa: if placa { fracao } else { 0.0 },
        }
    }

    /// RX 580 do relatório do Valkode: no teste pequeno o processador ganhou.
    fn rx580(fracao_8b: f32) -> Vec<Opcao> {
        vec![op("windows-cpu", false, 0, 0.0), op("windows-vulkan", true, 1, fracao_8b)]
    }

    #[test]
    fn modelo_grande_que_cabe_vai_para_a_placa_mesmo_com_o_teste_dizendo_processador() {
        let (ordem, motivo) = ordenar(&rx580(1.0), 5 * GB, Some(610_000_000), &HashMap::new());
        assert_eq!(ordem, vec!["windows-vulkan", "windows-cpu"]);
        assert_eq!(motivo, Motivo::CabeNaPlaca);
    }

    #[test]
    fn modelo_pequeno_segue_o_teste() {
        let (ordem, motivo) = ordenar(&rx580(1.0), 700_000_000, Some(610_000_000), &HashMap::new());
        assert_eq!(ordem, vec!["windows-cpu", "windows-vulkan"]);
        assert_eq!(motivo, Motivo::TestePequenoVale);
    }

    #[test]
    fn pouco_na_placa_prefere_o_processador() {
        let (ordem, motivo) = ordenar(&rx580(0.3), 20 * GB, Some(610_000_000), &HashMap::new());
        assert_eq!(ordem, vec!["windows-cpu", "windows-vulkan"]);
        assert_eq!(motivo, Motivo::PoucoNaPlaca);
        // metade ou mais na placa: placa primeiro
        assert_eq!(ordenar(&rx580(0.5), 9 * GB, Some(610_000_000), &HashMap::new()).0[0], "windows-vulkan");
    }

    #[test]
    fn medida_com_o_proprio_modelo_vence_a_regra() {
        let mut m = HashMap::new();
        m.insert("windows-cpu".to_string(), 8.1);
        m.insert("windows-vulkan".to_string(), 16.1);
        let (ordem, motivo) = ordenar(&rx580(0.3), 20 * GB, Some(610_000_000), &m);
        assert_eq!(ordem, vec!["windows-vulkan", "windows-cpu"]);
        assert_eq!(motivo, Motivo::MedidoNesteModelo);
        // uma medida só não basta para decidir
        m.remove("windows-vulkan");
        assert_eq!(ordenar(&rx580(1.0), 5 * GB, Some(610_000_000), &m).1, Motivo::CabeNaPlaca);
    }

    #[test]
    fn escolhido_a_mao_primeiro_e_falhou_por_ultimo() {
        let mut ops = rx580(1.0);
        ops[0].forcado = true;
        let (ordem, motivo) = ordenar(&ops, 5 * GB, Some(610_000_000), &HashMap::new());
        assert_eq!((ordem[0].as_str(), motivo), ("windows-cpu", Motivo::Forcado));
        let mut ops = rx580(1.0);
        ops[1].falhou = true;
        assert_eq!(ordenar(&ops, 5 * GB, Some(610_000_000), &HashMap::new()).0, vec!["windows-cpu", "windows-vulkan"]);
    }

    #[test]
    fn varias_placas_seguem_o_teste_entre_si() {
        // AMD com ROCm e Vulkan: entre as duas que cabem, vale o teste
        let ops = vec![op("linux-cpu", false, 0, 0.0), op("linux-vulkan", true, 2, 1.0), op("linux-rocm", true, 1, 1.0)];
        assert_eq!(ordenar(&ops, 5 * GB, Some(610_000_000), &HashMap::new()).0, vec!["linux-rocm", "linux-vulkan", "linux-cpu"]);
    }

    #[test]
    fn sem_teste_usa_a_regra_da_placa() {
        let (ordem, _) = ordenar(&rx580(1.0), 700_000_000, None, &HashMap::new());
        assert_eq!(ordem[0], "windows-vulkan");
        let so_cpu = vec![op("linux-cpu", false, 0, 0.0)];
        assert_eq!(ordenar(&so_cpu, 5 * GB, None, &HashMap::new()).1, Motivo::SemPlaca);
    }
}
