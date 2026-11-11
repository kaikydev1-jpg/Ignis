//! Botões da janela (minimizar, maximizar, fechar) adaptados ao sistema.
//!
//! A barra de título é desenhada pelo próprio Ignis. Para ficar em casa em cada
//! sistema, ela segue o lado, a ordem e os botões que o sistema usa (no Linux,
//! `gtk-decoration-layout`, que o GNOME e o KDE preenchem) e escolhe o estilo:
//! Windows 11, GNOME (redondos), KDE ou Mac (bolinhas coloridas).

use serde::Serialize;

pub const ESTILOS: [&str; 5] = ["auto", "windows", "gnome", "kde", "mac"];

const BOTOES: [&str; 3] = ["close", "minimize", "maximize"];
/// Padrão do GTK quando o sistema não diz nada.
const LAYOUT_GTK_PADRAO: &str = "menu:minimize,maximize,close";
const LAYOUT_WINDOWS: &str = ":minimize,maximize,close";
const LAYOUT_MAC: &str = "close,minimize,maximize:";

/// Como desenhar os botões: estilo e quais botões vão em cada lado, na ordem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BotoesJanela {
    pub estilo: String,
    pub esquerda: Vec<String>,
    pub direita: Vec<String>,
}

/// O que o sistema informa (vazio quando não se sabe).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Sistema {
    /// "windows", "linux" ou "mac".
    pub so: String,
    /// `gtk-decoration-layout`, ex.: "appmenu:minimize,maximize,close" ou "close,minimize,maximize:".
    pub layout: Option<String>,
    /// Nome do tema GTK, ex.: "Adwaita-dark", "WhiteSur-Dark".
    pub tema: Option<String>,
    /// `XDG_CURRENT_DESKTOP`, ex.: "GNOME", "KDE", "ubuntu:GNOME".
    pub desktop: Option<String>,
}

/// Lê "esquerda:direita", ficando só com fechar, minimizar e maximizar.
pub fn ler_layout(layout: &str) -> (Vec<String>, Vec<String>) {
    let (esq, dir) = layout.split_once(':').unwrap_or(("", layout));
    let lado = |s: &str| -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for b in s.split(',').map(str::trim) {
            if BOTOES.contains(&b) && !v.iter().any(|x| x == b) {
                v.push(b.to_string());
            }
        }
        v
    };
    let esquerda = lado(esq);
    let direita: Vec<String> = lado(dir).into_iter().filter(|b| !esquerda.contains(b)).collect();
    (esquerda, direita)
}

fn tema_de_mac(tema: &str) -> bool {
    let t = tema.to_lowercase().replace([' ', '-', '_'], "");
    ["whitesur", "mojave", "catalina", "bigsur", "monterey", "ventura", "sonoma", "sequoia", "macos", "cupertino", "mcos", "osx"]
        .iter()
        .any(|m| t.contains(m))
}

fn montar(estilo: &str, layout: &str) -> BotoesJanela {
    let (esquerda, mut direita) = ler_layout(layout);
    if esquerda.is_empty() && direita.is_empty() {
        // Sistema sem nenhum botão: deixa ao menos o fechar.
        direita.push("close".into());
    }
    BotoesJanela { estilo: estilo.to_string(), esquerda, direita }
}

/// Escolhe estilo e posição. `preferido` vem da configuração ("auto" segue o sistema).
pub fn escolher(sis: &Sistema, preferido: &str) -> BotoesJanela {
    let layout_do_sistema = sis.layout.as_deref().map(str::trim).filter(|l| {
        let (e, d) = ler_layout(l);
        !e.is_empty() || !d.is_empty()
    });
    let linux = sis.so != "windows" && sis.so != "mac";
    match preferido {
        "windows" => montar("windows", LAYOUT_WINDOWS),
        "mac" => montar("mac", LAYOUT_MAC),
        "gnome" | "kde" => montar(preferido, if linux { layout_do_sistema.unwrap_or(LAYOUT_GTK_PADRAO) } else { LAYOUT_WINDOWS }),
        _ => {
            if sis.so == "windows" {
                return montar("windows", LAYOUT_WINDOWS);
            }
            if sis.so == "mac" {
                return montar("mac", LAYOUT_MAC);
            }
            let layout = layout_do_sistema.unwrap_or(LAYOUT_GTK_PADRAO);
            let desktop = sis.desktop.as_deref().unwrap_or("").to_uppercase();
            let (esq, _) = ler_layout(layout);
            let estilo = if desktop.contains("PANTHEON") {
                "gnome"
            } else if sis.tema.as_deref().is_some_and(tema_de_mac) || esq.iter().any(|b| b == "close") {
                "mac"
            } else if desktop.contains("KDE") {
                "kde"
            } else {
                "gnome"
            };
            montar(estilo, layout)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux(layout: Option<&str>, tema: Option<&str>, desktop: Option<&str>) -> Sistema {
        Sistema { so: "linux".into(), layout: layout.map(String::from), tema: tema.map(String::from), desktop: desktop.map(String::from) }
    }
    fn v(x: &[&str]) -> Vec<String> {
        x.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn windows_sempre_no_estilo_do_windows_a_direita() {
        let b = escolher(&Sistema { so: "windows".into(), ..Default::default() }, "auto");
        assert_eq!(b, BotoesJanela { estilo: "windows".into(), esquerda: vec![], direita: v(&["minimize", "maximize", "close"]) });
    }

    #[test]
    fn gnome_padrao_segue_o_sistema() {
        // GNOME de fábrica mostra só o fechar
        let b = escolher(&linux(Some("appmenu:close"), Some("Adwaita"), Some("GNOME")), "auto");
        assert_eq!(b.estilo, "gnome");
        assert_eq!(b.direita, v(&["close"]));
        let b = escolher(&linux(Some("appmenu:minimize,maximize,close"), Some("Adwaita-dark"), Some("GNOME")), "auto");
        assert_eq!((b.estilo.as_str(), b.direita), ("gnome", v(&["minimize", "maximize", "close"])));
    }

    #[test]
    fn botoes_a_esquerda_viram_bolinhas_de_mac_na_ordem_do_sistema() {
        let b = escolher(&linux(Some("close,minimize,maximize:"), Some("Adwaita"), Some("GNOME")), "auto");
        assert_eq!(b, BotoesJanela { estilo: "mac".into(), esquerda: v(&["close", "minimize", "maximize"]), direita: vec![] });
    }

    #[test]
    fn tema_estilo_mac_vira_bolinhas_mesmo_a_direita() {
        let b = escolher(&linux(Some(":minimize,maximize,close"), Some("WhiteSur-Dark"), Some("GNOME")), "auto");
        assert_eq!(b.estilo, "mac");
        assert_eq!(b.direita, v(&["minimize", "maximize", "close"]));
        assert!(tema_de_mac("McMojave-light") && tema_de_mac("Big Sur") && !tema_de_mac("Breeze"));
    }

    #[test]
    fn kde_usa_estilo_do_kde() {
        let b = escolher(&linux(Some("icon:minimize,maximize,close"), Some("Breeze"), Some("KDE")), "auto");
        assert_eq!((b.estilo.as_str(), b.direita), ("kde", v(&["minimize", "maximize", "close"])));
    }

    #[test]
    fn sem_informacao_usa_o_padrao_do_gtk() {
        let b = escolher(&linux(None, None, None), "auto");
        assert_eq!((b.estilo.as_str(), b.direita), ("gnome", v(&["minimize", "maximize", "close"])));
        // layout sem botões conhecidos também cai no padrão
        let b = escolher(&linux(Some("menu:"), None, None), "auto");
        assert_eq!(b.direita, v(&["minimize", "maximize", "close"]));
    }

    #[test]
    fn escolha_manual_vence_o_automatico() {
        let s = linux(Some(":minimize,maximize,close"), Some("Adwaita"), Some("GNOME"));
        let b = escolher(&s, "mac");
        assert_eq!((b.estilo.as_str(), b.esquerda), ("mac", v(&["close", "minimize", "maximize"])));
        let b = escolher(&linux(Some("close,minimize,maximize:"), None, None), "windows");
        assert_eq!((b.estilo.as_str(), b.direita), ("windows", v(&["minimize", "maximize", "close"])));
        // GNOME/KDE escolhidos à mão continuam no lado do sistema
        let b = escolher(&linux(Some("close,minimize,maximize:"), None, None), "kde");
        assert_eq!((b.estilo.as_str(), b.esquerda), ("kde", v(&["close", "minimize", "maximize"])));
    }

    #[test]
    fn layout_estranho_nao_repete_nem_inventa_botao() {
        let (e, d) = ler_layout(" close , close,spacer,icon : maximize,close,xyz");
        assert_eq!((e, d), (v(&["close"]), v(&["maximize"])));
        assert_eq!(ler_layout("minimize,close"), (vec![], v(&["minimize", "close"])));
    }

    #[test]
    fn elementary_nao_vira_mac() {
        let b = escolher(&linux(Some("close:maximize"), Some("io.elementary.stylesheet.blueberry"), Some("Pantheon")), "auto");
        assert_eq!(b, BotoesJanela { estilo: "gnome".into(), esquerda: v(&["close"]), direita: v(&["maximize"]) });
    }
}
