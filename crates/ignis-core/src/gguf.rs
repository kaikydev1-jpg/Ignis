//! Leitor mínimo de metadados GGUF (só o cabeçalho; não carrega pesos).

use std::collections::BTreeMap;
use std::io::{BufReader, Read};
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum GgufError {
    #[error("não é um arquivo GGUF")]
    NotGguf,
    #[error("versão GGUF {0} não suportada")]
    Version(u32),
    #[error("arquivo GGUF corrompido: {0}")]
    Corrupt(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    /// Arrays: guardamos o tamanho e, se forem números pequenos, o maior valor.
    Array {
        len: u64,
        max_int: Option<i64>,
    },
}

impl Value {
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Int(v) if *v >= 0 => Some(*v as u64),
            Value::Array { max_int: Some(v), .. } if *v >= 0 => Some(*v as u64),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

const MAX_STR: u64 = 64 * 1024 * 1024;

struct R<T: Read> {
    inner: T,
}

impl<T: Read> R<T> {
    fn u8(&mut self) -> Result<u8, GgufError> {
        let mut b = [0u8; 1];
        self.inner.read_exact(&mut b)?;
        Ok(b[0])
    }
    fn u16(&mut self) -> Result<u16, GgufError> {
        let mut b = [0u8; 2];
        self.inner.read_exact(&mut b)?;
        Ok(u16::from_le_bytes(b))
    }
    fn u32(&mut self) -> Result<u32, GgufError> {
        let mut b = [0u8; 4];
        self.inner.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn u64(&mut self) -> Result<u64, GgufError> {
        let mut b = [0u8; 8];
        self.inner.read_exact(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn skip(&mut self, n: u64) -> Result<(), GgufError> {
        let copied = std::io::copy(&mut (&mut self.inner).take(n), &mut std::io::sink())?;
        if copied != n {
            return Err(GgufError::Corrupt("fim inesperado"));
        }
        Ok(())
    }
    fn string(&mut self) -> Result<String, GgufError> {
        let n = self.u64()?;
        if n > MAX_STR {
            return Err(GgufError::Corrupt("texto grande demais"));
        }
        let mut buf = vec![0u8; n as usize];
        self.inner.read_exact(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
    fn skip_string(&mut self) -> Result<(), GgufError> {
        let n = self.u64()?;
        if n > MAX_STR {
            return Err(GgufError::Corrupt("texto grande demais"));
        }
        self.skip(n)
    }
    fn scalar_int(&mut self, t: u32) -> Result<Option<i64>, GgufError> {
        Ok(Some(match t {
            0 => self.u8()? as i64,
            1 => self.u8()? as i8 as i64,
            2 => self.u16()? as i64,
            3 => self.u16()? as i16 as i64,
            4 => self.u32()? as i64,
            5 => self.u32()? as i32 as i64,
            10 => self.u64()? as i64,
            11 => self.u64()? as i64,
            _ => return Ok(None),
        }))
    }
    fn value(&mut self, t: u32) -> Result<Value, GgufError> {
        if let Some(v) = self.scalar_int(t)? {
            return Ok(Value::Int(v));
        }
        Ok(match t {
            6 => Value::Float(f32::from_bits(self.u32()?) as f64),
            7 => Value::Bool(self.u8()? != 0),
            8 => Value::Str(self.string()?),
            12 => Value::Float(f64::from_bits(self.u64()?)),
            9 => {
                let at = self.u32()?;
                let len = self.u64()?;
                let mut max_int: Option<i64> = None;
                match at {
                    8 => {
                        for _ in 0..len {
                            self.skip_string()?;
                        }
                    }
                    0..=7 | 10..=12 => {
                        let size = match at {
                            0 | 1 | 7 => 1,
                            2 | 3 => 2,
                            4..=6 => 4,
                            _ => 8,
                        };
                        if len <= 4096 && !matches!(at, 6 | 7 | 12) {
                            for _ in 0..len {
                                let v = self.scalar_int(at)?.unwrap_or(0);
                                max_int = Some(max_int.map_or(v, |m| m.max(v)));
                            }
                        } else {
                            self.skip(len.checked_mul(size).ok_or(GgufError::Corrupt("array"))?)?;
                        }
                    }
                    _ => return Err(GgufError::Corrupt("array de tipo desconhecido")),
                }
                Value::Array { len, max_int }
            }
            _ => return Err(GgufError::Corrupt("tipo desconhecido")),
        })
    }
}

/// Lê todos os metadados (chave → valor) de um leitor GGUF.
pub fn read_metadata<T: Read>(reader: T) -> Result<BTreeMap<String, Value>, GgufError> {
    let mut r = R { inner: reader };
    let mut magic = [0u8; 4];
    r.inner.read_exact(&mut magic).map_err(|_| GgufError::NotGguf)?;
    if &magic != b"GGUF" {
        return Err(GgufError::NotGguf);
    }
    let version = r.u32()?;
    if !(2..=3).contains(&version) {
        return Err(GgufError::Version(version));
    }
    let _tensors = r.u64()?;
    let kv_count = r.u64()?;
    if kv_count > 1_000_000 {
        return Err(GgufError::Corrupt("metadados demais"));
    }
    let mut out = BTreeMap::new();
    for _ in 0..kv_count {
        let key = r.string()?;
        let t = r.u32()?;
        let v = r.value(t)?;
        out.insert(key, v);
    }
    Ok(out)
}

/// Confere só os 4 primeiros bytes.
pub fn is_gguf(path: &Path) -> bool {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).is_ok() && &magic == b"GGUF"
}

/// Resumo do que importa para o orçamento de memória e a interface.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub arquitetura: String,
    pub nome: Option<String>,
    pub licenca: Option<String>,
    pub camadas: u32,
    pub contexto_treino: u32,
    pub embd: u32,
    pub cabecas: u32,
    pub cabecas_kv: u32,
    pub dim_k: u32,
    pub dim_v: u32,
    pub embeddings: bool,
    pub tem_template_chat: bool,
}

impl ModelInfo {
    pub fn from_metadata(m: &BTreeMap<String, Value>) -> Self {
        let arch = m.get("general.architecture").and_then(|v| v.as_str()).unwrap_or("desconhecida").to_string();
        let get = |k: &str| m.get(&format!("{arch}.{k}")).and_then(|v| v.as_u64());
        let camadas = get("block_count").unwrap_or(0) as u32;
        let embd = get("embedding_length").unwrap_or(0) as u32;
        let cabecas = get("attention.head_count").unwrap_or(0) as u32;
        let cabecas_kv = get("attention.head_count_kv").unwrap_or(cabecas as u64) as u32;
        let head_dim = embd.checked_div(cabecas).unwrap_or(0);
        let dim_k = get("attention.key_length").unwrap_or(head_dim as u64) as u32;
        let dim_v = get("attention.value_length").unwrap_or(head_dim as u64) as u32;
        let nome = m.get("general.name").and_then(|v| v.as_str()).map(String::from);
        let pooling = get("pooling_type").unwrap_or(0);
        let encoder_only = matches!(
            arch.as_str(),
            "bert" | "nomic-bert" | "nomic-bert-moe" | "jina-bert-v2" | "jina-bert-v3" | "t5encoder" | "modern-bert"
        );
        let nome_embed = nome.as_deref().map(|n| n.to_lowercase().contains("embed")).unwrap_or(false);
        Self {
            embeddings: encoder_only || pooling > 0 || nome_embed,
            tem_template_chat: m.contains_key("tokenizer.chat_template"),
            licenca: m.get("general.license").and_then(|v| v.as_str()).map(String::from),
            contexto_treino: get("context_length").unwrap_or(0) as u32,
            arquitetura: arch,
            nome,
            camadas,
            embd,
            cabecas,
            cabecas_kv,
            dim_k,
            dim_v,
        }
    }

    pub fn read(path: &Path) -> Result<Self, GgufError> {
        let f = std::fs::File::open(path)?;
        let meta = read_metadata(BufReader::with_capacity(1 << 20, f))?;
        Ok(Self::from_metadata(&meta))
    }

    /// Bytes do cache KV por token de contexto (f16), somando K e V de todas as camadas.
    pub fn kv_bytes_por_token(&self) -> u64 {
        kv_bytes_por_token(self.camadas, self.cabecas_kv, self.dim_k, self.dim_v)
    }
}

pub fn kv_bytes_por_token(camadas: u32, cabecas_kv: u32, dim_k: u32, dim_v: u32) -> u64 {
    camadas as u64 * cabecas_kv as u64 * (dim_k as u64 + dim_v as u64) * 2
}

/// Ferramenta de teste: escreve um GGUF mínimo (sem tensores) com os metadados dados.
pub fn write_test_gguf(path: &Path, kv: &[(&str, Value)]) -> std::io::Result<()> {
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"GGUF");
    b.extend_from_slice(&3u32.to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes());
    b.extend_from_slice(&(kv.len() as u64).to_le_bytes());
    for (k, v) in kv {
        b.extend_from_slice(&(k.len() as u64).to_le_bytes());
        b.extend_from_slice(k.as_bytes());
        match v {
            Value::Int(i) => {
                b.extend_from_slice(&4u32.to_le_bytes());
                b.extend_from_slice(&(*i as u32).to_le_bytes());
            }
            Value::Float(f) => {
                b.extend_from_slice(&6u32.to_le_bytes());
                b.extend_from_slice(&(*f as f32).to_bits().to_le_bytes());
            }
            Value::Bool(x) => {
                b.extend_from_slice(&7u32.to_le_bytes());
                b.push(*x as u8);
            }
            Value::Str(s) => {
                b.extend_from_slice(&8u32.to_le_bytes());
                b.extend_from_slice(&(s.len() as u64).to_le_bytes());
                b.extend_from_slice(s.as_bytes());
            }
            Value::Array { len, .. } => {
                // array de strings "t"
                b.extend_from_slice(&9u32.to_le_bytes());
                b.extend_from_slice(&8u32.to_le_bytes());
                b.extend_from_slice(&len.to_le_bytes());
                for _ in 0..*len {
                    b.extend_from_slice(&1u64.to_le_bytes());
                    b.push(b't');
                }
            }
        }
    }
    // um pouco de "peso" falso para o arquivo ter tamanho
    b.extend_from_slice(&[0u8; 64]);
    std::fs::write(path, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qwen3_meta() -> Vec<(&'static str, Value)> {
        vec![
            ("general.architecture", Value::Str("qwen3".into())),
            ("general.name", Value::Str("Qwen3 4B".into())),
            ("qwen3.block_count", Value::Int(36)),
            ("qwen3.context_length", Value::Int(40960)),
            ("qwen3.embedding_length", Value::Int(2560)),
            ("qwen3.attention.head_count", Value::Int(32)),
            ("qwen3.attention.head_count_kv", Value::Int(8)),
            ("qwen3.attention.key_length", Value::Int(128)),
            ("qwen3.attention.value_length", Value::Int(128)),
            ("tokenizer.ggml.tokens", Value::Array { len: 5, max_int: None }),
            ("tokenizer.chat_template", Value::Str("{{ messages }}".into())),
        ]
    }

    #[test]
    fn le_metadados_de_arquivo_sintetico() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("m.gguf");
        write_test_gguf(&p, &qwen3_meta()).unwrap();
        assert!(is_gguf(&p));
        let info = ModelInfo::read(&p).unwrap();
        assert_eq!(info.arquitetura, "qwen3");
        assert_eq!(info.camadas, 36);
        assert_eq!(info.cabecas_kv, 8);
        assert_eq!(info.contexto_treino, 40960);
        assert!(info.tem_template_chat);
        assert!(!info.embeddings);
        // 36 camadas * 8 cabeças * (128+128) * 2 bytes = 147456
        assert_eq!(info.kv_bytes_por_token(), 147_456);
    }

    #[test]
    fn dim_da_cabeca_sai_do_embd_quando_falta() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("m.gguf");
        write_test_gguf(
            &p,
            &[
                ("general.architecture", Value::Str("qwen2".into())),
                ("qwen2.block_count", Value::Int(24)),
                ("qwen2.embedding_length", Value::Int(896)),
                ("qwen2.attention.head_count", Value::Int(14)),
                ("qwen2.attention.head_count_kv", Value::Int(2)),
            ],
        )
        .unwrap();
        let info = ModelInfo::read(&p).unwrap();
        assert_eq!(info.dim_k, 64);
        assert_eq!(info.kv_bytes_por_token(), 24 * 2 * 128 * 2);
    }

    #[test]
    fn detecta_modelo_de_embeddings() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("e.gguf");
        write_test_gguf(
            &p,
            &[
                ("general.architecture", Value::Str("qwen3".into())),
                ("general.name", Value::Str("Qwen3 Embedding 0.6b".into())),
                ("qwen3.pooling_type", Value::Int(3)),
            ],
        )
        .unwrap();
        assert!(ModelInfo::read(&p).unwrap().embeddings);
    }

    #[test]
    fn rejeita_arquivo_que_nao_e_gguf() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.bin");
        std::fs::write(&p, b"NOPE1234").unwrap();
        assert!(!is_gguf(&p));
        assert!(matches!(ModelInfo::read(&p), Err(GgufError::NotGguf)));
    }

    #[test]
    fn modelo_real_se_disponivel() {
        // Roda só se IGNIS_TEST_MODEL apontar para um GGUF de verdade.
        let Some(p) = std::env::var_os("IGNIS_TEST_MODEL") else { return };
        let info = ModelInfo::read(Path::new(&p)).unwrap();
        assert!(info.camadas > 0);
        assert!(info.kv_bytes_por_token() > 0);
    }
}
