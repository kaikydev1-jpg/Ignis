//! Motores auxiliares: voz (omnivoice.cpp, `tts-server`), voz comercial (Chatterbox
//! Multilingual pelo codec.cpp, `tts-server`), transcrição (whisper.cpp, `whisper-server`)
//! e geração de imagem (stable-diffusion.cpp, `sd-server`).
//!
//! Todos são da mesma família do llama.cpp (GGML): rodam na placa por Vulkan
//! (AMD, Intel e NVIDIA) ou no processador. O Ignis baixa só a versão que serve
//! para a placa, sobe o servidor quando alguém pede e repassa as rotas no
//! formato da OpenAI (`/v1/audio/speech`, `/v1/audio/voices`,
//! `/v1/audio/transcriptions`, `/v1/audio/translations`, `/v1/images/generations`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::download::{self, DownloadError, Progress};
use crate::engine::install::InstallError;
use crate::engine::manifest::AssetFile;
use crate::engine::Backend;

const EMBUTIDO: &str = include_str!("../../../manifest/motores_audio.json");
const MARCADOR: &str = ".ignis-audio.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Familia {
    Voz,
    Transcricao,
    /// Chatterbox Multilingual (MIT): voz que pode ir para produto pago.
    VozComercial,
    /// stable-diffusion.cpp com Z-Image Turbo (Apache 2.0).
    Imagem,
}

impl Familia {
    pub const TODAS: [Familia; 4] = [Familia::Voz, Familia::Transcricao, Familia::VozComercial, Familia::Imagem];
    /// O que "Preparar" baixa quando ninguém diz o quê (como antes: voz e transcrição).
    pub const PADRAO: [Familia; 2] = [Familia::Voz, Familia::Transcricao];

    pub fn indice(self) -> usize {
        match self {
            Familia::Voz => 0,
            Familia::Transcricao => 1,
            Familia::VozComercial => 2,
            Familia::Imagem => 3,
        }
    }

    pub fn nome(self) -> &'static str {
        match self {
            Familia::Voz => "voz",
            Familia::Transcricao => "transcrição",
            Familia::VozComercial => "voz comercial",
            Familia::Imagem => "geração de imagem",
        }
    }

    pub fn chave(self) -> &'static str {
        match self {
            Familia::Voz => "voz",
            Familia::Transcricao => "transcricao",
            Familia::VozComercial => "voz-comercial",
            Familia::Imagem => "imagem",
        }
    }

    pub fn de_texto(s: &str) -> Option<Self> {
        match s {
            "voz" => Some(Familia::Voz),
            "transcricao" | "transcrição" => Some(Familia::Transcricao),
            "voz-comercial" | "voz_comercial" | "chatterbox" => Some(Familia::VozComercial),
            "imagem" | "imagens" => Some(Familia::Imagem),
            _ => None,
        }
    }

    /// Rota que diz que o servidor está pronto (o sd-server não tem /health).
    pub fn rota_saude(self) -> &'static str {
        match self {
            Familia::Imagem => "/v1/models",
            _ => "/health",
        }
    }
}

/// Um arquivo de modelo. Por padrão vem do repositório do modelo; `repo`, `revisao` e
/// `caminho` servem para modelos montados com arquivos de repositórios diferentes
/// (a imagem usa o modelo de difusão, o codificador de texto e o VAE).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArquivoModelo {
    pub nome: String,
    pub sha256: String,
    pub tamanho: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revisao: Option<String>,
    /// Caminho dentro do repositório, se não for o próprio nome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caminho: Option<String>,
}

/// Um pacote de motor (uma família, um sistema, um backend).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotorAudio {
    pub id: String,
    pub familia: Familia,
    pub so: String,
    pub backend: Backend,
    pub executavel: String,
    pub arquivo: AssetFile,
}

/// Um modelo (uma ou mais arquivos do Hugging Face, revisão fixada).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModeloAudio {
    pub id: String,
    pub familia: Familia,
    pub nome: String,
    /// "placa-grande", "placa", "placa-pequena", "processador".
    pub perfis: Vec<String>,
    pub repo: String,
    pub revisao: String,
    pub arquivos: Vec<ArquivoModelo>,
    pub licenca: String,
    pub licenca_url: String,
    pub uso_comercial: bool,
    pub descricao: String,
}

impl ModeloAudio {
    pub fn tamanho(&self) -> u64 {
        self.arquivos.iter().map(|a| a.tamanho).sum()
    }

    pub fn url(&self, a: &ArquivoModelo) -> String {
        format!(
            "{}/{}/resolve/{}/{}",
            crate::hub::base_url(),
            a.repo.as_deref().unwrap_or(&self.repo),
            a.revisao.as_deref().unwrap_or(&self.revisao),
            a.caminho.as_deref().unwrap_or(&a.nome)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestoAudio {
    pub versao_manifesto: u32,
    /// Release dos binários (ex.: "motores-audio-1").
    pub versao: String,
    pub url_base: String,
    pub motores: Vec<MotorAudio>,
    pub modelos: Vec<ModeloAudio>,
}

impl ManifestoAudio {
    pub fn embutido() -> Self {
        serde_json::from_str(EMBUTIDO).expect("manifest/motores_audio.json válido")
    }

    pub fn url(&self, a: &AssetFile) -> String {
        format!("{}{}", self.url_base, a.nome)
    }

    pub fn validar(&self) -> Result<(), String> {
        // 64 dígitos hexadecimais e não só zeros (zeros = soma ainda não preenchida)
        let hex = |s: &str| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) && s.chars().any(|c| c != '0');
        for m in &self.motores {
            if !hex(&m.arquivo.sha256) || m.arquivo.tamanho == 0 {
                return Err(format!("motor {} sem SHA-256 ou tamanho", m.id));
            }
        }
        for m in &self.modelos {
            if m.arquivos.is_empty() || m.arquivos.iter().any(|a| !hex(&a.sha256) || a.tamanho == 0) {
                return Err(format!("modelo {} sem SHA-256 ou tamanho", m.id));
            }
            if m.revisao.len() != 40 || m.arquivos.iter().any(|a| a.revisao.as_ref().is_some_and(|r| r.len() != 40)) {
                return Err(format!("modelo {} sem revisão fixada", m.id));
            }
            if m.arquivos.iter().any(|a| a.nome.contains(['/', '\\']) || a.nome.contains("..")) {
                return Err(format!("modelo {} com nome de arquivo inválido", m.id));
            }
        }
        for f in Familia::TODAS {
            for so in ["windows", "linux"] {
                if !self.motores.iter().any(|m| m.familia == f && m.so == so && m.backend == Backend::Cpu) {
                    return Err(format!("falta o motor de {} para processador no {so}", f.nome()));
                }
            }
            if !self.modelos.iter().any(|m| m.familia == f && m.perfis.iter().any(|p| p == "processador")) {
                return Err(format!("falta um modelo de {} para processador", f.nome()));
            }
        }
        Ok(())
    }

    pub fn modelo(&self, id: &str) -> Option<&ModeloAudio> {
        self.modelos.iter().find(|m| m.id == id)
    }
}

/// Qual perfil de modelo cabe nesta máquina.
pub fn perfil(fam: Familia, usa_placa: bool, vram_mb: Option<u64>) -> &'static str {
    let vram = vram_mb.unwrap_or(0);
    match fam {
        Familia::Voz if !usa_placa => "processador",
        Familia::Voz if vram >= 12_000 => "placa-grande",
        Familia::Voz if vram >= 4_000 => "placa",
        Familia::Voz => "placa-pequena",
        Familia::Transcricao if usa_placa && vram >= 2_000 => "placa",
        Familia::Transcricao => "processador",
        Familia::VozComercial if !usa_placa => "processador",
        Familia::VozComercial if vram >= 3_000 => "placa",
        Familia::VozComercial => "placa-pequena",
        Familia::Imagem if !usa_placa => "processador",
        Familia::Imagem if vram >= 7_000 => "placa",
        Familia::Imagem => "placa-pequena",
    }
}

/// O modelo recomendado para esta máquina.
pub fn escolher_modelo(m: &ManifestoAudio, fam: Familia, usa_placa: bool, vram_mb: Option<u64>) -> Option<&ModeloAudio> {
    let p = perfil(fam, usa_placa, vram_mb);
    m.modelos.iter().find(|x| x.familia == fam && x.perfis.iter().any(|y| y == p)).or_else(|| m.modelos.iter().find(|x| x.familia == fam))
}

/// Motores em ordem de tentativa: a placa (Vulkan) primeiro, se houver placa
/// e Vulkan; o processador sempre por último.
pub fn candidatos<'a>(m: &'a ManifestoAudio, fam: Familia, so: &str, placa_com_vulkan: bool) -> Vec<&'a MotorAudio> {
    let mut v: Vec<&MotorAudio> = m
        .motores
        .iter()
        .filter(|x| x.familia == fam && x.so == so && (x.backend == Backend::Cpu || (placa_com_vulkan && x.backend == Backend::Vulkan)))
        .collect();
    v.sort_by_key(|x| if x.backend == Backend::Cpu { 1 } else { 0 });
    v
}

/// Uma voz clonada guardada pelo Ignis (o servidor de voz guarda só na memória;
/// o Ignis registra de novo a cada vez que ele sobe).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VozSalva {
    pub name: String,
    #[serde(default)]
    pub ref_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wav_b64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rvq_b64: Option<String>,
}

pub fn nome_de_voz_valido(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ')) && !n.starts_with('.')
}

/// Argumentos do servidor de voz.
pub fn args_voz(base: &Path, tokenizer: &Path) -> Vec<String> {
    vec![
        "--model".into(),
        base.to_string_lossy().into_owned(),
        "--codec".into(),
        tokenizer.to_string_lossy().into_owned(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        "{porta}".into(),
    ]
}

/// Argumentos do servidor de voz comercial (codec.cpp: modelo principal + codec).
/// Na versão Vulkan, `--gpu` liga a placa.
pub fn args_voz_comercial(backbone: &Path, codec: &Path, gpu: bool) -> Vec<String> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8);
    let mut a = vec![
        "--backbone".into(),
        backbone.to_string_lossy().into_owned(),
        "--model".into(),
        codec.to_string_lossy().into_owned(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        "{porta}".into(),
        "--n-threads".into(),
        threads.to_string(),
        "--model-id".into(),
        "chatterbox".into(),
    ];
    // amostragem de referência do Chatterbox Multilingual (docs/models/chatterbox.md do codec.cpp)
    for (f, v) in [
        ("--temp", "0.8"),
        ("--top-k", "0"),
        ("--top-p", "1.0"),
        ("--min-p", "0.05"),
        ("--repeat-penalty", "1.2"),
        ("--repeat-last-n", "1024"),
        ("--samplers", "penalties;temperature;min_p;top_p"),
    ] {
        a.push(f.into());
        a.push(v.into());
    }
    if gpu {
        a.push("--gpu".into());
    }
    a
}

/// Argumentos do servidor de imagem (sd-server com Z-Image Turbo: modelo de difusão,
/// codificador de texto e VAE). Turbo: 8 passos, sem CFG. Os pesos ficam na RAM e vão
/// para a placa quando precisa (`--offload-to-cpu`), e o VAE em blocos: cabe em 8 GB.
/// Tamanho padrão (quando o pedido não diz): 1024x1024 na placa, 512x512 no processador.
pub fn args_imagem(difusao: &Path, texto: &Path, vae: &Path, gpu: bool) -> Vec<String> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 16);
    let lado = if gpu { "1024" } else { "512" };
    vec![
        "--diffusion-model".into(),
        difusao.to_string_lossy().into_owned(),
        "--llm".into(),
        texto.to_string_lossy().into_owned(),
        "--vae".into(),
        vae.to_string_lossy().into_owned(),
        "--listen-ip".into(),
        "127.0.0.1".into(),
        "--listen-port".into(),
        "{porta}".into(),
        "--offload-to-cpu".into(),
        "--vae-tiling".into(),
        "--cfg-scale".into(),
        "1.0".into(),
        "--steps".into(),
        "8".into(),
        "-W".into(),
        lado.into(),
        "-H".into(),
        lado.into(),
        "-t".into(),
        threads.to_string(),
    ]
}

/// Texto do jeito que o Chatterbox Multilingual espera (como o tokenizador original):
/// minúsculas, Unicode NFKD e a marca do idioma na frente (`[pt]`).
pub fn texto_chatterbox(texto: &str, idioma: Option<&str>) -> String {
    use unicode_normalization::UnicodeNormalization;
    let lang = codigo_idioma(idioma.unwrap_or("pt"));
    let limpo: String = texto.to_lowercase().nfkd().collect();
    format!("[{lang}]{limpo}")
}

/// "Portuguese", "português", "pt-BR" → "pt" (os 23 idiomas do Chatterbox Multilingual).
pub fn codigo_idioma(s: &str) -> &'static str {
    let s = s.trim().to_lowercase();
    let s = s.split(['-', '_']).next().unwrap_or("");
    match s {
        "ar" | "arabic" | "árabe" | "arabe" => "ar",
        "da" | "danish" | "dinamarquês" => "da",
        "de" | "german" | "alemão" | "alemao" => "de",
        "el" | "greek" | "grego" => "el",
        "en" | "english" | "inglês" | "ingles" => "en",
        "es" | "spanish" | "espanhol" => "es",
        "fi" | "finnish" | "finlandês" => "fi",
        "fr" | "french" | "francês" | "frances" => "fr",
        "he" | "hebrew" | "hebraico" => "he",
        "hi" | "hindi" => "hi",
        "it" | "italian" | "italiano" => "it",
        "ja" | "japanese" | "japonês" | "japones" => "ja",
        "ko" | "korean" | "coreano" => "ko",
        "ms" | "malay" | "malaio" => "ms",
        "nl" | "dutch" | "holandês" => "nl",
        "no" | "nb" | "norwegian" | "norueguês" => "no",
        "pl" | "polish" | "polonês" => "pl",
        "ru" | "russian" | "russo" => "ru",
        "sv" | "swedish" | "sueco" => "sv",
        "sw" | "swahili" | "suaíli" => "sw",
        "tr" | "turkish" | "turco" => "tr",
        "zh" | "chinese" | "chinês" | "chines" => "zh",
        _ => "pt",
    }
}

/// Argumentos do servidor de transcrição (rota no formato da OpenAI).
pub fn args_transcricao(modelo: &Path) -> Vec<String> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8);
    vec![
        "-m".into(),
        modelo.to_string_lossy().into_owned(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        "{porta}".into(),
        "--inference-path".into(),
        ROTA_TRANSCRICAO.into(),
        "-t".into(),
        threads.to_string(),
    ]
}

pub const ROTA_TRANSCRICAO: &str = "/v1/audio/transcriptions";

/// Acrescenta um campo de texto a um corpo `multipart/form-data` já pronto
/// (ex.: `translate=true` para `/v1/audio/translations`).
pub fn multipart_com_campo(corpo: &[u8], content_type: &str, nome: &str, valor: &str) -> Option<Vec<u8>> {
    let boundary = content_type.split(';').map(str::trim).find_map(|p| p.strip_prefix("boundary="))?.trim_matches('"');
    let fim = format!("--{boundary}--");
    let pos = corpo.windows(fim.len()).rposition(|w| w == fim.as_bytes())?;
    let mut out = Vec::with_capacity(corpo.len() + 128);
    out.extend_from_slice(&corpo[..pos]);
    out.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{nome}\"\r\n\r\n{valor}\r\n").as_bytes());
    out.extend_from_slice(&corpo[pos..]);
    Some(out)
}

#[derive(Debug, Serialize, Deserialize)]
struct Marcador {
    sha256: Vec<String>,
    executavel: Option<String>,
}

/// Onde ficam os motores, os modelos e as vozes de áudio.
pub struct AudioStore {
    raiz: PathBuf,
    downloads: PathBuf,
    extras: Option<PathBuf>,
}

impl AudioStore {
    pub fn new(raiz: PathBuf, downloads: PathBuf, extras: Option<PathBuf>) -> Self {
        Self { raiz, downloads, extras }
    }

    fn motor_dir(&self, man: &ManifestoAudio, mo: &MotorAudio) -> PathBuf {
        self.raiz.join("motores").join(&man.versao).join(&mo.id)
    }

    /// Executável instalado (e conferido) deste motor.
    pub fn motor_instalado(&self, man: &ManifestoAudio, mo: &MotorAudio) -> Option<PathBuf> {
        let dir = self.motor_dir(man, mo);
        let m: Marcador = crate::paths::read_json(&dir.join(MARCADOR))?;
        let exe = dir.join(&mo.executavel);
        (m.sha256.first().is_some_and(|s| s.eq_ignore_ascii_case(&mo.arquivo.sha256)) && exe.is_file()).then_some(exe)
    }

    pub async fn instalar_motor(
        &self,
        client: &reqwest::Client,
        man: &ManifestoAudio,
        mo: &MotorAudio,
        cancel: &CancellationToken,
        progress: &(dyn Fn(Progress) + Send + Sync),
    ) -> Result<PathBuf, InstallError> {
        if let Some(exe) = self.motor_instalado(man, mo) {
            return Ok(exe);
        }
        let dl = self.downloads.join("audio");
        std::fs::create_dir_all(&dl)?;
        let arq = dl.join(&mo.arquivo.nome);
        download::download_file(
            client,
            &man.url(&mo.arquivo),
            &arq,
            Some(&mo.arquivo.sha256),
            Some(mo.arquivo.tamanho),
            cancel,
            progress,
            None,
        )
        .await?;
        let dir = self.motor_dir(man, mo);
        let staging = dir.with_file_name(format!(".{}.instalando", mo.id));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let (a2, s2) = (arq.clone(), staging.clone());
        tokio::task::spawn_blocking(move || crate::engine::install::extract(&a2, &s2))
            .await
            .map_err(|e| InstallError::Io(std::io::Error::other(e)))??;
        let exe = staging.join(&mo.executavel);
        if !exe.is_file() {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(InstallError::NoServer);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&exe)?.permissions();
            perm.set_mode(perm.mode() | 0o755);
            std::fs::set_permissions(&exe, perm)?;
        }
        self.copiar_extras(&staging);
        crate::paths::write_json(
            &staging.join(MARCADOR),
            &Marcador { sha256: vec![mo.arquivo.sha256.to_lowercase()], executavel: Some(mo.executavel.clone()) },
        )?;
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.parent().unwrap())?;
        std::fs::rename(&staging, &dir)?;
        let _ = std::fs::remove_file(&arq);
        self.motor_instalado(man, mo).ok_or(InstallError::NoServer)
    }

    /// Instala a partir de um executável local (testes).
    pub fn instalar_motor_de_arquivo(
        &self,
        man: &ManifestoAudio,
        mo: &MotorAudio,
        exe: &Path,
        extras: &[(&str, &[u8])],
    ) -> std::io::Result<PathBuf> {
        let dir = self.motor_dir(man, mo);
        std::fs::create_dir_all(&dir)?;
        std::fs::copy(exe, dir.join(&mo.executavel))?;
        for (n, b) in extras {
            std::fs::write(dir.join(n), b)?;
        }
        crate::paths::write_json(
            &dir.join(MARCADOR),
            &Marcador { sha256: vec![mo.arquivo.sha256.to_lowercase()], executavel: Some(mo.executavel.clone()) },
        )?;
        self.motor_instalado(man, mo).ok_or_else(|| std::io::Error::other("instalação do motor de áudio falhou"))
    }

    /// Copia as DLLs do Visual C++ (Windows) para a pasta do motor.
    fn copiar_extras(&self, dir: &Path) {
        let Some(src) = &self.extras else { return };
        let Ok(rd) = std::fs::read_dir(src) else { return };
        for e in rd.flatten() {
            let dest = dir.join(e.file_name());
            if e.path().is_file() && !dest.exists() {
                let _ = std::fs::copy(e.path(), &dest);
            }
        }
    }

    fn modelo_dir(&self, mo: &ModeloAudio) -> PathBuf {
        self.raiz.join("modelos").join(&mo.id)
    }

    /// Arquivos do modelo, se todos estiverem baixados e conferidos.
    pub fn modelo_instalado(&self, mo: &ModeloAudio) -> Option<Vec<PathBuf>> {
        let dir = self.modelo_dir(mo);
        let m: Marcador = crate::paths::read_json(&dir.join(MARCADOR))?;
        let esperados: Vec<String> = mo.arquivos.iter().map(|a| a.sha256.to_lowercase()).collect();
        if m.sha256 != esperados {
            return None;
        }
        let arquivos: Vec<PathBuf> = mo.arquivos.iter().map(|a| dir.join(&a.nome)).collect();
        arquivos
            .iter()
            .zip(&mo.arquivos)
            .all(|(p, a)| std::fs::metadata(p).map(|md| md.len() == a.tamanho).unwrap_or(false))
            .then_some(arquivos)
    }

    pub async fn instalar_modelo(
        &self,
        client: &reqwest::Client,
        mo: &ModeloAudio,
        cancel: &CancellationToken,
        progress: &(dyn Fn(Progress) + Send + Sync),
        hf_token: Option<&str>,
    ) -> Result<Vec<PathBuf>, DownloadError> {
        if let Some(a) = self.modelo_instalado(mo) {
            return Ok(a);
        }
        let dir = self.modelo_dir(mo);
        std::fs::create_dir_all(&dir)?;
        let total = mo.tamanho();
        let mut antes = 0u64;
        for a in &mo.arquivos {
            let base = antes;
            let cb = move |p: Progress| progress(Progress { baixado: base + p.baixado, total: Some(total) });
            download::download_file(client, &mo.url(a), &dir.join(&a.nome), Some(&a.sha256), Some(a.tamanho), cancel, &cb, hf_token)
                .await?;
            antes += a.tamanho;
        }
        crate::paths::write_json(
            &dir.join(MARCADOR),
            &Marcador { sha256: mo.arquivos.iter().map(|a| a.sha256.to_lowercase()).collect(), executavel: None },
        )?;
        self.modelo_instalado(mo).ok_or_else(|| DownloadError::Io(std::io::Error::other("modelo de áudio incompleto")))
    }

    /// Marca um modelo como instalado a partir de arquivos já na pasta (testes).
    pub fn instalar_modelo_de_arquivos(&self, mo: &ModeloAudio, conteudos: &[&[u8]]) -> std::io::Result<Vec<PathBuf>> {
        let dir = self.modelo_dir(mo);
        std::fs::create_dir_all(&dir)?;
        for (a, c) in mo.arquivos.iter().zip(conteudos) {
            std::fs::write(dir.join(&a.nome), c)?;
        }
        crate::paths::write_json(
            &dir.join(MARCADOR),
            &Marcador { sha256: mo.arquivos.iter().map(|a| a.sha256.to_lowercase()).collect(), executavel: None },
        )?;
        Ok(mo.arquivos.iter().map(|a| dir.join(&a.nome)).collect())
    }

    fn vozes_dir(&self) -> PathBuf {
        self.raiz.join("vozes")
    }

    pub fn vozes(&self) -> Vec<VozSalva> {
        let mut v: Vec<VozSalva> = std::fs::read_dir(self.vozes_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| crate::paths::read_json::<VozSalva>(&e.path()))
            .collect();
        v.sort_by_key(|a| a.name.to_lowercase());
        v
    }

    fn arquivo_da_voz(&self, nome: &str) -> PathBuf {
        self.vozes_dir().join(format!("{}.json", crate::util::safe_file_name(nome)))
    }

    pub fn salvar_voz(&self, v: &VozSalva) -> std::io::Result<()> {
        std::fs::create_dir_all(self.vozes_dir())?;
        crate::paths::write_json(&self.arquivo_da_voz(&v.name), v)
    }

    pub fn apagar_voz(&self, nome: &str) -> bool {
        std::fs::remove_file(self.arquivo_da_voz(nome)).is_ok()
    }

    /// Espaço usado por motores e modelos de áudio.
    pub fn uso_em_disco(&self) -> u64 {
        crate::util::dir_size(&self.raiz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texto_do_chatterbox() {
        assert_eq!(texto_chatterbox("Olá, Mundo!", None), "[pt]ola\u{301}, mundo!");
        assert_eq!(texto_chatterbox("Ação", Some("Portuguese")), "[pt]ac\u{327}a\u{303}o");
        assert_eq!(texto_chatterbox("Hello", Some("en-US")), "[en]hello");
        assert_eq!(texto_chatterbox("Bonjour", Some("francês")), "[fr]bonjour");
        assert_eq!(codigo_idioma("pt_BR"), "pt");
        assert_eq!(codigo_idioma("klingon"), "pt", "idioma desconhecido fica no português");
    }

    #[test]
    fn perfis_da_voz_comercial_e_da_imagem() {
        assert_eq!(perfil(Familia::VozComercial, false, Some(8000)), "processador");
        assert_eq!(perfil(Familia::VozComercial, true, Some(8000)), "placa");
        assert_eq!(perfil(Familia::VozComercial, true, Some(2000)), "placa-pequena");
        assert_eq!(perfil(Familia::Imagem, true, Some(8176)), "placa", "RX 580 de 8 GB");
        assert_eq!(perfil(Familia::Imagem, true, Some(4096)), "placa-pequena");
        assert_eq!(perfil(Familia::Imagem, false, None), "processador");
    }

    #[test]
    fn argumentos_dos_servidores_novos() {
        let p = Path::new;
        let v = args_voz_comercial(p("mtl.gguf"), p("codec.gguf"), true);
        let pos = |a: &[String], f: &str| a.iter().position(|x| x == f).unwrap_or_else(|| panic!("{f} em {a:?}"));
        assert_eq!(v[pos(&v, "--backbone") + 1], "mtl.gguf");
        assert_eq!(v[pos(&v, "--model") + 1], "codec.gguf");
        assert_eq!(v[pos(&v, "--port") + 1], "{porta}");
        assert_eq!(v[pos(&v, "--host") + 1], "127.0.0.1");
        assert!(v.contains(&"--gpu".to_string()));
        assert!(!args_voz_comercial(p("a"), p("b"), false).contains(&"--gpu".to_string()));
        let i = args_imagem(p("d.gguf"), p("t.gguf"), p("vae.safetensors"), false);
        assert_eq!(i[pos(&i, "--diffusion-model") + 1], "d.gguf");
        assert_eq!(i[pos(&i, "--llm") + 1], "t.gguf");
        assert_eq!(i[pos(&i, "--vae") + 1], "vae.safetensors");
        assert_eq!(i[pos(&i, "--listen-ip") + 1], "127.0.0.1");
        assert_eq!(i[pos(&i, "--listen-port") + 1], "{porta}");
        assert_eq!(i[pos(&i, "-W") + 1], "512", "no processador, 512 por padrão");
        let g = args_imagem(p("d"), p("t"), p("v"), true);
        assert_eq!(g[pos(&g, "-W") + 1], "1024");
        assert_eq!(Familia::Imagem.rota_saude(), "/v1/models", "o sd-server não tem /health");
    }

    #[test]
    fn manifesto_embutido_e_valido() {
        let m = ManifestoAudio::embutido();
        m.validar().unwrap();
        // motores publicados de verdade (release motores-audio-N), não marcadores
        assert!(m.motores.iter().all(|x| x.arquivo.tamanho > 100_000), "preencha os motores com o SHA256SUMS.txt da release");
        assert!(m.url_base.contains(&m.versao), "a url_base aponta para a release {}", m.versao);
        // licenças que a janela mostra antes de baixar
        let voz = m.modelos.iter().find(|x| x.familia == Familia::Voz).unwrap();
        assert!(!voz.uso_comercial, "OmniVoice é CC BY-NC");
        assert!(m.modelos.iter().filter(|x| x.familia == Familia::Transcricao).all(|x| x.uso_comercial), "Whisper é MIT");
    }

    #[test]
    fn perfil_pela_memoria_da_placa() {
        assert_eq!(perfil(Familia::Voz, true, Some(16_000)), "placa-grande");
        assert_eq!(perfil(Familia::Voz, true, Some(8_000)), "placa");
        assert_eq!(perfil(Familia::Voz, true, Some(2_000)), "placa-pequena");
        assert_eq!(perfil(Familia::Voz, false, Some(16_000)), "processador");
        assert_eq!(perfil(Familia::Transcricao, true, Some(8_000)), "placa");
        assert_eq!(perfil(Familia::Transcricao, true, Some(1_000)), "processador");
        let m = ManifestoAudio::embutido();
        // RX 580 de 8 GB: OmniVoice Q8_0 e Whisper large-v3-turbo
        assert!(escolher_modelo(&m, Familia::Voz, true, Some(8_000)).unwrap().arquivos[0].nome.contains("Q8_0"));
        assert!(escolher_modelo(&m, Familia::Transcricao, true, Some(8_000)).unwrap().id.contains("turbo"));
        // só processador: os menores
        assert!(escolher_modelo(&m, Familia::Voz, false, None).unwrap().arquivos[0].nome.contains("Q4_K_M"));
    }

    #[test]
    fn placa_primeiro_processador_por_ultimo() {
        let m = ManifestoAudio::embutido();
        let c = candidatos(&m, Familia::Voz, "windows", true);
        assert_eq!(c.iter().map(|x| x.backend).collect::<Vec<_>>(), vec![Backend::Vulkan, Backend::Cpu]);
        let c = candidatos(&m, Familia::Transcricao, "linux", false);
        assert_eq!(c.iter().map(|x| x.backend).collect::<Vec<_>>(), vec![Backend::Cpu]);
    }

    #[test]
    fn traducao_acrescenta_o_campo_no_multipart() {
        let ct = "multipart/form-data; boundary=XyZ";
        let corpo = b"--XyZ\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\n\r\nRIFF\r\n--XyZ--\r\n";
        let novo = multipart_com_campo(corpo, ct, "translate", "true").unwrap();
        let s = String::from_utf8_lossy(&novo);
        assert!(s.contains("name=\"translate\"\r\n\r\ntrue\r\n--XyZ--"), "{s}");
        assert!(s.starts_with("--XyZ\r\nContent-Disposition: form-data; name=\"file\""));
        assert!(multipart_com_campo(corpo, "multipart/form-data", "x", "y").is_none());
    }

    #[test]
    fn nomes_de_voz() {
        assert!(nome_de_voz_valido("Minha voz-1"));
        assert!(!nome_de_voz_valido(""));
        assert!(!nome_de_voz_valido("../etc"));
        assert!(!nome_de_voz_valido(".oculta"));
        assert!(!nome_de_voz_valido(&"a".repeat(65)));
    }

    #[test]
    fn vozes_ficam_no_disco() {
        let d = tempfile::tempdir().unwrap();
        let s = AudioStore::new(d.path().to_path_buf(), d.path().join("dl"), None);
        assert!(s.vozes().is_empty());
        s.salvar_voz(&VozSalva { name: "Ana".into(), ref_text: "olá".into(), wav_b64: Some("UklGRg==".into()), rvq_b64: None }).unwrap();
        s.salvar_voz(&VozSalva { name: "bruno".into(), ref_text: "oi".into(), wav_b64: None, rvq_b64: Some("AAAA".into()) }).unwrap();
        assert_eq!(s.vozes().iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), vec!["Ana", "bruno"]);
        assert!(s.apagar_voz("Ana"));
        assert_eq!(s.vozes().len(), 1);
    }
}
