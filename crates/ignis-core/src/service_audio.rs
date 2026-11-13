//! Voz e transcrição dentro do `Motor`: preparar (baixar), subir sob demanda,
//! cair para o processador se a placa falhar, descarregar quando parado.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use super::{Motor, MotorError};
use crate::audio::{self, Familia, ModeloAudio, MotorAudio, VozSalva};
use crate::download::{DownloadError, Progress};
use crate::engine::manifest::current_os;
use crate::engine::Backend;
use crate::supervisor::{self, Server};
use crate::util::now_unix;

/// O que está rodando numa família (para o estado e para os pedidos).
#[derive(Debug, Clone, Serialize)]
pub struct AudioInfo {
    pub familia: Familia,
    pub motor_id: String,
    pub backend: Backend,
    pub motor_nome: String,
    pub modelo_id: String,
    pub modelo_nome: String,
    pub base_url: String,
    pub desde: u64,
}

pub(crate) struct AudioRodando {
    pub server: Server,
    pub info: AudioInfo,
}

/// Uso em andamento de uma família (não descarrega no meio de um pedido).
pub struct AudioUso {
    motor: Arc<Motor>,
    familia: Familia,
}

impl Drop for AudioUso {
    fn drop(&mut self) {
        let mut u = self.motor.audio_uso.lock().unwrap();
        let e = u.entry(self.familia).or_insert((Instant::now(), 0));
        e.0 = Instant::now();
        e.1 = e.1.saturating_sub(1);
    }
}

fn nome_backend(b: Backend) -> &'static str {
    match b {
        Backend::Vulkan => "Vulkan (placa)",
        Backend::Cpu => "Processador",
        other => other.nome(),
    }
}

impl Motor {
    fn audio_slot(&self, f: Familia) -> &tokio::sync::Mutex<Option<AudioRodando>> {
        match f {
            Familia::Voz => &self.audio_voz,
            Familia::Transcricao => &self.audio_transcricao,
        }
    }

    pub fn audio_manifest(&self) -> &audio::ManifestoAudio {
        &self.audio_man
    }

    /// Onde ficam motores, modelos e vozes de áudio (usado também nos testes).
    pub fn audio_store(&self) -> &audio::AudioStore {
        &self.audio_store
    }

    /// (tem placa com Vulkan, memória de vídeo livre)
    fn audio_placa(&self) -> (bool, Option<u64>) {
        let Some(det) = self.detection() else { return (false, None) };
        let Some(g) = det.principal() else { return (false, None) };
        let lib = if cfg!(windows) { "vulkan-1.dll" } else { "libvulkan.so.1" };
        // sem informação sobre a biblioteca, tenta (se falhar, cai para o processador)
        let vulkan = det.libs.get(lib).copied().unwrap_or(true);
        (vulkan, g.vram_livre_mb.or(g.vram_total_mb))
    }

    fn audio_candidatos(&self, f: Familia) -> Vec<MotorAudio> {
        let (vulkan, _) = self.audio_placa();
        audio::candidatos(&self.audio_man, f, current_os(), vulkan).into_iter().cloned().collect()
    }

    fn audio_modelo_recomendado(&self, f: Familia) -> Option<ModeloAudio> {
        let (vulkan, vram) = self.audio_placa();
        audio::escolher_modelo(&self.audio_man, f, vulkan, vram).cloned()
    }

    /// Modelo instalado a usar: o recomendado se estiver baixado, senão qualquer um da família.
    fn audio_modelo_instalado(&self, f: Familia) -> Option<(ModeloAudio, Vec<std::path::PathBuf>)> {
        let rec = self.audio_modelo_recomendado(f);
        let mut ordem: Vec<&ModeloAudio> = self.audio_man.modelos.iter().filter(|m| m.familia == f).collect();
        if let Some(r) = &rec {
            ordem.sort_by_key(|m| if m.id == r.id { 0 } else { 1 });
        }
        ordem.into_iter().find_map(|m| self.audio_store.modelo_instalado(m).map(|a| (m.clone(), a)))
    }

    /// O que cada família tem e o que falta baixar.
    pub fn audio_plano(&self) -> Value {
        let mut out = serde_json::Map::new();
        for f in Familia::TODAS {
            let cands = self.audio_candidatos(f);
            let motores: Vec<Value> = cands
                .iter()
                .map(|m| {
                    json!({"id": m.id, "backend": m.backend, "nome": nome_backend(m.backend), "tamanho": m.arquivo.tamanho,
                           "instalado": self.audio_store.motor_instalado(&self.audio_man, m).is_some(),
                           "falhou": self.audio_falhas.lock().unwrap().contains(&m.id)})
                })
                .collect();
            let rec = self.audio_modelo_recomendado(f);
            let instalado = self.audio_modelo_instalado(f);
            let modelo = rec.as_ref().map(|m| {
                json!({"id": m.id, "nome": m.nome, "descricao": m.descricao, "licenca": m.licenca, "licenca_url": m.licenca_url,
                       "uso_comercial": m.uso_comercial, "tamanho": m.tamanho(),
                       "instalado": self.audio_store.modelo_instalado(m).is_some()})
            });
            let falta: u64 = cands
                .iter()
                .filter(|m| self.audio_store.motor_instalado(&self.audio_man, m).is_none())
                .map(|m| m.arquivo.tamanho)
                .sum::<u64>()
                + rec.as_ref().filter(|m| self.audio_store.modelo_instalado(m).is_none()).map(|m| m.tamanho()).unwrap_or(0);
            let algum_motor = cands.iter().any(|m| self.audio_store.motor_instalado(&self.audio_man, m).is_some());
            out.insert(
                f.chave().into(),
                json!({
                    "preparado": algum_motor && instalado.is_some(),
                    "motores": motores,
                    "modelo": modelo,
                    "modelo_em_uso": instalado.map(|(m, _)| json!({"id": m.id, "nome": m.nome, "licenca": m.licenca, "uso_comercial": m.uso_comercial})),
                    "falta_baixar": falta,
                    "rodando": self.audio_info.read().unwrap().get(&f),
                }),
            );
        }
        out.insert("vozes".into(), json!(self.audio_store.vozes().iter().map(|v| &v.name).collect::<Vec<_>>()));
        Value::Object(out)
    }

    /// Baixa os motores (placa e processador) e o modelo recomendado de cada família pedida.
    pub fn preparar_audio(self: &Arc<Self>, familias: Vec<Familia>) -> Result<(), MotorError> {
        if familias.is_empty() {
            return Err(MotorError::Invalido("Diga o que preparar: voz, transcrição ou os dois.".into()));
        }
        if self.detection().is_none() {
            return Err(MotorError::Ocupado("Ainda estou conhecendo a placa; tente de novo em instantes.".into()));
        }
        let me = self.clone();
        tokio::spawn(async move {
            for f in familias {
                for mo in me.audio_candidatos(f) {
                    if me.audio_store.motor_instalado(&me.audio_man, &mo).is_some() {
                        continue;
                    }
                    let tid = format!("audio:motor:{}", mo.id);
                    let Ok(cancel) = me.task_begin(
                        &tid,
                        "motor",
                        &format!("Motor de {} ({})", f.nome(), nome_backend(mo.backend)),
                        Some(mo.arquivo.tamanho),
                    ) else {
                        continue;
                    };
                    let cb = |p: Progress| me.task_progress(&tid, p);
                    match me.audio_store.instalar_motor(&me.http, &me.audio_man, &mo, &cancel, &cb).await {
                        Ok(_) => me.task_finish(&tid, "concluida", None),
                        Err(crate::engine::install::InstallError::Download(DownloadError::Cancelled)) => {
                            me.task_finish(&tid, "pausada", None);
                            return;
                        }
                        Err(e) => {
                            me.task_finish(&tid, "erro", Some(e.to_string()));
                            me.aviso("erro", format!("Não consegui instalar o motor de {}: {e}", f.nome()));
                        }
                    }
                }
                if let Some(m) = me.audio_modelo_recomendado(f) {
                    if me.audio_store.modelo_instalado(&m).is_none() {
                        let tid = format!("audio:modelo:{}", m.id);
                        if let Ok(cancel) = me.task_begin(&tid, "modelo", &m.nome, Some(m.tamanho())) {
                            let cb = |p: Progress| me.task_progress(&tid, p);
                            let token = me.hf_token();
                            match me.audio_store.instalar_modelo(&me.http, &m, &cancel, &cb, token.as_deref()).await {
                                Ok(_) => me.task_finish(&tid, "concluida", None),
                                Err(DownloadError::Cancelled) => {
                                    me.task_finish(&tid, "pausada", None);
                                    return;
                                }
                                Err(e) => {
                                    me.task_finish(&tid, "erro", Some(e.to_string()));
                                    me.aviso("erro", format!("Não consegui baixar {}: {e}", m.nome));
                                }
                            }
                        }
                    }
                }
                if me.audio_modelo_instalado(f).is_some()
                    && me.audio_candidatos(f).iter().any(|m| me.audio_store.motor_instalado(&me.audio_man, m).is_some())
                {
                    let pronto = match f {
                        Familia::Voz => "Voz pronta: o Ignis já fala e clona vozes.",
                        Familia::Transcricao => "Transcrição pronta: o Ignis já transcreve áudio.",
                    };
                    me.aviso("info", pronto);
                }
            }
            me.bump();
        });
        Ok(())
    }

    /// Marca o início de um pedido de áudio (até o fim dele, não descarrega).
    pub fn audio_inicio_uso(self: &Arc<Self>, f: Familia) -> AudioUso {
        let mut u = self.audio_uso.lock().unwrap();
        let e = u.entry(f).or_insert((Instant::now(), 0));
        e.0 = Instant::now();
        e.1 += 1;
        AudioUso { motor: self.clone(), familia: f }
    }

    /// Garante o servidor da família rodando; tenta a placa e cai para o processador.
    pub async fn garantir_audio(self: &Arc<Self>, f: Familia) -> Result<AudioInfo, MotorError> {
        if self.is_paused() {
            return Err(MotorError::Pausado);
        }
        let mut slot = self.audio_slot(f).lock().await;
        if let Some(r) = slot.as_mut() {
            if r.server.exited().is_none() {
                return Ok(r.info.clone());
            }
            *slot = None;
        }
        let nao_preparado = || {
            MotorError::AudioNaoPreparado(format!(
                "A {} ainda não foi preparada. Abra o Ignis, vá em Voz e transcrição e clique em Preparar.",
                f.nome()
            ))
        };
        let Some((modelo, arquivos)) = self.audio_modelo_instalado(f) else { return Err(nao_preparado()) };
        let falhas = self.audio_falhas.lock().unwrap().clone();
        let motores: Vec<(MotorAudio, std::path::PathBuf)> = self
            .audio_candidatos(f)
            .into_iter()
            .filter(|m| !falhas.contains(&m.id))
            .filter_map(|m| self.audio_store.motor_instalado(&self.audio_man, &m).map(|exe| (m, exe)))
            .collect();
        if motores.is_empty() {
            return Err(nao_preparado());
        }
        let mut erros = Vec::new();
        for (i, (mo, exe)) in motores.iter().enumerate() {
            let args = match f {
                Familia::Voz => audio::args_voz(&arquivos[0], arquivos.get(1).unwrap_or(&arquivos[0])),
                Familia::Transcricao => audio::args_transcricao(&arquivos[0]),
            };
            let spec = supervisor::LaunchSpec {
                exe: exe.clone(),
                modelo: arquivos[0].clone(),
                alias: f.chave().into(),
                contexto: 0,
                ngl: "0".into(),
                embeddings: false,
                backend: mo.backend,
                variante: mo.id.clone(),
                args_extras: vec![],
                mmproj: None,
                args_proprios: Some(args),
            };
            let t0 = Instant::now();
            match supervisor::start(&spec, &self.local, self.load_timeout).await {
                Ok(server) => {
                    let info = AudioInfo {
                        familia: f,
                        motor_id: mo.id.clone(),
                        backend: mo.backend,
                        motor_nome: nome_backend(mo.backend).into(),
                        modelo_id: modelo.id.clone(),
                        modelo_nome: modelo.nome.clone(),
                        base_url: server.base_url.clone(),
                        desde: now_unix(),
                    };
                    tracing::info!("{} pronta no motor {} em {:.1}s", f.nome(), mo.id, t0.elapsed().as_secs_f32());
                    if f == Familia::Voz {
                        self.registrar_vozes(&info.base_url).await;
                    }
                    if i > 0 {
                        self.aviso("aviso", format!("A {} não subiu na placa; usando o processador.", f.nome()));
                    }
                    self.audio_info.write().unwrap().insert(f, info.clone());
                    *slot = Some(AudioRodando { server, info: info.clone() });
                    self.audio_uso.lock().unwrap().entry(f).or_insert((Instant::now(), 0)).0 = Instant::now();
                    self.bump();
                    return Ok(info);
                }
                Err(e) => {
                    let motivo = e.to_string();
                    tracing::warn!("motor de {} {} falhou: {motivo}; registro: {:?}", f.nome(), mo.id, e.log());
                    self.audio_falhas.lock().unwrap().insert(mo.id.clone());
                    erros.push(format!("{}: {motivo}", nome_backend(mo.backend)));
                }
            }
        }
        Err(MotorError::Falhou(format!("Nenhum motor de {} conseguiu subir. {}", f.nome(), erros.join(" | "))))
    }

    /// Registra no servidor de voz as vozes guardadas pelo Ignis.
    async fn registrar_vozes(&self, base: &str) {
        for v in self.audio_store.vozes() {
            let r = self.local.post(format!("{base}/v1/audio/voices")).json(&v).timeout(Duration::from_secs(120)).send().await;
            match r {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => {
                    let txt = resp.text().await.unwrap_or_default();
                    self.aviso(
                        "aviso",
                        format!("A voz \"{}\" não pôde ser registrada: {}", v.name, txt.chars().take(200).collect::<String>()),
                    );
                }
                Err(e) => self.aviso("aviso", format!("A voz \"{}\" não pôde ser registrada: {e}", v.name)),
            }
        }
    }

    /// Endereço do servidor de voz, se já estiver rodando (sem subir).
    pub fn audio_rodando(&self, f: Familia) -> Option<AudioInfo> {
        self.audio_info.read().unwrap().get(&f).cloned()
    }

    pub fn vozes(&self) -> Vec<VozSalva> {
        self.audio_store.vozes()
    }

    /// Guarda uma voz clonada. Com o servidor de voz no ar, registra nele também
    /// (e, se ele recusar, não guarda).
    pub async fn salvar_voz(self: &Arc<Self>, v: VozSalva) -> Result<Value, MotorError> {
        if !audio::nome_de_voz_valido(&v.name) {
            return Err(MotorError::Invalido(
                "Nome de voz inválido: use letras, números, espaço, ponto, hífen ou sublinhado (até 64).".into(),
            ));
        }
        if v.wav_b64.as_deref().unwrap_or("").is_empty() && v.rvq_b64.as_deref().unwrap_or("").is_empty() {
            return Err(MotorError::Invalido("Mande a gravação da voz em \"wav_b64\" (WAV em base64) ou \"rvq_b64\".".into()));
        }
        if let Some(info) = self.audio_rodando(Familia::Voz) {
            let resp = self
                .local
                .post(format!("{}/v1/audio/voices", info.base_url))
                .json(&v)
                .timeout(Duration::from_secs(120))
                .send()
                .await
                .map_err(|e| MotorError::Falhou(format!("O servidor de voz não respondeu ({e}).")))?;
            if !resp.status().is_success() {
                let txt = resp.text().await.unwrap_or_default();
                return Err(MotorError::Invalido(format!(
                    "O servidor de voz recusou a voz: {}",
                    txt.chars().take(300).collect::<String>()
                )));
            }
        }
        self.audio_store.salvar_voz(&v).map_err(|e| MotorError::Falhou(e.to_string()))?;
        self.bump();
        Ok(json!({"ok": true, "name": v.name}))
    }

    /// Apaga uma voz. O servidor de voz guarda vozes só na memória, então ele é
    /// reiniciado no próximo pedido, já sem ela.
    pub async fn apagar_voz(&self, nome: &str) -> bool {
        let ok = self.audio_store.apagar_voz(nome);
        if ok {
            self.descarregar_audio(Some(Familia::Voz)).await;
        }
        ok
    }

    /// Para os servidores de áudio (uma família ou as duas).
    pub async fn descarregar_audio(&self, f: Option<Familia>) -> usize {
        let mut n = 0;
        for fam in Familia::TODAS {
            if f.is_some_and(|x| x != fam) {
                continue;
            }
            let mut slot = self.audio_slot(fam).lock().await;
            if let Some(mut r) = slot.take() {
                r.server.stop().await;
                n += 1;
            }
            self.audio_info.write().unwrap().remove(&fam);
        }
        if n > 0 {
            self.bump();
        }
        n
    }

    /// Vigia: queda (troca para o processador no próximo pedido) e descarregar quando parado.
    pub(crate) async fn vigiar_audio(&self) {
        let limite = Duration::from_secs(self.settings().descarregar_apos_seg.max(1));
        for f in Familia::TODAS {
            let Ok(mut slot) = self.audio_slot(f).try_lock() else { continue };
            let Some(r) = slot.as_mut() else { continue };
            if let Some(status) = r.server.exited() {
                let log = r.server.log_tail(60);
                let motivo = supervisor::classify_failure(&log, status.code());
                let info = r.info.clone();
                *slot = None;
                self.audio_info.write().unwrap().remove(&f);
                if info.backend != Backend::Cpu {
                    self.audio_falhas.lock().unwrap().insert(info.motor_id.clone());
                }
                self.aviso("aviso", format!("O motor de {} parou ({motivo}). No próximo pedido ele sobe de novo.", f.nome()));
                self.bump();
                continue;
            }
            let (ultimo, ativos) = self.audio_uso.lock().unwrap().get(&f).copied().unwrap_or((Instant::now(), 0));
            if ativos == 0 && ultimo.elapsed() > limite {
                r.server.stop().await;
                *slot = None;
                self.audio_info.write().unwrap().remove(&f);
                self.aviso("info", format!("Motor de {} descarregado depois de {} min parado.", f.nome(), limite.as_secs() / 60));
                self.bump();
            }
        }
    }

    pub(crate) fn audio_status_json(&self) -> Value {
        let info = self.audio_info.read().unwrap().clone();
        let mut out: HashMap<&str, Value> = HashMap::new();
        for f in Familia::TODAS {
            let preparado = self.audio_modelo_instalado(f).is_some()
                && self.audio_candidatos(f).iter().any(|m| self.audio_store.motor_instalado(&self.audio_man, m).is_some());
            out.insert(f.chave(), json!({"preparado": preparado, "rodando": info.get(&f)}));
        }
        json!(out)
    }
}
