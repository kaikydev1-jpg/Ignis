//! Pequenas utilidades de sistema.

use std::time::{SystemTime, UNIX_EPOCH};

/// Não abre janela de console no Windows ao rodar programas auxiliares.
pub fn hide_console(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// (total, disponível) de RAM em MB.
pub fn system_memory_mb() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
            return parse_meminfo(&text);
        }
        (0, 0)
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut st = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        unsafe {
            if GlobalMemoryStatusEx(&mut st).is_ok() {
                return (st.ullTotalPhys / (1024 * 1024), st.ullAvailPhys / (1024 * 1024));
            }
        }
        (0, 0)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        (0, 0)
    }
}

pub fn parse_meminfo(text: &str) -> (u64, u64) {
    let mut total = 0;
    let mut avail = 0;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match (it.next(), it.next()) {
            (Some("MemTotal:"), Some(v)) => total = v.parse::<u64>().unwrap_or(0) / 1024,
            (Some("MemAvailable:"), Some(v)) => avail = v.parse::<u64>().unwrap_or(0) / 1024,
            _ => {}
        }
    }
    (total, avail)
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Tamanho legível em português ("1,8 GB").
pub fn human_size(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    let s = if b >= GB { format!("{:.1} GB", b / GB) } else { format!("{:.0} MB", b / MB) };
    s.replace('.', ",")
}

/// Porta livre em 127.0.0.1 (para o llama-server interno).
pub fn free_local_port() -> std::io::Result<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(l.local_addr()?.port())
}

/// Nome de arquivo seguro a partir de um texto qualquer.
pub fn safe_file_name(s: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    while out.starts_with('.') {
        out.remove(0);
    }
    if out.is_empty() {
        out.push_str("modelo");
    }
    out.truncate(120);
    out
}

/// Tamanho total de uma pasta (recursivo; ignora o que não conseguir ler).
pub fn dir_size(dir: &std::path::Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo() {
        let (t, a) = parse_meminfo("MemTotal:       16303428 kB\nMemFree:  100 kB\nMemAvailable:   12000000 kB\n");
        assert_eq!(t, 15921);
        assert_eq!(a, 11718);
    }

    #[test]
    fn tamanho_legivel() {
        assert_eq!(human_size(639_446_688), "610 MB");
        assert_eq!(human_size(5_027_783_488), "4,7 GB");
    }

    #[test]
    fn nome_seguro() {
        assert_eq!(safe_file_name("../library/llama3.2:3b"), "_library_llama3.2_3b");
        assert_eq!(safe_file_name(""), "modelo");
    }
}
