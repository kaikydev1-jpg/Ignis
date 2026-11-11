//! Configurações da pessoa, gravadas em `config.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::consts::PORTA_PADRAO;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Porta da API local (sempre em 127.0.0.1).
    pub porta: u16,
    /// Abrir junto com o computador.
    pub abrir_com_computador: bool,
    /// Descarregar o modelo depois de tantos segundos parado (0 = nunca).
    pub descarregar_apos_seg: u64,
    /// Variante de motor escolhida à mão (ex.: "linux-vulkan"). `None` = automático.
    pub motor_forcado: Option<String>,
    /// Tamanho de contexto desejado (o orçamento pode reduzir).
    pub contexto_preferido: u32,
    /// Procurar atualizações sozinho uma vez por semana (desligado por padrão: sem conexões sem ação da pessoa).
    pub procurar_atualizacoes_sozinho: bool,
    /// Caminho de um `llama-server` próprio (opcional, entra como candidato).
    pub llama_server_proprio: Option<PathBuf>,
    /// Modelo usado quando o pedido não diz qual.
    pub modelo_padrao: Option<String>,
    /// Já passou pela primeira execução (liga o "abrir com o computador" uma vez).
    pub primeira_execucao_feita: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            porta: PORTA_PADRAO,
            abrir_com_computador: true,
            descarregar_apos_seg: 10 * 60,
            motor_forcado: None,
            contexto_preferido: 8192,
            procurar_atualizacoes_sozinho: false,
            llama_server_proprio: None,
            modelo_padrao: None,
            primeira_execucao_feita: false,
        }
    }
}

impl Settings {
    /// Corrige valores fora do razoável.
    pub fn sanitize(&mut self) {
        if self.porta < 1024 {
            self.porta = PORTA_PADRAO;
        }
        self.contexto_preferido = self.contexto_preferido.clamp(1024, 262_144);
    }
}

/// Alterações parciais vindas da janela ou da API.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SettingsPatch {
    pub porta: Option<u16>,
    pub abrir_com_computador: Option<bool>,
    pub descarregar_apos_seg: Option<u64>,
    pub descarregar_apos_min: Option<u64>,
    pub contexto_preferido: Option<u32>,
    pub procurar_atualizacoes_sozinho: Option<bool>,
    /// String vazia remove.
    pub llama_server_proprio: Option<String>,
    /// String vazia remove.
    pub modelo_padrao: Option<String>,
}

impl SettingsPatch {
    pub fn apply(self, s: &mut Settings) {
        if let Some(v) = self.porta {
            s.porta = v;
        }
        if let Some(v) = self.abrir_com_computador {
            s.abrir_com_computador = v;
        }
        if let Some(v) = self.descarregar_apos_min {
            s.descarregar_apos_seg = v * 60;
        }
        if let Some(v) = self.descarregar_apos_seg {
            s.descarregar_apos_seg = v;
        }
        if let Some(v) = self.contexto_preferido {
            s.contexto_preferido = v;
        }
        if let Some(v) = self.procurar_atualizacoes_sozinho {
            s.procurar_atualizacoes_sozinho = v;
        }
        if let Some(v) = self.llama_server_proprio {
            s.llama_server_proprio = if v.trim().is_empty() { None } else { Some(PathBuf::from(v.trim())) };
        }
        if let Some(v) = self.modelo_padrao {
            s.modelo_padrao = if v.trim().is_empty() { None } else { Some(v.trim().to_string()) };
        }
        s.sanitize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padrao_tem_porta_e_descarregar_10_min() {
        let s = Settings::default();
        assert_eq!(s.porta, PORTA_PADRAO);
        assert_eq!(s.descarregar_apos_seg, 600);
        assert!(s.abrir_com_computador);
        assert!(!s.procurar_atualizacoes_sozinho);
    }

    #[test]
    fn json_antigo_sem_campos_usa_padrao() {
        let s: Settings = serde_json::from_str(r#"{"porta": 12000}"#).unwrap();
        assert_eq!(s.porta, 12000);
        assert_eq!(s.contexto_preferido, 8192);
    }

    #[test]
    fn patch_aplica_e_saneia() {
        let mut s = Settings::default();
        SettingsPatch { porta: Some(80), descarregar_apos_min: Some(3), llama_server_proprio: Some("  ".into()), ..Default::default() }
            .apply(&mut s);
        assert_eq!(s.porta, PORTA_PADRAO, "porta privilegiada volta ao padrão");
        assert_eq!(s.descarregar_apos_seg, 180);
        assert_eq!(s.llama_server_proprio, None);
    }
}
