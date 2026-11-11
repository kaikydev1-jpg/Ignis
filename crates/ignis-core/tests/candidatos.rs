//! Escolha dos motores candidatos para cada placa.

mod comum;

use comum::detection_with;
use ignis_core::distro::Familia;
use ignis_core::engine::{build_candidates, Backend, Candidate, EngineManifest};

fn ids(c: &[Candidate]) -> Vec<(String, bool)> {
    c.iter().map(|c| (c.variante.clone(), c.disponivel)).collect()
}

fn get<'a>(c: &'a [Candidate], b: Backend) -> &'a Candidate {
    c.iter().find(|c| c.backend == b).unwrap_or_else(|| panic!("sem {b:?} em {:?}", ids(c)))
}

const VK: (&str, bool) = ("libvulkan.so.1", true);
const ROCM_LIBS: [(&str, bool); 3] = [("libamdhip64.so.7", true), ("librocblas.so.5", true), ("libhipblas.so.3", true)];

#[test]
fn rx580_linux_usa_vulkan_e_nao_rocm() {
    let m = EngineManifest::embedded();
    let d = detection_with("rx580", Familia::Fedora, &[VK, ROCM_LIBS[0], ROCM_LIBS[1], ROCM_LIBS[2]]);
    let c = build_candidates(&d, &m, None);
    assert_eq!(ids(&c), vec![("linux-rocm".into(), false), ("linux-vulkan".into(), true), ("linux-cpu".into(), true)]);
    assert!(get(&c, Backend::Rocm).motivo.contains("gfx803"));
}

#[test]
fn rx6600_fedora_com_rocm_do_sistema() {
    let m = EngineManifest::embedded();
    let mut libs = vec![VK];
    libs.extend(ROCM_LIBS);
    let d = detection_with("rx6600", Familia::Fedora, &libs);
    let c = build_candidates(&d, &m, None);
    assert_eq!(ids(&c), vec![("linux-rocm".into(), true), ("linux-vulkan".into(), true), ("linux-cpu".into(), true)]);
}

#[test]
fn rx6600_fedora_sem_rocm_mostra_dnf() {
    let m = EngineManifest::embedded();
    let d = detection_with("rx6600", Familia::Fedora, &[VK]);
    let c = build_candidates(&d, &m, None);
    let r = get(&c, Backend::Rocm);
    assert!(!r.disponivel);
    assert!(r.dica.as_deref().unwrap().contains("sudo dnf install rocm-hip hipblas rocblas"), "{:?}", r.dica);
}

#[test]
fn rx7600_fedora_atomic_sem_rocm_mostra_rpm_ostree() {
    let m = EngineManifest::embedded();
    let d = detection_with("rx7600", Familia::FedoraAtomic, &[VK]);
    let c = build_candidates(&d, &m, None);
    let r = get(&c, Backend::Rocm);
    assert!(!r.disponivel);
    let dica = r.dica.as_deref().unwrap();
    assert!(dica.contains("rpm-ostree install rocm-hip"), "{dica}");
    assert!(dica.contains("reinicie"));
    assert!(get(&c, Backend::Vulkan).disponivel, "Vulkan vem no Atomic");
}

#[test]
fn flatpak_nao_oferece_rocm() {
    let m = EngineManifest::embedded();
    let mut libs = vec![VK];
    libs.extend(ROCM_LIBS);
    let mut d = detection_with("rx9060xt", Familia::FedoraAtomic, &libs);
    d.sistema.flatpak = true;
    let c = build_candidates(&d, &m, None);
    let r = get(&c, Backend::Rocm);
    assert!(!r.disponivel);
    assert!(r.motivo.contains("Flatpak"));
    assert!(get(&c, Backend::Vulkan).disponivel);
}

#[test]
fn sem_acesso_ao_kfd_pede_grupo_render() {
    let m = EngineManifest::embedded();
    let mut libs = vec![VK];
    libs.extend(ROCM_LIBS);
    let mut d = detection_with("rx6600", Familia::Fedora, &libs);
    d.kfd_acessivel = false;
    let c = build_candidates(&d, &m, None);
    assert!(get(&c, Backend::Rocm).dica.as_deref().unwrap().contains("usermod -aG render,video"));
}

#[test]
fn sem_vulkan_no_ubuntu_mostra_apt() {
    let m = EngineManifest::embedded();
    let d = detection_with("rx580", Familia::Debian, &[]);
    let c = build_candidates(&d, &m, None);
    let v = get(&c, Backend::Vulkan);
    assert!(!v.disponivel);
    assert!(v.dica.as_deref().unwrap().contains("sudo apt install libvulkan1 mesa-vulkan-drivers"));
}

#[test]
fn rx9060xt_windows_tem_rocm_oficial() {
    let m = EngineManifest::embedded();
    let d = detection_with("rx9060xt-windows", Familia::Windows, &[("vulkan-1.dll", true)]);
    let c = build_candidates(&d, &m, None);
    assert_eq!(ids(&c), vec![("windows-rocm".into(), true), ("windows-vulkan".into(), true), ("windows-cpu".into(), true)]);
}

#[test]
fn rx580_windows_cai_para_vulkan() {
    let m = EngineManifest::embedded();
    let d = detection_with("rx580-windows", Familia::Windows, &[("vulkan-1.dll", true)]);
    let c = build_candidates(&d, &m, None);
    assert!(!get(&c, Backend::Rocm).disponivel);
    assert!(get(&c, Backend::Vulkan).disponivel);
}

#[test]
fn nvidia_driver_novo_usa_cuda_13() {
    let m = EngineManifest::embedded();
    let d = detection_with("rtx3060", Familia::Fedora, &[VK, ("libcuda.so.1", true)]);
    let c = build_candidates(&d, &m, None);
    assert_eq!(ids(&c), vec![("linux-cuda-13".into(), true), ("linux-vulkan".into(), true), ("linux-cpu".into(), true)]);
}

#[test]
fn nvidia_driver_570_usa_cuda_12() {
    let m = EngineManifest::embedded();
    let mut d = detection_with("rtx3060", Familia::Fedora, &[VK, ("libcuda.so.1", true)]);
    d.placas[0].versao_driver = Some("570.86.16".into());
    let c = build_candidates(&d, &m, None);
    assert_eq!(get(&c, Backend::Cuda).variante, "linux-cuda-12");
    assert!(get(&c, Backend::Cuda).disponivel);
}

#[test]
fn nvidia_driver_antigo_fica_indisponivel() {
    let m = EngineManifest::embedded();
    let mut d = detection_with("rtx3060", Familia::Fedora, &[VK, ("libcuda.so.1", true)]);
    d.placas[0].versao_driver = Some("470.256.02".into());
    let c = build_candidates(&d, &m, None);
    let cu = get(&c, Backend::Cuda);
    assert!(!cu.disponivel);
    assert!(cu.motivo.contains("antigo"));
}

#[test]
fn sem_placa_so_processador() {
    let m = EngineManifest::embedded();
    let d = detection_with("sem-placa", Familia::Fedora, &[VK]);
    let c = build_candidates(&d, &m, None);
    assert_eq!(ids(&c), vec![("linux-cpu".into(), true)]);
}

#[test]
fn llama_server_proprio_entra_como_candidato() {
    let m = EngineManifest::embedded();
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("llama-server");
    std::fs::write(&exe, b"x").unwrap();
    let d = detection_with("sem-placa", Familia::Fedora, &[]);
    let c = build_candidates(&d, &m, Some(&exe));
    assert_eq!(ids(&c), vec![("proprio".into(), true), ("linux-cpu".into(), true)]);
}
