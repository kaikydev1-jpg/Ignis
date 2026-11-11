//! Detecção no Windows via DXGI (nome, IDs PCI, VRAM total e livre, driver).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DxgiAdapter {
    pub nome: String,
    pub vendor: u16,
    pub device: u16,
    pub vram_total_mb: u64,
    pub vram_livre_mb: Option<u64>,
    pub driver: Option<String>,
}

#[cfg(windows)]
pub fn enumerate() -> Vec<DxgiAdapter> {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter1, IDXGIAdapter3, IDXGIDevice, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
        DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
    };

    let mut out = Vec::new();
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return out };
        let mut i = 0u32;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let adapter: IDXGIAdapter1 = adapter;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            if (desc.Flags as i32 & DXGI_ADAPTER_FLAG_SOFTWARE.0) != 0 {
                continue;
            }
            let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
            let nome = String::from_utf16_lossy(&desc.Description[..len]);
            if desc.VendorId == 0x1414 {
                // "Microsoft Basic Render Driver"
                continue;
            }
            let mut livre = None;
            if let Ok(a3) = adapter.cast::<IDXGIAdapter3>() {
                let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                if a3.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info).is_ok() {
                    livre = Some(info.Budget.saturating_sub(info.CurrentUsage) / (1024 * 1024));
                }
            }
            let driver = adapter.CheckInterfaceSupport(&IDXGIDevice::IID).ok().map(|v| {
                let v = v as u64;
                format!("{}.{}.{}.{}", (v >> 48) & 0xffff, (v >> 32) & 0xffff, (v >> 16) & 0xffff, v & 0xffff)
            });
            out.push(DxgiAdapter {
                nome,
                vendor: desc.VendorId as u16,
                device: desc.DeviceId as u16,
                vram_total_mb: (desc.DedicatedVideoMemory as u64) / (1024 * 1024),
                vram_livre_mb: livre,
                driver,
            });
        }
    }
    out
}

#[cfg(not(windows))]
pub fn enumerate() -> Vec<DxgiAdapter> {
    Vec::new()
}
