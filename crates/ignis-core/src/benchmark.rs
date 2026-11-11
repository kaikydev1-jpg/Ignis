//! Teste rápido de velocidade de cada motor candidato (até 30 s cada).
//! Mede tokens por segundo na leitura (prompt) e na geração.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::engine::Backend;
use crate::supervisor::{self, LaunchSpec};

/// Tempo máximo da medição de cada motor.
pub const LIMITE_POR_CANDIDATO: Duration = Duration::from_secs(30);
/// Tempo para o motor subir com o modelo de teste (na primeira vez o driver
/// pode compilar sombreadores — ex.: Vulkan/Mesa — e isso não conta na medição).
pub const LIMITE_PARA_SUBIR: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchResult {
    pub variante: String,
    pub backend: Backend,
    pub nome: String,
    pub ok: bool,
    /// tokens/s lendo o pedido
    pub leitura_tps: Option<f64>,
    /// tokens/s escrevendo a resposta
    pub geracao_tps: Option<f64>,
    pub erro: Option<String>,
    pub duracao_ms: u64,
}

const PROMPT: &str = "O Ignis é um aplicativo que roda modelos de inteligência artificial no próprio computador, \
usando a placa de vídeo quando possível. Ele escolhe sozinho o motor mais rápido para cada placa, baixa modelos \
do Hugging Face e conversa com outros programas por uma porta local compatível com a API da OpenAI. ";

fn prompt_text() -> String {
    // ~400 tokens
    PROMPT.repeat(6)
}

#[derive(Deserialize)]
struct Timings {
    prompt_per_second: Option<f64>,
    predicted_per_second: Option<f64>,
}

#[derive(Deserialize)]
struct CompletionResp {
    timings: Option<Timings>,
}

async fn measure(client: &reqwest::Client, base: &str) -> Result<(f64, f64), String> {
    // aquecimento curto (compila sombreadores, enche caches)
    let _ = client
        .post(format!("{base}/completion"))
        .json(&serde_json::json!({"prompt": "Olá", "n_predict": 4, "cache_prompt": false}))
        .send()
        .await;
    let resp = client
        .post(format!("{base}/completion"))
        .json(&serde_json::json!({
            "prompt": prompt_text(),
            "n_predict": 48,
            "ignore_eos": true,
            "cache_prompt": false,
            "temperature": 0.0
        }))
        .send()
        .await
        .map_err(|e| format!("o motor não respondeu ({e})"))?;
    if !resp.status().is_success() {
        return Err(format!("o motor respondeu erro {}", resp.status().as_u16()));
    }
    let body: CompletionResp = resp.json().await.map_err(|e| format!("resposta estranha do motor ({e})"))?;
    let t = body.timings.ok_or("o motor não informou a velocidade")?;
    match (t.prompt_per_second, t.predicted_per_second) {
        (Some(p), Some(g)) if p > 0.0 && g > 0.0 => Ok((p, g)),
        _ => Err("velocidade zerada".into()),
    }
}

/// Testa um motor: sobe com o modelo pequeno, mede e encerra.
pub async fn bench_one(exe: &Path, variante: &str, backend: Backend, modelo: &Path, client: &reqwest::Client) -> BenchResult {
    let started = Instant::now();
    let spec = LaunchSpec {
        exe: exe.to_path_buf(),
        modelo: modelo.to_path_buf(),
        alias: "teste".into(),
        contexto: 2048,
        ngl: "999".into(),
        embeddings: false,
        backend,
        variante: variante.into(),
        args_extras: vec![],
        mmproj: None,
    };
    let mut result = BenchResult {
        variante: variante.into(),
        backend,
        nome: backend.nome().into(),
        ok: false,
        leitura_tps: None,
        geracao_tps: None,
        erro: None,
        duracao_ms: 0,
    };
    match supervisor::start(&spec, client, LIMITE_PARA_SUBIR).await {
        Err(e) => result.erro = Some(e.to_string()),
        Ok(mut server) => {
            match tokio::time::timeout(LIMITE_POR_CANDIDATO, measure(client, &server.base_url)).await {
                Ok(Ok((p, g))) => {
                    result.ok = true;
                    result.leitura_tps = Some((p * 10.0).round() / 10.0);
                    result.geracao_tps = Some((g * 10.0).round() / 10.0);
                }
                Ok(Err(e)) => result.erro = Some(e),
                Err(_) => result.erro = Some("a medição passou de 30 segundos".into()),
            }
            server.stop().await;
        }
    }
    result.duracao_ms = started.elapsed().as_millis() as u64;
    tracing::info!("teste {variante}: {:?}", result);
    result
}

/// Ordena do mais rápido para o mais lento (só os que funcionaram).
/// Critério: velocidade de geração; empate (até 5%) desempata pela leitura.
pub fn rank(results: &[BenchResult]) -> Vec<String> {
    let mut ok: Vec<&BenchResult> = results.iter().filter(|r| r.ok).collect();
    ok.sort_by(|a, b| {
        let (ga, gb) = (a.geracao_tps.unwrap_or(0.0), b.geracao_tps.unwrap_or(0.0));
        let max = ga.max(gb).max(0.001);
        if (ga - gb).abs() / max <= 0.05 {
            b.leitura_tps.unwrap_or(0.0).partial_cmp(&a.leitura_tps.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            gb.partial_cmp(&ga).unwrap_or(std::cmp::Ordering::Equal)
        }
    });
    ok.into_iter().map(|r| r.variante.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(v: &str, ok: bool, p: f64, g: f64) -> BenchResult {
        BenchResult {
            variante: v.into(),
            backend: Backend::Vulkan,
            nome: String::new(),
            ok,
            leitura_tps: Some(p),
            geracao_tps: Some(g),
            erro: None,
            duracao_ms: 0,
        }
    }

    #[test]
    fn escolhe_o_mais_rapido_que_funcionou() {
        let res = vec![r("rocm", false, 0.0, 0.0), r("vulkan", true, 400.0, 40.0), r("cpu", true, 50.0, 8.0)];
        assert_eq!(rank(&res), vec!["vulkan", "cpu"]);
    }

    #[test]
    fn empate_na_geracao_desempata_pela_leitura() {
        let res = vec![r("vulkan", true, 300.0, 50.0), r("rocm", true, 900.0, 49.0)];
        assert_eq!(rank(&res), vec!["rocm", "vulkan"]);
        let res = vec![r("vulkan", true, 300.0, 60.0), r("rocm", true, 900.0, 49.0)];
        assert_eq!(rank(&res), vec!["vulkan", "rocm"]);
    }
}
