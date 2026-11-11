# ESTADO do projeto Ignis

> Toda retomada começa por aqui. Atualizado antes de cada pausa.

## Situação
**Em construção** — manifestos prontos; núcleo (`crates/ignis-core`) sendo escrito.

### Progresso do núcleo
- [x] `consts`, `paths`, `config` (configurações com testes)
- [x] `gguf` (leitor de metadados, com testes)
- [x] `distro` (Fedora, Fedora Atomic, Debian/Ubuntu, Arch, openSUSE; Flatpak/AppImage)
- [x] `gpu/pci`, `gpu/parse` (vulkaninfo, rocminfo, nvidia-smi, --list-devices), `gpu/sysfs`, `gpu/windows` (DXGI)
- [ ] `gpu/mod` (juntar as fontes) + fixtures das placas
- [ ] motores (candidatos, instalação), teste de velocidade, orçamento de memória
- [ ] modelos (catálogo, download, Ollama), supervisor do llama-server, API

## Próxima ação exata
Escrever `gpu/mod.rs` (junta sysfs/DXGI/vulkaninfo/rocminfo/nvidia-smi) e as fixtures das placas.

## Decisões tomadas
| Assunto | Decisão | Motivo |
|---|---|---|
| Nome | **Ignis** (nome do repositório; "fogo" em latim) | O briefing permitia criar; centralizado em `ignis-core/src/consts.rs`, `app/src-tauri/tauri.conf.json` e nomes de pacote para renomear fácil |
| Versão do llama.cpp | **b12271** (commit 3e86ce2c2), fixada em `manifest/motores.json` com SHA-256 de cada arquivo | Último release com todos os pacotes publicados no dia da escolha (11/11/2026) |
| ROCm no Linux | Usa o pacote oficial `ubuntu-rocm-10.0` do llama.cpp, **mas** as bibliotecas ROCm vêm do sistema (`libamdhip64.so.7`, `librocblas.so.5`, `libhipblas.so.3`) | O pacote do llama.cpp não traz o runtime; no Fedora 44 isso vem de `dnf install rocm-hip hipblas rocblas` (repositórios oficiais), como pede o briefing |
| ROCm no Windows | Candidato só se o alvo gfx estiver na lista oficial da AMD **e** nos alvos compilados pelo llama.cpp | O pacote Windows do llama.cpp traz `amdhip64_7.dll`; placas fora da lista (ex.: RX 580/gfx803) vão para Vulkan |
| CUDA | 13.5 se driver ≥ 580; senão 12.x (12.8 Linux / 12.4 Windows) | Compatibilidade de driver da NVIDIA |
| Linux "para todos" | Pacotes: RPM (Fedora 44), **Flatpak** (Fedora Atomic e qualquer distro com Flatpak), AppImage e .deb | Pedido do criador: funcionar em praticamente todo Linux, incluindo Fedora Atomic (imutável, onde RPM exige rpm-ostree + reiniciar) |
| Dicas por distro | O Ignis detecta a família (Fedora, Atomic, Debian/Ubuntu, Arch, openSUSE) e mostra o comando certo (`dnf`, `rpm-ostree`, `apt`, `pacman`, `zypper`) | Uma mensagem só para todos não serve no Atomic |
| ROCm no Flatpak | Indisponível (as bibliotecas ROCm do sistema não ficam visíveis no sandbox); usa Vulkan e explica | Limitação do Flatpak |
| Catálogo | 11 modelos fixados por revisão do Hugging Face + SHA-256 (`manifest/catalogo.json`) | Curto e escolhido a dedo; todos sem login (não "gated") |

## Como testar
(será preenchido conforme os módulos ficarem prontos)
