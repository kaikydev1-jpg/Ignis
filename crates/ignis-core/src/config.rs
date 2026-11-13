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
    /// Procurar versão nova sozinho, uma vez por dia (só lê a lista pública da versão no GitHub; nada é enviado).
    /// Nome novo de propósito: quem tinha a opção antiga desligada (o padrão antigo) passa a receber o aviso.
    pub procurar_atualizacoes: bool,
    /// Caminho de um `llama-server` próprio (opcional, entra como candidato).
    pub llama_server_proprio: Option<PathBuf>,
    /// Modelo usado quando o pedido não diz qual.
    pub modelo_padrao: Option<String>,
    /// Já passou pela primeira execução (liga o "abrir com o computador" uma vez).
    pub primeira_execucao_feita: bool,
    /// Estilo dos botões da janela: "auto" (segue o sistema), "windows", "gnome", "kde" ou "mac".
    pub botoes_janela: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            porta: PORTA_PADRAO,
            abrir_com_computador: true,
            descarregar_apos_seg: 10 * 60,
            motor_forcado: None,
            contexto_preferido: 8192,
            procurar_atualizacoes: true,
            llama_server_proprio: None,
            modelo_padrao: None,
            primeira_execucao_feita: false,
            botoes_janela: "auto".into(),
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
        if !crate::janela::ESTILOS.contains(&self.botoes_janela.as_str()) {
            self.botoes_janela = "auto".into();
        }
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
    pub procurar_atualizacoes: Option<bool>,
    /// Nome antigo (0.1/0.2), ainda aceito pela API.
    pub procurar_atualizacoes_sozinho: Option<bool>,
    /// String vazia remove.
    pub llama_server_proprio: Option<String>,
    /// String vazia remove.
    pub modelo_padrao: Option<String>,
    pub botoes_janela: Option<String>,
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
        if let Some(v) = self.procurar_atualizacoes.or(self.procurar_atualizacoes_sozinho) {
            s.procurar_atualizacoes = v;
        }
        if let Some(v) = self.llama_server_proprio {
            s.llama_server_proprio = if v.trim().is_empty() { None } else { Some(PathBuf::from(v.trim())) };
        }
        if let Some(v) = self.modelo_padrao {
            s.modelo_padrao = if v.trim().is_empty() { None } else { Some(v.trim().to_string()) };
        }
        if let Some(v) = self.botoes_janela {
            s.botoes_janela = v.trim().to_lowercase();
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
        assert!(s.procurar_atualizacoes);
        // a opção antiga desligada não desliga o aviso novo
        let antigo: Settings = serde_json::from_str(r#"{"procurar_atualizacoes_sozinho": false}"#).unwrap();
        assert!(antigo.procurar_atualizacoes);
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

    #[test]
    fn botoes_da_janela_aceitam_so_estilos_conhecidos() {
        let mut s = Settings::default();
        assert_eq!(s.botoes_janela, "auto");
        SettingsPatch { botoes_janela: Some(" Mac ".into()), ..Default::default() }.apply(&mut s);
        assert_eq!(s.botoes_janela, "mac");
        SettingsPatch { botoes_janela: Some("amiga".into()), ..Default::default() }.apply(&mut s);
        assert_eq!(s.botoes_janela, "auto", "estilo desconhecido volta ao automático");
    }
}
