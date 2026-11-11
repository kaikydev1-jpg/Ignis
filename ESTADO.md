# ESTADO do projeto Ignis

> Toda retomada começa por aqui. Atualizado antes de cada pausa.

## Situação
**Em construção** — manifestos prontos; núcleo (`crates/ignis-core`) sendo escrito.

### Progresso
- [x] Núcleo `crates/ignis-core`: configurações, GGUF, distro (incl. Fedora Atomic/Flatpak), detecção de placa, candidatos de motor, instalação com SHA-256, orçamento de memória, catálogo, registro de modelos, Ollama, supervisor do llama-server, teste de velocidade, orquestrador, API local, autostart, atualizações
- [x] Placa simulada (`crates/fake-llama-server`) + 19 testes de ponta a ponta (escolha, queda, streaming, Parar, descarregar por tempo, pausa, embeddings, downloads, Ollama, proteção da API)
- [x] Integração com o llama-server **oficial** b12271 (processador) + Qwen2.5 0.5B: instalação real, conversa, streaming, Parar, descarregar por tempo, teste de velocidade
- [ ] `ignis-cli` (ferramenta de linha de comando para diagnóstico)
- [ ] Aplicativo Tauri (bandeja, janela, UI)
- [ ] CI + instaladores (NSIS, RPM, Flatpak, AppImage, .deb)
- [ ] TESTE_DO_CRIADOR.md, CONTRATO_VALKODE.md, AVISOS.md, README.md

## Próxima ação exata
Criar `crates/ignis-cli` e depois o aplicativo Tauri em `app/`.

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
```bash
# tudo que não precisa de placa nem internet
cargo test --workspace

# integração com o llama-server oficial (baixe antes o pacote CPU e um modelo pequeno)
IGNIS_TEST_ENGINE_ARCHIVE=llama-b12271-bin-ubuntu-x64.tar.gz \
IGNIS_TEST_MODEL=qwen2.5-0.5b-instruct-q4_k_m.gguf \
cargo test -p fake-llama-server --test integracao_real -- --nocapture
```
Resultado medido no container de desenvolvimento (4 núcleos, sem placa): resposta "Olá!" em 1,0 s; 44 tokens/s gerando; Parar em 6 ms.
