# Avisos de terceiros

O Ignis é distribuído sob a licença MIT (veja `LICENSE`). Ele usa peças prontas
de outros projetos, listadas abaixo com suas licenças.

## Motores: llama.cpp (MIT)
- Projeto: https://github.com/ggml-org/llama.cpp — licença **MIT**
  (Copyright (c) 2023-2026 The ggml authors).
- O Ignis **não leva** os motores no instalador: baixa por ação da pessoa só o
  pacote oficial que serve para a placa dela (GitHub Releases, versão fixada
  `b12271`, cada arquivo conferido por SHA-256 em `manifest/motores.json`).
- O pacote oficial traz o arquivo `LICENSE` do llama.cpp, que fica na pasta do
  motor instalado.
- Pacotes de GPU podem trazer bibliotecas de terceiros com licenças próprias:
  - CUDA: `cudart`, `cuBLAS` (NVIDIA, licença do CUDA Toolkit, redistribuíveis);
  - ROCm/HIP no Windows: `amdhip64_7.dll`, `amd_comgr.dll`, `rocm_kpack.dll`
    (AMD, licenças do ROCm);
  - processador: `libomp.dll` (LLVM OpenMP, Apache-2.0 com exceção LLVM).
- No Linux, o ROCm vem dos repositórios oficiais da distribuição (ex.: Fedora:
  `rocm-hip`, `hipblas`, `rocblas`), instalados pela pessoa.

## Voz e transcrição (desde a 0.3.0)
- **omnivoice.cpp** (https://github.com/ServeurpersoCom/omnivoice.cpp, MIT) e
  **whisper.cpp** (https://github.com/ggml-org/whisper.cpp, MIT), compilados pelo
  CI do Ignis a partir de commits fixados e publicados na release
  `motores-audio-1`, com SHA-256 em `manifest/motores_audio.json`. Baixados só
  quando a pessoa prepara a voz ou a transcrição.
- Pesos da voz: **OmniVoice** (`Serveurperso/OmniVoice-GGUF`) — licença
  **CC BY-NC 4.0, uso não comercial**. O Ignis mostra isso antes de baixar.
- Pesos da transcrição: **Whisper** (`ggerganov/whisper.cpp`, modelos da OpenAI) — **MIT**.

## Modelos (cada um com sua licença)
O Ignis não leva modelos. Ao baixar, a janela mostra a licença de cada um (com
link). Catálogo atual (`manifest/catalogo.json`):

| Modelo | Licença |
|---|---|
| Qwen3 0.6B, 1.7B, 4B, 8B, 14B; Qwen3 Embedding 0.6B; Qwen2.5 Coder 1.5B e 7B | Apache 2.0 (Alibaba Cloud / Qwen) |
| Gemma 3 4B | Termos de uso do Gemma (Google) — https://ai.google.dev/gemma/terms |
| Phi-4 mini | MIT (Microsoft) |
| gpt-oss 20B | Apache 2.0 (OpenAI) |

Modelos importados do Ollama ou de arquivos da pessoa seguem a licença original
deles; o Ignis só lê os arquivos.

## Aplicativo e bibliotecas
- **Tauri 2** (janela, bandeja) — MIT ou Apache-2.0.
- **WebKitGTK** (Linux, janela) — LGPL-2.1 / BSD; usado como biblioteca do
  sistema (RPM, .deb, Flatpak) ou embutido como biblioteca dinâmica (AppImage).
- **Microsoft Edge WebView2** (Windows, janela) — componente do Windows.
- **libayatana-appindicator**, **libayatana-indicator**, **ayatana-ido**,
  **libdbusmenu** (bandeja no Linux) — LGPL-2.1/LGPL-3.0/GPL-3.0 conforme o
  componente; vêm do sistema (RPM/.deb) ou são compiladas dentro do Flatpak a
  partir dos módulos do Flathub (`packaging/flatpak/shared-modules`, copiados de
  https://github.com/flathub/shared-modules, com ajuste de `CMAKE_INSTALL_LIBDIR`).
- **Microsoft Visual C++ Runtime** (`msvcp140.dll`, `vcruntime140.dll`,
  `vcruntime140_1.dll`) — arquivos redistribuíveis do Visual Studio, incluídos no
  instalador do Windows e copiados para a pasta dos motores (o llama-server
  oficial do Windows precisa deles).
- **Lista de autoridades certificadoras da Mozilla** (via `webpki-root-certs`,
  CDLA-Permissive-2.0) — usada só quando os certificados do sistema não podem
  ser lidos.
- **Bibliotecas Rust**: 538 pacotes, quase todos MIT e/ou Apache-2.0; alguns
  MPL-2.0 (`cssparser`, `selectors`, `dtoa-short`, `option-ext`, usados sem
  modificação), BSD, ISC, Zlib, Unicode-3.0. Lista completa com versões:
  `AVISOS-DEPENDENCIAS.txt` (regerar com `scripts/listar_licencas.py`).

## Fonte
**Atkinson Hyperlegible Next** (Braille Institute / The Atkinson Hyperlegible Next
Project Authors), licença **SIL Open Font License 1.1** — embutida na janela
(`app/ui/fontes/`, com o texto da licença em `OFL.txt`) para a letra ficar
igual no Windows e no Linux.

## Ícone
O ícone de chama do Ignis foi desenhado para o projeto (licença MIT, como o resto).

## Privacidade
Sem telemetria. As únicas conexões externas são:
- GitHub Releases (`github.com`), para baixar motores, a lista de versões e a versão nova;
- Hugging Face (`huggingface.co`), para baixar modelos.

Downloads só por ação da pessoa. A única conexão sozinha é ler, uma vez por
dia, a lista pública de versões (`atualizacoes.json`) para avisar de versão
nova; nada sobre a pessoa é enviado e dá para desligar em Motor → Atualizações.
A versão nova só é instalada com o clique da pessoa.

**Flatpak:** para atualizar com um clique, o Flatpak do Ignis tem permissão de
pedir ao Flatpak do sistema que instale a versão nova
(`--talk-name=org.freedesktop.Flatpak`). Essa permissão deixa o Ignis rodar
comandos fora da caixa de areia do Flatpak; ele só usa para instalar o arquivo
da versão oficial, depois de conferir o SHA-256.
