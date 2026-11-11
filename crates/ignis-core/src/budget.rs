//! Orçamento de memória: quantas camadas vão para a placa e qual tamanho de
//! contexto cabe na VRAM livre. As contas são estimativas conservadoras.

use serde::{Deserialize, Serialize};

const MB: u64 = 1024 * 1024;
/// Contexto mínimo aceitável.
pub const CONTEXTO_MINIMO: u32 = 2048;
/// Contexto usado quando o modelo não cabe inteiro na placa.
const CONTEXTO_DIVIDIDO: u32 = 4096;
/// Folga na VRAM para o sistema, a tela e outros programas.
const FOLGA_VRAM_MB: u64 = 300;
/// Folga na RAM.
const FOLGA_RAM_MB: u64 = 1024;

/// Forma do modelo (do GGUF ou do catálogo).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelShape {
    pub tamanho_arquivo: u64,
    pub camadas: u32,
    pub kv_bytes_por_token: u64,
    pub contexto_max: u32,
    pub embd: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Recursos {
    pub usa_placa: bool,
    pub vram_livre_mb: Option<u64>,
    pub ram_livre_mb: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Encaixe {
    /// Tudo na placa: roda bem.
    Placa,
    /// Parte na placa, parte no processador: roda, mais devagar.
    Dividido,
    /// Só no processador.
    Processador,
    /// Não cabe na memória disponível.
    NaoCabe,
}

impl Encaixe {
    pub fn rotulo(&self) -> &'static str {
        match self {
            Encaixe::Placa => "Roda bem na sua placa",
            Encaixe::Dividido => "Roda, mas mais devagar",
            Encaixe::Processador => "Só no processador (lento)",
            Encaixe::NaoCabe => "Não cabe neste computador",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plano {
    pub camadas_na_placa: u32,
    pub todas_na_placa: bool,
    pub contexto: u32,
    pub encaixe: Encaixe,
    /// Memória estimada na placa e na RAM (MB).
    pub memoria_placa_mb: u64,
    pub memoria_ram_mb: u64,
    pub aviso: Option<String>,
}

impl Plano {
    /// Valor para `-ngl` do llama-server.
    pub fn ngl_arg(&self) -> String {
        if self.todas_na_placa {
            "999".into()
        } else {
            self.camadas_na_placa.to_string()
        }
    }
}

/// Memória extra para buffers de cálculo (estimativa).
fn overhead_mb(ctx: u32, embd: u32) -> u64 {
    350 + (ctx as u64 * embd.max(1) as u64 * 4) / MB
}

fn kv_mb(shape: &ModelShape, ctx: u32) -> u64 {
    (shape.kv_bytes_por_token * ctx as u64).div_ceil(MB)
}

fn gb(mb: u64) -> String {
    format!("{:.1}", mb as f64 / 1024.0).replace('.', ",")
}

/// Contextos a tentar, do maior para o menor.
fn ctx_steps(cap: u32) -> Vec<u32> {
    let mut v = vec![cap];
    let mut c = cap;
    while c / 2 >= CONTEXTO_MINIMO {
        c /= 2;
        v.push(c);
    }
    v
}

pub fn plan(shape: &ModelShape, rec: &Recursos, contexto_preferido: u32) -> Plano {
    let tamanho_mb = shape.tamanho_arquivo.div_ceil(MB);
    let cap = if shape.contexto_max > 0 { contexto_preferido.min(shape.contexto_max) } else { contexto_preferido };
    let cap = cap.max(CONTEXTO_MINIMO.min(shape.contexto_max.max(CONTEXTO_MINIMO)));
    let camadas = shape.camadas.max(1);

    let so_processador = |ctx: u32, motivo: Option<&str>| -> Plano {
        let need = tamanho_mb + kv_mb(shape, ctx) + overhead_mb(ctx, shape.embd);
        let cabe = need + FOLGA_RAM_MB <= rec.ram_livre_mb;
        let aviso = if cabe {
            motivo.map(|m| format!("{m} O modelo vai rodar só no processador (mais lento)."))
        } else {
            Some(format!(
                "Este modelo precisa de cerca de {} GB e o computador tem {} GB livres. Ele pode ficar muito lento ou não abrir; escolha um modelo menor.",
                gb(need),
                gb(rec.ram_livre_mb)
            ))
        };
        Plano {
            camadas_na_placa: 0,
            todas_na_placa: false,
            contexto: ctx,
            encaixe: if cabe { Encaixe::Processador } else { Encaixe::NaoCabe },
            memoria_placa_mb: 0,
            memoria_ram_mb: need,
            aviso,
        }
    };

    let vram = match (rec.usa_placa, rec.vram_livre_mb) {
        (true, Some(v)) if v > FOLGA_VRAM_MB => v - FOLGA_VRAM_MB,
        (true, _) => return so_processador(cap.min(CONTEXTO_DIVIDIDO.max(CONTEXTO_MINIMO)), Some("Não consegui medir a memória livre da placa.")),
        (false, _) => return so_processador(cap, None),
    };

    // 1) Tudo na placa, com o maior contexto que couber.
    for ctx in ctx_steps(cap) {
        let need = tamanho_mb + kv_mb(shape, ctx) + overhead_mb(ctx, shape.embd);
        if need <= vram {
            let aviso = (ctx < cap).then(|| format!("Contexto reduzido para {ctx} para caber na memória da placa."));
            return Plano {
                camadas_na_placa: camadas,
                todas_na_placa: true,
                contexto: ctx,
                encaixe: Encaixe::Placa,
                memoria_placa_mb: need,
                memoria_ram_mb: 0,
                aviso,
            };
        }
    }

    // 2) Dividido entre placa e processador.
    let ctx = cap.min(CONTEXTO_DIVIDIDO).max(CONTEXTO_MINIMO.min(cap));
    let por_camada = tamanho_mb as f64 / (camadas as f64 + 1.0);
    let kv_camada = kv_mb(shape, ctx) as f64 / camadas as f64;
    let disponivel = vram.saturating_sub(overhead_mb(ctx, shape.embd)) as f64;
    let n = ((disponivel / (por_camada + kv_camada)).floor() as i64).clamp(0, camadas as i64) as u32;
    if n == 0 {
        return so_processador(ctx, Some("Não sobra memória na placa para este modelo."));
    }
    let placa = (n as f64 * (por_camada + kv_camada)) as u64 + overhead_mb(ctx, shape.embd);
    let ram = ((camadas - n) as f64 * (por_camada + kv_camada) + por_camada) as u64;
    if ram + FOLGA_RAM_MB > rec.ram_livre_mb {
        return Plano {
            camadas_na_placa: n,
            todas_na_placa: false,
            contexto: ctx,
            encaixe: Encaixe::NaoCabe,
            memoria_placa_mb: placa,
            memoria_ram_mb: ram,
            aviso: Some(format!(
                "Este modelo não cabe: precisa de mais {} GB além da placa, e o computador tem {} GB livres. Escolha um modelo menor.",
                gb(ram),
                gb(rec.ram_livre_mb)
            )),
        };
    }
    Plano {
        camadas_na_placa: n,
        todas_na_placa: false,
        contexto: ctx,
        encaixe: Encaixe::Dividido,
        memoria_placa_mb: placa,
        memoria_ram_mb: ram,
        aviso: Some(format!(
            "O modelo não cabe inteiro na placa: {n} de {camadas} camadas vão na placa e o resto no processador. Funciona, mas mais devagar."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(size: u64, camadas: u32, kv_kb: u64, embd: u32, ctx_max: u32) -> ModelShape {
        ModelShape { tamanho_arquivo: size, camadas, kv_bytes_por_token: kv_kb * 1024, contexto_max: ctx_max, embd }
    }
    fn placa(livre: u64) -> Recursos {
        Recursos { usa_placa: true, vram_livre_mb: Some(livre), ram_livre_mb: 12_000 }
    }
    const QWEN3_8B: (u64, u32, u64, u32) = (5_027_783_488, 36, 144, 4096);
    const QWEN3_14B: (u64, u32, u64, u32) = (9_001_752_960, 40, 160, 5120);
    const QWEN3_4B: (u64, u32, u64, u32) = (2_497_280_256, 36, 144, 2560);

    #[test]
    fn qwen3_8b_cabe_inteiro_na_rx580_8gb() {
        let (s, l, kv, e) = QWEN3_8B;
        let p = plan(&shape(s, l, kv, e, 40960), &placa(8192 - 310), 8192);
        assert_eq!(p.encaixe, Encaixe::Placa);
        assert!(p.todas_na_placa);
        assert_eq!(p.contexto, 8192);
        assert_eq!(p.ngl_arg(), "999");
        assert!(p.aviso.is_none());
    }

    #[test]
    fn qwen3_14b_divide_numa_placa_de_8gb() {
        let (s, l, kv, e) = QWEN3_14B;
        let p = plan(&shape(s, l, kv, e, 40960), &placa(7774), 8192);
        assert_eq!(p.encaixe, Encaixe::Dividido);
        assert!(p.camadas_na_placa > 20 && p.camadas_na_placa < 40, "{}", p.camadas_na_placa);
        assert_eq!(p.contexto, 4096);
        assert!(p.aviso.unwrap().contains("camadas vão na placa"));
    }

    #[test]
    fn qwen3_14b_cabe_na_rx9060xt_16gb() {
        let (s, l, kv, e) = QWEN3_14B;
        let p = plan(&shape(s, l, kv, e, 40960), &placa(15684), 8192);
        assert_eq!(p.encaixe, Encaixe::Placa);
        assert_eq!(p.contexto, 8192);
    }

    #[test]
    fn reduz_contexto_numa_placa_de_4gb() {
        let (s, l, kv, e) = QWEN3_4B;
        let p = plan(&shape(s, l, kv, e, 40960), &placa(3800), 8192);
        assert_eq!(p.encaixe, Encaixe::Placa);
        assert_eq!(p.contexto, 4096);
        assert!(p.aviso.unwrap().contains("Contexto reduzido"));
    }

    #[test]
    fn respeita_contexto_maximo_do_modelo() {
        let p = plan(&shape(400 * MB, 24, 12, 896, 2048), &placa(8000), 8192);
        assert_eq!(p.contexto, 2048);
    }

    #[test]
    fn sem_placa_usa_processador() {
        let (s, l, kv, e) = QWEN3_4B;
        let r = Recursos { usa_placa: false, vram_livre_mb: None, ram_livre_mb: 14_000 };
        let p = plan(&shape(s, l, kv, e, 40960), &r, 8192);
        assert_eq!(p.encaixe, Encaixe::Processador);
        assert_eq!(p.camadas_na_placa, 0);
        assert_eq!(p.ngl_arg(), "0");
    }

    #[test]
    fn modelo_grande_demais_nao_cabe() {
        let (s, l, kv, e) = QWEN3_14B;
        let r = Recursos { usa_placa: false, vram_livre_mb: None, ram_livre_mb: 6_000 };
        let p = plan(&shape(s, l, kv, e, 40960), &r, 8192);
        assert_eq!(p.encaixe, Encaixe::NaoCabe);
        assert!(p.aviso.unwrap().contains("escolha um modelo menor"));
        // na placa pequena e com pouca RAM também não cabe
        let r = Recursos { usa_placa: true, vram_livre_mb: Some(2000), ram_livre_mb: 4000 };
        assert_eq!(plan(&shape(s, l, kv, e, 40960), &r, 8192).encaixe, Encaixe::NaoCabe);
    }

    #[test]
    fn placa_sem_memoria_livre_vai_para_processador() {
        let (s, l, kv, e) = QWEN3_8B;
        let r = Recursos { usa_placa: true, vram_livre_mb: Some(500), ram_livre_mb: 14_000 };
        let p = plan(&shape(s, l, kv, e, 40960), &r, 8192);
        assert_eq!(p.encaixe, Encaixe::Processador);
    }
}
