//! Atualização do próprio Ignis com um clique, dentro do `Motor`.
//!
//! Baixa da versão oficial só o arquivo do tipo de instalação deste computador,
//! confere o SHA-256, espera não haver conversa, fala ou transcrição em
//! andamento e entrega o comando de instalação para quem hospeda o Ignis (a
//! bandeja ou o `ignis serve`), que fecha tudo, roda o comando e sai.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::watch;

use super::{Motor, MotorError};
use crate::atualizador::{self, Comando, Instalacao};
use crate::consts::URL_RELEASES;
use crate::download::Progress;
use crate::updates::UpdateCheck;

const TAREFA: &str = "atualizacao:app";

impl Motor {
    /// Como o Ignis está instalado neste computador.
    pub fn instalacao(&self) -> &Instalacao {
        &self.instalacao
    }

    pub(super) fn instalacao_json(&self) -> Value {
        let i = &self.instalacao;
        let mut v = serde_json::to_value(i).unwrap_or_else(|_| json!({}));
        v["nome"] = json!(i.nome());
        v["automatica"] = json!(i.automatica());
        v["precisa_reiniciar_pc"] = json!(i.precisa_reiniciar_pc());
        v["arquivo"] = json!(i.arquivo());
        v
    }

    /// Quem hospeda o Ignis espera aqui. Quando chega um comando: fecha a janela,
    /// para os motores (`shutdown_all`), roda o comando separado
    /// (`atualizador::rodar_separado`) e sai.
    pub fn pedido_de_saida(&self) -> watch::Receiver<Option<Comando>> {
        self.saida.subscribe()
    }

    fn audio_em_uso(&self) -> usize {
        self.audio_uso.lock().unwrap().values().map(|(_, n)| *n).sum()
    }

    pub(super) fn atualizar_app(self: &Arc<Self>, info: UpdateCheck) -> Result<(), MotorError> {
        let app = info.app.ok_or_else(|| MotorError::Invalido("Você já está na versão mais nova do Ignis.".into()))?;
        let inst = self.instalacao.clone();
        if !inst.automatica() {
            return Err(MotorError::Invalido(inst.instrucao_manual(&app.versao, None)));
        }
        let cancel = self.task_begin(TAREFA, "atualizacao", &format!("Ignis {}", app.versao), None)?;
        let me = self.clone();
        tokio::spawn(async move {
            let base = std::env::var("IGNIS_URL_RELEASES").unwrap_or_else(|_| URL_RELEASES.to_string());
            let pasta = me.paths.data_dir.join("atualizacao");
            let _ = std::fs::remove_dir_all(&pasta);
            let cb = |p: Progress| me.task_progress(TAREFA, p);
            let arquivo = match atualizador::baixar(&me.http, &base, &app.versao, &inst, &pasta, &cancel, &cb).await {
                Ok(a) => a,
                Err(e) => {
                    me.task_finish(TAREFA, "erro", Some(e.clone()));
                    me.aviso("erro", format!("Não consegui baixar o Ignis {}: {e}. O Ignis atual continua funcionando.", app.versao));
                    return;
                }
            };
            me.task_finish(TAREFA, "concluida", None);
            // nunca no meio de uma conversa, fala ou transcrição
            let mut avisou = false;
            while me.ativos() > 0 || me.audio_em_uso() > 0 {
                if !avisou {
                    me.aviso("info", format!("Ignis {} baixado. Instalo assim que terminar o pedido em andamento.", app.versao));
                    avisou = true;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            let exe = std::env::current_exe().unwrap_or_default();
            let Some(comando) = atualizador::comando_de_instalacao(&inst, &arquivo, &exe, std::process::id()) else {
                me.aviso("erro", format!("Não consegui instalar sozinho. {}", inst.instrucao_manual(&app.versao, Some(&arquivo))));
                return;
            };
            if inst.precisa_reiniciar_pc() {
                // Fedora Atomic: instala com o Ignis aberto; a versão nova entra ao reiniciar
                me.aviso("info", format!("Instalando o Ignis {}. O sistema vai pedir a sua senha.", app.versao));
                let r = tokio::process::Command::new(&comando.programa).args(&comando.args).output().await;
                match r {
                    Ok(o) if o.status.success() => {
                        me.aviso("info", format!("Ignis {} instalado. Reinicie o computador para usar a versão nova.", app.versao));
                        if let Some(u) = me.update_info.write().unwrap().as_mut() {
                            u.app = None;
                        }
                        me.bump();
                    }
                    other => {
                        let motivo = match other {
                            Ok(o) => String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("").to_string(),
                            Err(e) => e.to_string(),
                        };
                        me.aviso(
                            "erro",
                            format!("A instalação não terminou ({motivo}). {}", inst.instrucao_manual(&app.versao, Some(&arquivo))),
                        );
                    }
                }
                return;
            }
            if let Instalacao::AppImage { caminho } = &inst {
                if let Err(e) = atualizador::trocar_appimage(&arquivo, caminho) {
                    me.aviso(
                        "erro",
                        format!(
                            "Não consegui trocar o AppImage em {} ({e}). O arquivo novo ficou em {}.",
                            caminho.display(),
                            arquivo.display()
                        ),
                    );
                    return;
                }
            }
            me.aviso("info", format!("Instalando o Ignis {}. Ele vai fechar e abrir de novo sozinho.", app.versao));
            tracing::info!("atualização pronta para instalar: {comando:?}");
            let _ = me.saida.send(Some(comando));
        });
        Ok(())
    }
}
