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
/// Contexto automático: abaixo disto vale a folga normal; acima, a folga extra.
pub const CONTEXTO_AUTO_BASE: u32 = 8192;
/// Contexto automático: o maior que o Ignis tenta sozinho.
pub const CONTEXTO_AUTO_TETO: u32 = 32_768;
/// Contexto automático acima de 8.192: memória que fica livre para jogos e outros programas.
const FOLGA_EXTRA_AUTO_MB: u64 = 1024;

/// Tipo dos números do cache de contexto (KV cache) do llama-server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CacheKv {
    /// 16 bits (padrão do llama.cpp).
    #[default]
    #[serde(rename = "f16")]
    F16,
    /// 8 bits: ocupa pouco mais da metade e cabe quase o dobro de contexto.
    #[serde(rename = "q8_0")]
    Q8_0,
}

impl CacheKv {
    pub fn nome(self) -> &'static str {
        match self {
            CacheKv::F16 => "f16",
            CacheKv::Q8_0 => "q8_0",
        }
    }

    pub fn de_texto(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "f16" | "16" | "padrao" | "padrão" => Some(CacheKv::F16),
            "q8_0" | "q8" | "8" | "8bits" | "8 bits" => Some(CacheKv::Q8_0),
            _ => None,
        }
    }

    /// Bytes por número em relação ao f16: q8_0 guarda 32 números em 34 bytes (f16: 64).
    fn kv_mb(self, kv_f16_mb: u64) -> u64 {
        match self {
            CacheKv::F16 => kv_f16_mb,
            CacheKv::Q8_0 => (kv_f16_mb * 34).div_ceil(64),
        }
    }
}

/// O que pedir ao orçamento: um contexto (ou automático) e o tipo de cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PedidoContexto {
    /// `None` = automático: o maior que cabe com folga, entre 8.192 e 32.768.
    pub contexto: Option<u32>,
    pub cache: CacheKv,
}

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

fn kv_mb(shape: &ModelShape, ctx: u32, cache: CacheKv) -> u64 {
    cache.kv_mb((shape.kv_bytes_por_token * ctx as u64).div_ceil(MB))
}

/// Do maior para o menor, de 1.024 em 1.024 (o primeiro arredondado para múltiplo de 256).
fn passos_finos(teto: u32, piso: u32) -> Vec<u32> {
    let teto = (teto / 256) * 256;
    if teto < piso {
        return vec![];
    }
    let mut v = vec![teto];
    let mut c = (teto / 1024) * 1024;
    if c == teto {
        c = c.saturating_sub(1024);
    }
    while c >= piso && c > 0 {
        v.push(c);
        c -= 1024;
    }
    v
}

fn gb(mb: u64) -> String {
    format!("{:.1}", mb as f64 / 1024.0).replace('.', ",")
}

/// Memória estimada (MB) com o modelo inteiro na placa (ou na RAM): (total, só a memória da conversa).
pub fn memoria_estimada_mb(shape: &ModelShape, ctx: u32, cache: CacheKv) -> (u64, u64) {
    let kv = kv_mb(shape, ctx, cache);
    (shape.tamanho_arquivo.div_ceil(MB) + kv + overhead_mb(ctx, shape.embd), kv)
}

/// Como fica na memória livre: "folga" (sobra memória para jogos e outros programas),
/// "apertado" (cabe, mas sobra pouco) ou "nao-cabe" (o Ignis carrega o maior que couber).
pub fn situacao(total_mb: u64, livre_mb: u64, placa: bool) -> &'static str {
    let folga = if placa { FOLGA_VRAM_MB } else { FOLGA_RAM_MB };
    if total_mb + folga + FOLGA_EXTRA_AUTO_MB <= livre_mb {
        "folga"
    } else if total_mb + folga <= livre_mb {
        "apertado"
    } else {
        "nao-cabe"
    }
}

/// Tamanhos que a barra da janela oferece (até o máximo do treino do modelo).
pub fn pontos_da_barra(contexto_max: u32) -> Vec<u32> {
    let teto = if contexto_max > 0 { contexto_max } else { CONTEXTO_AUTO_TETO };
    let mut v: Vec<u32> = [2048, 4096, 8192, 12288, 16384, 24576, 32768, 49152, 65536, 98304, 131_072, 196_608, 262_144]
        .into_iter()
        .filter(|c| *c < teto)
        .collect();
    v.push(teto);
    v
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

fn limitar(shape: &ModelShape, contexto: u32) -> u32 {
    let cap = if shape.contexto_max > 0 { contexto.min(shape.contexto_max) } else { contexto };
    cap.max(CONTEXTO_MINIMO.min(shape.contexto_max.max(CONTEXTO_MINIMO)))
}

/// Plano com o contexto preferido das configurações e cache de 16 bits (rótulos "cabe na placa").
pub fn plan(shape: &ModelShape, rec: &Recursos, contexto_preferido: u32) -> Plano {
    let cap = limitar(shape, contexto_preferido);
    let tentativas: Vec<(u32, u64)> = ctx_steps(cap).into_iter().map(|c| (c, 0)).collect();
    plan_core(shape, rec, cap, CacheKv::F16, &tentativas)
}

/// Plano para carregar: contexto pedido (ou automático) e tipo de cache.
/// Pedido maior do que cabe não falha: fica com o maior que cabe.
pub fn plan_com(shape: &ModelShape, rec: &Recursos, pedido: &PedidoContexto) -> Plano {
    let treino = if shape.contexto_max > 0 { shape.contexto_max } else { CONTEXTO_AUTO_TETO };
    match pedido.contexto {
        Some(c) => {
            let cap = limitar(shape, c);
            let tentativas: Vec<(u32, u64)> = passos_finos(cap, CONTEXTO_MINIMO.min(cap)).into_iter().map(|c| (c, 0)).collect();
            plan_core(shape, rec, cap, pedido.cache, &tentativas)
        }
        None => {
            let base = limitar(shape, CONTEXTO_AUTO_BASE);
            let teto = treino.min(CONTEXTO_AUTO_TETO);
            let mut tentativas: Vec<(u32, u64)> =
                passos_finos(teto, base + 1).into_iter().filter(|c| *c > base).map(|c| (c, FOLGA_EXTRA_AUTO_MB)).collect();
            tentativas.extend(passos_finos(base, CONTEXTO_MINIMO.min(base)).into_iter().map(|c| (c, 0)));
            plan_core(shape, rec, base, pedido.cache, &tentativas)
        }
    }
}

/// Maior contexto que caberia agora (até o máximo do treino), com este cache.
pub fn contexto_possivel(shape: &ModelShape, rec: &Recursos, cache: CacheKv) -> Option<u32> {
    let treino = if shape.contexto_max > 0 { shape.contexto_max } else { CONTEXTO_AUTO_TETO };
    let p = plan_com(shape, rec, &PedidoContexto { contexto: Some(treino), cache });
    matches!(p.encaixe, Encaixe::Placa | Encaixe::Processador).then_some(p.contexto)
}

/// `tentativas`: contextos do maior para o menor, com a folga extra de cada um (MB).
/// `cap`: contexto de referência quando o modelo não cabe inteiro na placa.
fn plan_core(shape: &ModelShape, rec: &Recursos, cap: u32, cache: CacheKv, tentativas: &[(u32, u64)]) -> Plano {
    let tamanho_mb = shape.tamanho_arquivo.div_ceil(MB);
    let camadas = shape.camadas.max(1);

    let so_processador = |ctx: u32, motivo: Option<&str>| -> Plano {
        let need = tamanho_mb + kv_mb(shape, ctx, cache) + overhead_mb(ctx, shape.embd);
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
        (true, _) => {
            return so_processador(cap.min(CONTEXTO_DIVIDIDO.max(CONTEXTO_MINIMO)), Some("Não consegui medir a memória livre da placa."))
        }
        (false, _) => {
            // processador: o maior contexto das tentativas que cabe na RAM
            let ctx = tentativas
                .iter()
                .find(|(c, extra)| {
                    tamanho_mb + kv_mb(shape, *c, cache) + overhead_mb(*c, shape.embd) + FOLGA_RAM_MB + extra <= rec.ram_livre_mb
                })
                .map(|t| t.0)
                .unwrap_or(cap);
            return so_processador(ctx, None);
        }
    };

    // 1) Tudo na placa, com o maior contexto que couber.
    for &(ctx, extra) in tentativas {
        let need = tamanho_mb + kv_mb(shape, ctx, cache) + overhead_mb(ctx, shape.embd);
        if need + extra <= vram {
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
    let kv_camada = kv_mb(shape, ctx, cache) as f64 / camadas as f64;
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

    fn pedido(contexto: Option<u32>, cache: CacheKv) -> PedidoContexto {
        PedidoContexto { contexto, cache }
    }

    #[test]
    fn cache_de_8_bits_cabe_16k_na_rx580() {
        // RX 580 de 8 GB com ~7,5 GB livres e o Qwen3 8B: 16.384 só cabe com o cache de 8 bits
        let (s, l, kv, e) = QWEN3_8B;
        let m = shape(s, l, kv, e, 40960);
        let f16 = plan_com(&m, &placa(7600), &pedido(Some(16384), CacheKv::F16));
        assert_eq!(f16.encaixe, Encaixe::Placa);
        assert!(f16.contexto < 16384 && f16.contexto >= 8192, "{}", f16.contexto);
        assert!(f16.aviso.as_deref().unwrap().contains("Contexto reduzido"));
        let q8 = plan_com(&m, &placa(7600), &pedido(Some(16384), CacheKv::Q8_0));
        assert_eq!(q8.contexto, 16384, "{q8:?}");
        assert!(q8.aviso.is_none());
        // o cache de 8 bits ocupa pouco mais da metade
        assert_eq!(CacheKv::Q8_0.kv_mb(2304), 1224);
    }

    #[test]
    fn pedido_maior_que_cabe_fica_com_o_maior_possivel() {
        let (s, l, kv, e) = QWEN3_8B;
        let m = shape(s, l, kv, e, 40960);
        let p = plan_com(&m, &placa(7600), &pedido(Some(1_000_000), CacheKv::Q8_0));
        assert_eq!(p.encaixe, Encaixe::Placa);
        assert!(p.contexto > 16384 && p.contexto < 40960 && p.contexto % 256 == 0, "{}", p.contexto);
        assert_eq!(contexto_possivel(&m, &placa(7600), CacheKv::Q8_0), Some(p.contexto));
    }

    #[test]
    fn automatico_deixa_folga_e_nunca_fica_abaixo_do_que_cabe() {
        let (s, l, kv, e) = QWEN3_8B;
        let m = shape(s, l, kv, e, 40960);
        // RX 580: acima de 8.192 precisa deixar 1 GB livre; com 16 bits não sobra, fica 8.192
        assert_eq!(plan_com(&m, &placa(7600), &pedido(None, CacheKv::F16)).contexto, 8192);
        // com 8 bits passa de 8.192, mas menos que o pedido explícito (que usa a folga normal)
        let auto = plan_com(&m, &placa(7600), &pedido(None, CacheKv::Q8_0)).contexto;
        assert!(auto > 8192 && auto < 16384, "{auto}");
        // placa de 16 GB: vai até o teto de 32.768
        assert_eq!(plan_com(&m, &placa(15684), &pedido(None, CacheKv::F16)).contexto, 32768);
        // modelo com treino curto: respeita o máximo do modelo
        let curto = shape(400 * MB, 24, 12, 896, 4096);
        assert_eq!(plan_com(&curto, &placa(8000), &pedido(None, CacheKv::F16)).contexto, 4096);
        // placa pequena: abaixo de 8.192 o automático faz o mesmo de antes
        let (s4, l4, kv4, e4) = QWEN3_4B;
        let p = plan_com(&shape(s4, l4, kv4, e4, 40960), &placa(3800), &pedido(None, CacheKv::F16));
        assert!(p.contexto >= 4096 && p.contexto < 8192, "{}", p.contexto);
        // sem placa: o maior que cabe na RAM, com folga
        let r = Recursos { usa_placa: false, vram_livre_mb: None, ram_livre_mb: 30_000 };
        assert_eq!(plan_com(&m, &r, &pedido(None, CacheKv::F16)).contexto, 32768);
    }

    #[test]
    fn barra_mostra_quanto_usa_da_placa() {
        let (s, l, kv, e) = QWEN3_8B;
        let m = shape(s, l, kv, e, 40960);
        let (t16, c16) = memoria_estimada_mb(&m, 16384, CacheKv::F16);
        let (t8, c8) = memoria_estimada_mb(&m, 16384, CacheKv::Q8_0);
        assert_eq!(c16, 2304);
        assert!(c8 < c16 * 55 / 100 && t8 < t16);
        // RX 580 (~7,6 GB livres): 16.384 em 16 bits não cabe; em 8 bits cabe, mas apertado
        assert_eq!(situacao(t16, 7600, true), "nao-cabe");
        assert_eq!(situacao(t8, 7600, true), "apertado");
        assert_eq!(situacao(memoria_estimada_mb(&m, 4096, CacheKv::F16).0, 7600, true), "folga");
        assert_eq!(pontos_da_barra(40960), vec![2048, 4096, 8192, 12288, 16384, 24576, 32768, 40960]);
        assert_eq!(pontos_da_barra(8192), vec![2048, 4096, 8192]);
    }

    #[test]
    fn passos_do_maior_para_o_menor() {
        assert_eq!(passos_finos(4500, 2048), vec![4352, 4096, 3072, 2048]);
        assert_eq!(passos_finos(1000, 2048), Vec::<u32>::new());
        assert_eq!(CacheKv::de_texto("Q8_0"), Some(CacheKv::Q8_0));
        assert_eq!(CacheKv::de_texto("f16"), Some(CacheKv::F16));
        assert_eq!(CacheKv::de_texto("q4"), None);
    }

    #[test]
    fn placa_sem_memoria_livre_vai_para_processador() {
        let (s, l, kv, e) = QWEN3_8B;
        let r = Recursos { usa_placa: true, vram_livre_mb: Some(500), ram_livre_mb: 14_000 };
        let p = plan(&shape(s, l, kv, e, 40960), &r, 8192);
        assert_eq!(p.encaixe, Encaixe::Processador);
    }
}
