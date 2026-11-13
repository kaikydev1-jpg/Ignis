//! Atualização do próprio Ignis, com um clique.
//!
//! Descobre como o Ignis foi instalado, baixa da versão oficial (GitHub
//! Releases) só o arquivo desse tipo de instalação, confere o SHA-256 contra a
//! lista publicada (`SHA256SUMS.txt`) e monta o comando que instala por cima e
//! reabre o Ignis. O comando roda depois que o Ignis fecha.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::download::{self, Progress};

pub const APP_ID_FLATPAK: &str = "io.github.kaikydev1jpg.Ignis";

/// Como o Ignis está instalado neste computador.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "tipo", rename_all = "kebab-case")]
pub enum Instalacao {
    Windows,
    Flatpak {
        matsu: bool,
        sistema: bool,
    },
    #[serde(rename = "appimage")]
    AppImage {
        caminho: PathBuf,
    },
    Rpm,
    Deb,
    /// RPM em camada no Fedora Atomic (rpm-ostree): instala e pede para reiniciar o computador.
    RpmOstree {
        pacote: String,
    },
    /// Rodando do código ou de um jeito que não dá para atualizar sozinho.
    Manual,
}

impl Instalacao {
    pub fn nome(&self) -> &'static str {
        match self {
            Instalacao::Windows => "Windows",
            Instalacao::Flatpak { matsu: true, .. } => "Matsu OS (Flatpak)",
            Instalacao::Flatpak { .. } => "Flatpak",
            Instalacao::AppImage { .. } => "AppImage",
            Instalacao::Rpm => "Fedora (RPM)",
            Instalacao::Deb => "Ubuntu/Debian (.deb)",
            Instalacao::RpmOstree { .. } => "Fedora Atomic (RPM em camada)",
            Instalacao::Manual => "instalação manual",
        }
    }

    /// Arquivo da versão oficial para este tipo de instalação.
    pub fn arquivo(&self) -> Option<&'static str> {
        match self {
            Instalacao::Windows => Some("Ignis-Windows.exe"),
            Instalacao::Flatpak { matsu: true, .. } => Some("Ignis-Matsu-OS.flatpak"),
            Instalacao::Flatpak { .. } => Some("Ignis-Flatpak.flatpak"),
            Instalacao::AppImage { .. } => Some("Ignis-Linux.AppImage"),
            Instalacao::Rpm | Instalacao::RpmOstree { .. } => Some("Ignis-Fedora.rpm"),
            Instalacao::Deb => Some("Ignis-Ubuntu-Debian.deb"),
            Instalacao::Manual => None,
        }
    }

    /// Dá para atualizar com um clique?
    pub fn automatica(&self) -> bool {
        !matches!(self, Instalacao::Manual)
    }

    /// No Fedora Atomic a versão nova só entra depois de reiniciar o computador.
    pub fn precisa_reiniciar_pc(&self) -> bool {
        matches!(self, Instalacao::RpmOstree { .. })
    }

    /// Como instalar à mão o arquivo já baixado (quando o clique não dá certo).
    pub fn instrucao_manual(&self, versao: &str, arquivo: Option<&Path>) -> String {
        let arq = arquivo.map(|a| a.display().to_string()).unwrap_or_else(|| self.arquivo().unwrap_or("").to_string());
        match self {
            Instalacao::Rpm => format!("Para instalar a versão {versao} pelo terminal: sudo dnf install -y \"{arq}\""),
            Instalacao::Deb => format!("Para instalar a versão {versao} pelo terminal: sudo apt-get install -y \"{arq}\""),
            Instalacao::RpmOstree { pacote } => format!(
                "Para instalar a versão {versao} pelo terminal: rpm-ostree uninstall {pacote} --install \"{arq}\" e reinicie o computador."
            ),
            Instalacao::Flatpak { sistema, .. } => format!(
                "Para instalar a versão {versao} pelo terminal: flatpak install {} -y --reinstall --bundle \"{arq}\"",
                if *sistema { "--system" } else { "--user" }
            ),
            _ => format!("Baixe a versão {versao} na página oficial: https://github.com/kaikydev1-jpg/Ignis/releases/latest"),
        }
    }
}

/// Descobre o tipo de instalação.
pub fn detectar() -> Instalacao {
    if cfg!(windows) {
        let exe = std::env::current_exe().unwrap_or_default();
        // instalado pelo instalador (pasta com o desinstalador); rodando de "target" = código
        let instalado = exe.parent().is_some_and(|d| d.join("uninstall.exe").is_file());
        return if instalado { Instalacao::Windows } else { Instalacao::Manual };
    }
    if std::env::var_os("FLATPAK_ID").is_some() || Path::new("/.flatpak-info").exists() {
        let info = std::fs::read_to_string("/.flatpak-info").unwrap_or_default();
        let sistema = info.lines().any(|l| l.starts_with("app-path=") && l.contains("/var/lib/flatpak"));
        let matsu = crate::distro::eh_matsu(&crate::distro::ler_os_release());
        return Instalacao::Flatpak { matsu, sistema };
    }
    if let Some(p) = std::env::var_os("APPIMAGE") {
        return Instalacao::AppImage { caminho: PathBuf::from(p) };
    }
    let exe = std::env::current_exe().unwrap_or_default();
    if !exe.starts_with("/usr") {
        return Instalacao::Manual;
    }
    let dono = |prog: &str, args: &[&str]| {
        std::process::Command::new(prog)
            .args(args)
            .arg(&exe)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    if dono("rpm", &["-qf"]) {
        if Path::new("/run/ostree-booted").exists() {
            let pacote = std::process::Command::new("rpm")
                .args(["-qf", "--qf", "%{NAME}"])
                .arg(&exe)
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|n| !n.is_empty() && !n.contains(char::is_whitespace))
                .unwrap_or_else(|| "ignis".into());
            return Instalacao::RpmOstree { pacote };
        }
        return Instalacao::Rpm;
    }
    if dono("dpkg", &["-S"]) {
        return Instalacao::Deb;
    }
    Instalacao::Manual
}

/// Base dos downloads da versão oficial (`IGNIS_URL_RELEASES` troca, nos testes).
pub fn url_versao(base: &str, versao: &str) -> String {
    format!("{}/download/v{}/", base.trim_end_matches('/'), versao.trim_start_matches('v'))
}

/// SHA-256 de um arquivo na lista `SHA256SUMS.txt`.
pub fn sha_na_lista(lista: &str, arquivo: &str) -> Option<String> {
    lista.lines().find_map(|l| {
        let (sha, nome) = l.split_once(char::is_whitespace)?;
        let nome = nome.trim().trim_start_matches('*');
        (nome == arquivo && sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit())).then(|| sha.to_lowercase())
    })
}

/// Baixa o arquivo da versão para esta instalação, conferido pelo SHA-256 da lista.
pub async fn baixar(
    client: &reqwest::Client,
    base: &str,
    versao: &str,
    inst: &Instalacao,
    pasta: &Path,
    cancel: &CancellationToken,
    progress: &(dyn Fn(Progress) + Send + Sync),
) -> Result<PathBuf, String> {
    let arquivo = inst.arquivo().ok_or("este tipo de instalação não atualiza sozinho")?;
    let url = url_versao(base, versao);
    let lista = client
        .get(format!("{url}SHA256SUMS.txt"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("não consegui ler a lista de conferência da versão {versao}: {e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let sha = sha_na_lista(&lista, arquivo).ok_or_else(|| format!("{arquivo} não está na lista da versão {versao}"))?;
    std::fs::create_dir_all(pasta).map_err(|e| e.to_string())?;
    let destino = pasta.join(arquivo);
    download::download_file(client, &format!("{url}{arquivo}"), &destino, Some(&sha), None, cancel, progress, None)
        .await
        .map_err(|e| e.to_string())?;
    Ok(destino)
}

/// Troca o AppImage em uso pelo novo (mesma pasta: troca atômica).
pub fn trocar_appimage(novo: &Path, atual: &Path) -> std::io::Result<()> {
    let pasta = atual.parent().ok_or_else(|| std::io::Error::other("AppImage sem pasta"))?;
    let temp = pasta.join(".Ignis-atualizando.AppImage");
    std::fs::copy(novo, &temp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&temp, atual)?;
    let _ = std::fs::remove_file(novo);
    Ok(())
}

/// Comando (programa e argumentos) que instala e reabre o Ignis depois que ele fechar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Comando {
    pub programa: String,
    pub args: Vec<String>,
}

fn aspas(s: &Path) -> String {
    format!("'{}'", s.to_string_lossy().replace('\'', "'\\''"))
}

fn aspas_ps(s: &Path) -> String {
    format!("'{}'", s.to_string_lossy().replace('\'', "''"))
}

fn tem(programa: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(programa).is_file()))
}

pub fn comando_de_instalacao(inst: &Instalacao, arquivo: &Path, exe_atual: &Path, pid: u32) -> Option<Comando> {
    let espera = format!("while kill -0 {pid} 2>/dev/null; do sleep 0.5; done");
    let sh = |script: String| Comando { programa: "sh".into(), args: vec!["-c".into(), script] };
    match inst {
        Instalacao::Windows => Some(Comando {
            programa: "powershell".into(),
            args: vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-WindowStyle".into(),
                "Hidden".into(),
                "-Command".into(),
                // espera o Ignis fechar, instala em silêncio e abre de novo
                format!(
                    "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; Start-Sleep -Seconds 1; Start-Process -Wait -FilePath {} -ArgumentList '/S'; Start-Process -FilePath {}",
                    aspas_ps(arquivo),
                    aspas_ps(exe_atual)
                ),
            ],
        }),
        Instalacao::Rpm if tem("pkexec") => {
            Some(sh(format!("{espera}; pkexec dnf install -y {} && exec {}", aspas(arquivo), aspas(exe_atual))))
        }
        Instalacao::Deb if tem("pkexec") => {
            Some(sh(format!("{espera}; pkexec apt-get install -y {} && exec {}", aspas(arquivo), aspas(exe_atual))))
        }
        Instalacao::AppImage { caminho } => Some(sh(format!("{espera}; exec {}", aspas(caminho)))),
        Instalacao::Flatpak { sistema, .. } => Some(Comando {
            programa: "flatpak-spawn".into(),
            args: vec![
                "--host".into(),
                "sh".into(),
                "-c".into(),
                format!(
                    "sleep 4; flatpak install {} -y --noninteractive --reinstall --bundle {} && flatpak run {APP_ID_FLATPAK}",
                    if *sistema { "--system" } else { "--user" },
                    aspas(arquivo)
                ),
            ],
        }),
        // roda com o Ignis aberto; o sistema pede a senha e a versão nova entra ao reiniciar
        Instalacao::RpmOstree { pacote } => {
            Some(Comando { programa: "rpm-ostree".into(), args: vec!["uninstall".into(), pacote.clone(), "--install".into(), arquivo.to_string_lossy().into()] })
        }
        _ => None,
    }
}

/// Roda o comando separado do Ignis (continua depois que ele fechar).
pub fn rodar_separado(c: &Comando) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(&c.programa);
    cmd.args(&c.args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // sessão própria: não morre junto com o Ignis
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    cmd.spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arquivo_certo_para_cada_instalacao() {
        assert_eq!(Instalacao::Windows.arquivo(), Some("Ignis-Windows.exe"));
        assert_eq!(Instalacao::Flatpak { matsu: true, sistema: false }.arquivo(), Some("Ignis-Matsu-OS.flatpak"));
        assert_eq!(Instalacao::Flatpak { matsu: false, sistema: false }.arquivo(), Some("Ignis-Flatpak.flatpak"));
        assert_eq!(Instalacao::AppImage { caminho: "/x".into() }.arquivo(), Some("Ignis-Linux.AppImage"));
        assert_eq!(Instalacao::Rpm.arquivo(), Some("Ignis-Fedora.rpm"));
        assert_eq!(Instalacao::Deb.arquivo(), Some("Ignis-Ubuntu-Debian.deb"));
        let atomic = Instalacao::RpmOstree { pacote: "ignis".into() };
        assert!(atomic.automatica() && atomic.precisa_reiniciar_pc() && !Instalacao::Manual.automatica());
        assert!(atomic.instrucao_manual("0.3.0", None).contains("rpm-ostree uninstall ignis --install"));
        assert!(Instalacao::Rpm
            .instrucao_manual("0.3.0", Some(Path::new("/d/Ignis-Fedora.rpm")))
            .contains("dnf install -y \"/d/Ignis-Fedora.rpm\""));
    }

    #[test]
    fn le_a_lista_de_conferencia() {
        let lista = format!("{}  Ignis-Fedora.rpm\n{} *Ignis-Windows.exe\n", "a".repeat(64), "B".repeat(64));
        assert_eq!(sha_na_lista(&lista, "Ignis-Fedora.rpm"), Some("a".repeat(64)));
        assert_eq!(sha_na_lista(&lista, "Ignis-Windows.exe"), Some("b".repeat(64)));
        assert_eq!(sha_na_lista(&lista, "Ignis-Linux.AppImage"), None);
        assert_eq!(url_versao("https://github.com/x/y/releases/", "v0.3.0"), "https://github.com/x/y/releases/download/v0.3.0/");
    }

    #[test]
    fn comandos_por_sistema() {
        let c = comando_de_instalacao(&Instalacao::Windows, Path::new("C:\\t\\Ignis-Windows.exe"), Path::new("C:\\Ignis\\ignis.exe"), 1)
            .unwrap();
        assert_eq!(c.programa, "powershell");
        let ps = c.args.last().unwrap();
        assert!(
            ps.starts_with("Wait-Process -Id 1 ")
                && ps.contains("'C:\\t\\Ignis-Windows.exe' -ArgumentList '/S'")
                && ps.ends_with("Start-Process -FilePath 'C:\\Ignis\\ignis.exe'"),
            "{ps}"
        );
        let c =
            comando_de_instalacao(&Instalacao::RpmOstree { pacote: "ignis".into() }, Path::new("/d/I.rpm"), Path::new("/usr/bin/ignis"), 1)
                .unwrap();
        assert_eq!(c.args, ["uninstall", "ignis", "--install", "/d/I.rpm"]);
        let c = comando_de_instalacao(
            &Instalacao::Flatpak { matsu: true, sistema: false },
            Path::new("/home/a/x.flatpak"),
            Path::new("/app/bin/ignis"),
            1,
        )
        .unwrap();
        assert_eq!(&c.args[..2], &["--host".to_string(), "sh".to_string()]);
        assert!(
            c.args[3].contains("flatpak install --user") && c.args[3].contains("--reinstall --bundle '/home/a/x.flatpak'"),
            "{}",
            c.args[3]
        );
        let c = comando_de_instalacao(
            &Instalacao::AppImage { caminho: "/home/a/O'Neil/Ignis.AppImage".into() },
            Path::new("/t"),
            Path::new("/t"),
            42,
        )
        .unwrap();
        assert!(
            c.args[1].starts_with("while kill -0 42") && c.args[1].ends_with("exec '/home/a/O'\\''Neil/Ignis.AppImage'"),
            "{}",
            c.args[1]
        );
        assert!(comando_de_instalacao(&Instalacao::Manual, Path::new("/t"), Path::new("/t"), 1).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn troca_o_appimage_em_uso() {
        let d = tempfile::tempdir().unwrap();
        let atual = d.path().join("Ignis.AppImage");
        std::fs::write(&atual, b"velho").unwrap();
        let novo = d.path().join("baixado.AppImage");
        std::fs::write(&novo, b"novo").unwrap();
        trocar_appimage(&novo, &atual).unwrap();
        assert_eq!(std::fs::read(&atual).unwrap(), b"novo");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&atual).unwrap().permissions().mode() & 0o111, 0o111);
    }
}
