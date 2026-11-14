# ESTADO do projeto Ignis

> Toda retomada começa por aqui. Atualizado antes de cada pausa.

## Situação
**Versão oficial 0.3.0** (GitHub Releases, sem login e sem prazo; as anteriores continuam lá):
https://github.com/kaikydev1-jpg/Ignis/releases/latest, com arquivos por sistema
(`Ignis-Windows.exe`, `Ignis-Fedora.rpm`, `Ignis-Flatpak.flatpak`, `Ignis-Ubuntu-Debian.deb`,
`Ignis-Linux.AppImage`, `Ignis-Matsu-OS.flatpak`, `instalar.sh`, `SHA256SUMS.txt`, `atualizacoes.json`).
Novo na 0.3.0: **voz** (OmniVoice, com clonagem) e **transcrição** (Whisper) na placa pelo Vulkan
ou no processador (motores na release `motores-audio-1`), e **atualizar com um clique** em todos os
tipos de instalação, com aviso diário de versão nova.
Instalador pelo terminal (`scripts/instalar.sh`): descobre o sistema, baixa a versão certa, confere o SHA-256 e instala.
Versões de teste continuam separadas: Actions → **Instaladores** (90 dias, pede login).
Próximo passo: o criador instala a 0.3.0, roda o TESTE_DO_CRIADOR.md (seções 10 e 11 são as novas) e passa o
`docs/IGNIS_PARA_AZURIA.md` para o chat do Azúria Voice.

## O que está pronto
- **Núcleo** (`crates/ignis-core`):
  - detecção de placa: sysfs/KFD, DXGI, vulkaninfo, rocminfo, nvidia-smi, módulo NVIDIA e `llama-server --list-devices`;
  - candidatos de motor (ROCm/HIP, CUDA, Vulkan, processador, llama-server próprio);
  - instalação dos motores oficiais com SHA-256 e extração segura;
  - teste de velocidade com escolha do mais rápido;
  - queda automática para o próximo motor (na subida e quando o motor cai);
  - orçamento de memória (camadas na placa e contexto pela VRAM livre);
  - catálogo curado filtrado pela placa;
  - downloads com cancelar, retomar, hash e remover;
  - importação do Ollama ("usar daqui" ou "copiar", nunca apaga nada);
  - descarregar por tempo parado e pausar/retomar;
  - API local compatível com a OpenAI (streaming, Parar, embeddings) e rotas `/motor/*`, só em 127.0.0.1, com proteção de Host/Origin;
  - abrir com o computador (Run do Windows, `.desktop` no Linux, AppImage e Flatpak);
  - atualizações separadas (app, motores, catálogo); o app **atualiza com um clique** (`atualizador.rs`): descobre o tipo de instalação (Windows/NSIS, RPM, deb, AppImage, Flatpak, Matsu OS, Fedora Atomic com RPM em camada), baixa só o arquivo dele da versão oficial, confere pelo `SHA256SUMS.txt` e a bandeja fecha, instala e reabre (Fedora Atomic: `rpm-ostree`, entra ao reiniciar); procura versão nova uma vez por dia (pode desligar), com notificação do sistema e item no menu do ícone;
  - **voz e transcrição** (`audio.rs`, `service_audio.rs`): omnivoice.cpp (`tts-server`) e whisper.cpp (`whisper-server`) compilados no CI (`motores-audio.yml`) em Vulkan e processador, modelos do Hugging Face com revisão e SHA-256 fixados, quantização pela memória de vídeo, sobe sob demanda, cai para o processador se a placa falhar, descarrega parado; API da OpenAI `/v1/audio/*` e vozes clonadas guardadas;
  - dicas por distro (dnf, rpm-ostree, apt, pacman, zypper).
- **Aplicativo** (`app/`, Tauri 2):
  - ícone na bandeja com menu Abrir / Modelo / Pausar-Retomar / Descarregar modelo / Procurar atualizações (vira "Atualizar para a versão X") / Sair;
  - janela clássica em português (Placa, Modelos, Voz, Motor, Status), clara ou escura conforme o sistema, com faixa "versão nova disponível — Atualizar agora";
  - janela em **processo separado** (núcleo leve em repouso);
  - instância única.
- **Qualquer modelo do Hugging Face**:
  - busca dentro do app, com as versões de cada repositório, tamanho e se cabe (lendo o cabeçalho pela internet, sem baixar tudo);
  - modelos em várias partes;
  - visão/áudio via `--mmproj`;
  - **conta da pessoa** (token) para modelos que pedem aceite dos termos: fica só no computador e só vai ao huggingface.co.
- **Janela redesenhada** (igual no Windows e no Linux):
  - fonte embutida (Atkinson Hyperlegible Next);
  - barra de memória da placa mostrando onde o modelo cabe;
  - primeiro uso guiado em 3 passos;
  - velocidade dos motores em barras de "calor";
  - catálogo com filtros e mini-barras de memória;
  - status com velocidade e exemplos prontos (curl/Python/JS);
  - sem menu de botão direito, zoom ou seleção solta de página;
  - **barra de título integrada** (como VS Code/Discord): sem a barra cinza do sistema; o topo do Ignis arrasta a janela, duplo clique maximiza, botões minimizar/maximizar/fechar no jeito de cada sistema (Windows 11; no Linux segue lado, ordem e estilo do sistema: bolinhas tipo Mac, GNOME ou KDE; **Matsu OS** (sistema do criador, Fedora Atomic; o nome traz o ano: Matsu 27 OS, 28…) é reconhecido pelo os-release e usa as cores exatas da referência dele; escolha manual em Motor (sem opção Matsu, que é automática)).
- **Placa simulada** (`crates/fake-llama-server`) e testes de ponta a ponta.
- **`ignis-cli`**: `detectar`, `servir`, `gguf`, `ollama`, `versao`.
- **CI** (`.github/workflows/ci.yml`): fmt, clippy `-D warnings` e testes em Linux e Windows, incluindo o llama-server oficial com modelo de 0,5 B.
- **Instaladores** (`.github/workflows/instaladores.yml`):
  - NSIS por usuário (Windows), RPM gerado dentro de um container Fedora 44, .deb, AppImage e Flatpak;
  - publicar só à mão, e como rascunho de release;
  - `atualizacoes.json` gerado junto.
- **Documentos:** README.md, CONTRATO_VALKODE.md, TESTE_DO_CRIADOR.md, AVISOS.md e AVISOS-DEPENDENCIAS.txt.

## Provas feitas (sem placa de vídeo)
| O quê | Resultado |
|---|---|
| Testes automáticos | 131 passando (unidades, candidatos, detecção com fixtures, placa simulada, downloads/Ollama, Hugging Face com servidor imitando o site, llama-server oficial) |
| CI GitHub Linux **e Windows** | fmt, clippy e todos os testes passaram nos dois, inclusive o llama-server oficial rodando no Windows (resposta "Olá!", 44,8 tokens/s) |
| Instaladores no CI | NSIS (Windows, com as DLLs do Visual C++), RPM (Fedora 44), .deb, AppImage e Flatpak gerados |
| Hugging Face de verdade | busca, lista de versões e "ver se cabe" (lendo o cabeçalho real por Range) funcionaram pela janela |
| Janela nova | conferida em capturas nos temas claro e escuro, no Chromium (motor do WebView2 do Windows) e no WebKitGTK (Linux): visual idêntico |
| llama-server oficial b12271 (CPU) + Qwen2.5 0.5B | instalado e conferido por SHA-256; respondeu "Olá!" em 1,0 s; 44 tokens/s; Parar em 6 ms; descarregar por tempo; teste de velocidade |
| Preparação real (GitHub + Hugging Face) | baixou o motor CPU e o Qwen3 0.6B, conferiu os SHA-256 e testou sozinho (39,6 tokens/s) |
| App real em Xvfb | janela com as 4 seções (capturas de tela conferidas) |
| Bandeja via StatusNotifier (D-Bus) | registrou o ícone com o menu certo; Pausar→Retomar, Descarregar, Abrir e Sair funcionaram (Sair encerra o llama-server) |
| Memória em repouso (build release) | núcleo **≈60 MB RSS** (12 MB próprios), **0,05% de CPU**; ao fechar a janela, o processo dela some (sem WebKit restante) |
| Instância única | abrir de novo traz a janela; não duplica |
| Abrir com o computador | `.desktop` criado (binário, AppImage e Flatpak) |
| Pacotes locais | .deb, .rpm, AppImage (runtime estático: dispensa libfuse2) e Flatpak (GNOME 51) gerados |
| Recursos para apps (llama-server oficial + Qwen3 0.6B) | raciocínio separado em `reasoning_content`; `chat_template_kwargs.enable_thinking=false` desliga o pensamento; `tools` devolve `tool_calls`; pedido de 30 MB aceito (o Ignis agora aceita até 256 MB: fotos/áudio em base64) |
| Barra integrada (app real, Xvfb + openbox) | sem barra do sistema; botões seguindo o sistema simulado (bolinhas à esquerda com layout `close,minimize,maximize:`, GNOME redondos, GNOME de fábrica só com fechar, KDE com setas, escolha manual Windows; Matsu OS simulado por os-release: bolinhas com as cores da referência nos 3 estados, nos temas claro e escuro, mesmo com os botões à direita); arrastar pela faixa de cima e pela marca, duplo clique maximiza e restaura, botão maximizar troca o ícone, redimensionar pelo canto, minimizar, fechar deixa o núcleo rodando; temas claro e escuro |
| Escolha do motor pelo modelo (pedido do Valkode) | placa simulada reproduzindo a RX 580: teste pequeno dá processador, modelo grande que cabe vai para a placa ("o modelo cabe na placa"); medidas de uso real e "Testar velocidade" com o modelo decidem depois; motor escolhido à mão troca no próximo pedido |
| Carga do motor (llama-server oficial, CPU, Qwen3 0.6B) | pelo Ignis 1,9–2,1 s; direto 1,5–1,6 s (o Ignis acrescenta ~0,4 s). A 1ª carga direta, com o arquivo fora da memória, levou 23,5 s: a diferença do relatório (137 s × 76 s) é carga fria/driver, não o Ignis |
| Modelos já no computador | mesmo arquivo do catálogo no cache do Hugging Face achado pelo SHA-256 e usado sem baixar; "Preparar" usa um modelo pequeno do LM Studio para o teste; lista Ollama + Hugging Face + LM Studio |
| Voz real no CI (omnivoice.cpp, processador) | Linux e Windows: gerou fala em português ("Olá! Este é um teste do motor de voz do Ignis.", ~134 KB de WAV, 26–43 s no processador do CI) |
| Transcrição real no CI (whisper.cpp, processador) | Linux: transcreveu exatamente "Olá! Este é um teste do motor de voz do Ignis." a partir da fala gerada |
| Voz e transcrição pelo Ignis de verdade (Linux, processador, motores publicados em `motores-audio-1`) | `POST /motor/audio/preparar` baixou e conferiu motores e modelos (OmniVoice Q4_K_M + Whisper small) em 50 s; a fala "Olá! Eu sou o Ignis…" saiu em 60 s e voltou transcrita igual em 8 s; voz clonada registrada, usada e mantida depois de reiniciar o Ignis ("Agora eu falo com uma voz clonada."); tradução para inglês ("Now I speak with a cloned voice.") |
| Transcrição real no CI (Windows) | transcreveu a fala gerada ("olá, este é um teste de um tor de voz do ignis", com o modelo base, menor que o usado pelo Ignis) |
| Motores de áudio Vulkan sem placa (lavapipe) | sobem, não acham placa e seguem no processador gerando áudio |
| Voz/transcrição pelo Ignis (servidores simulados) | falar com voz clonada, transcrever, traduzir, `/v1/models`, teste da janela, voz recusada não é guardada, apagar voz; placa falha → processador; sem preparar → 503 claro; descarrega parado |
| Atualizar com um clique (versão oficial simulada) | soma errada na lista → recusa e não mexe em nada; soma certa → troca o AppImage e pede para fechar e reabrir com o comando certo |
| Flatpak de verdade | instalou, rodou, leu a distro real do sistema (`/run/host`), gravou o autostart com `flatpak run` e rodou o llama-server oficial dentro do sandbox (41 tokens/s) |

## O que só a placa real prova
Está em **TESTE_DO_CRIADOR.md**:
- RX 580 com Vulkan; AMD com ROCm (Fedora e Windows); NVIDIA com CUDA;
- bandeja no Windows e no Fedora (GNOME + extensão AppIndicator, KDE);
- abrir com o computador depois de reiniciar;
- importar do Ollama real;
- ligar no Valkode;
- capturar saídas reais de vulkaninfo/rocminfo/nvidia-smi para trocar as fixtures;
- **voz e transcrição na placa de verdade** (RX 580 e outras pelo Vulkan): o CI só tem processador;
- **atualizar com um clique** em cada sistema de verdade (Windows, Fedora, Ubuntu, AppImage, Flatpak, Matsu OS, Fedora Atomic): o primeiro teste real é da 0.3.0 para a próxima versão.

## Próxima ação exata
1. O criador instala a 0.3.0, roda o TESTE_DO_CRIADOR.md (seções 10, voz e transcrição, e 11, atualização) e manda o resultado (e o "Copiar relatório" onde algo der diferente).
2. O criador passa o `docs/IGNIS_PARA_AZURIA.md` (seção 7 tem o texto pronto) para o chat do Azúria Voice.
3. Trocar as fixtures de detecção pelas saídas reais capturadas (vulkaninfo/rocminfo/nvidia-smi).
4. Nova versão: subir `version` nos Cargo.toml/tauri.conf.json, trocar `.github/release/resumo.txt` (uma linha, aparece na faixa de versão nova) e as notas, e rodar o workflow **Instaladores** à mão com "publicar = sim". Quem tem a 0.3.0 recebe o aviso e atualiza com um clique.
5. Motores de áudio novos: rodar **Motores de áudio** com "publicar = sim" numa release nova (`motores-audio-N`) e atualizar `manifest/motores_audio.json` com os SHA-256 do `SHA256SUMS.txt`.

## Pendente com o criador
- **Azúria Voice:** o chat dele vai ligar o Ignis (LLM, voz `ignis-tts`, transcrição) junto com a mudança de visual. Guia: `docs/IGNIS_PARA_AZURIA.md`.
- **Valkode:** já tem o contrato (CONTRATO_VALKODE.md) e a resposta ao pedido do teste do motor.
- **Música e efeitos sonoros** (ACE-Step, MusicGen, Stable Audio, AudioLDM2): não há versão GGML desses modelos; continuam no PyTorch do Azúria.
- **Voz CUDA/ROCm:** a 0.3.0 usa Vulkan (AMD, NVIDIA e Intel) e processador. Versões CUDA e ROCm do omnivoice.cpp ficam para depois, se o Vulkan não bastar na NVIDIA.

## Decisões tomadas
| Assunto | Decisão | Motivo |
|---|---|---|
| Nome | **Ignis** (nome do repositório; "fogo" em latim) | O briefing permitia criar um. Fica em `ignis-core/src/consts.rs`, `app/src-tauri/tauri.conf.json` e nos nomes de pacote, para trocar fácil |
| Identificador | `io.github.kaikydev1jpg.Ignis` | Sem hífen nem sublinhado: vale para Tauri, Flatpak e D-Bus |
| Versão do llama.cpp | **b12271** (commit 3e86ce2c2), fixada em `manifest/motores.json` com SHA-256 de cada arquivo | Último release com todos os pacotes publicados no dia (11/11/2026) |
| ROCm no Linux | Pacote oficial `ubuntu-rocm-10.0` do llama.cpp + bibliotecas ROCm do sistema (`libamdhip64.so.7`, `librocblas.so.5`, `libhipblas.so.3`) | O pacote não traz o runtime; no Fedora ele vem dos repositórios oficiais, como pede o briefing |
| ROCm no Windows | Candidato só se o alvo gfx estiver na lista oficial da AMD **e** nos alvos compilados | Placas fora da lista (ex.: RX 580, gfx803) usam Vulkan |
| CUDA | 13.5 com driver ≥ 580; senão 12.x (12.8 no Linux, 12.4 no Windows) | Compatibilidade de driver |
| Linux "para todos" | RPM (Fedora 44), **Flatpak** (Fedora Atomic e qualquer distro), AppImage e .deb | Pedido do criador: funcionar em quase todo Linux, incluindo o Fedora Atomic |
| Dicas por distro | Detecta Fedora, Atomic, Debian/Ubuntu, Arch e openSUSE e mostra o comando certo | No Atomic, `dnf` não serve |
| ROCm no Flatpak | Indisponível (as bibliotecas do sistema não ficam visíveis); usa Vulkan e explica | Limitação do Flatpak |
| Runtime do Flatpak | GNOME 51 (traz WebKitGTK 4.1 e Vulkan); bandeja via módulo libayatana-appindicator do Flathub | Verificado instalando o runtime |
| Janela | **Processo separado** (`ignis --janela`), que fala com o núcleo por um canal local com senha | O WebView deixava ~190 MB residentes mesmo com a janela fechada; separado, o núcleo fica em ≈60 MB, e a janela não derruba o motor |
| Fechar a janela | Encerra o processo da janela; o Ignis fica na bandeja | Na prática é igual a "minimizar para a bandeja", gastando menos memória |
| Velocidade | Mostrada em **tokens/s**, explicando que 1 token ≈ ¾ de palavra | O llama.cpp mede tokens; chamar de "palavras" não seria honesto |
| Escolha do mais rápido | Velocidade de geração; empate (até 5%) desempata pela leitura | É o que a pessoa sente numa conversa |
| Teste de velocidade | 30 s de **medição** por motor; para subir o motor, até 2 min | Na primeira vez o Vulkan/Mesa compila sombreadores; isso não pode reprovar o motor |
| Teste automático | Roda sozinho quando a placa/driver mudar **e** já houver motores e modelo de teste baixados | Sem baixar nada sem ação da pessoa |
| Preparação | Botão "Preparar": baixa só os motores disponíveis para a placa + Qwen3 0.6B e testa | Conexões externas só por ação da pessoa |
| Pausar | Descarrega o modelo e recusa pedidos (503 `ignis_pausado`) até retomar | Libera a placa para jogos |
| Parar | Fechar a conexão **ou** `POST /motor/parar` com o `X-Ignis-Requisicao` | Serve clientes que não conseguem fechar a conexão |
| Atualizações | Verificação manual; a semanal é opcional e vem desligada | Privacidade: sem conexão sem ação da pessoa |
| Atualizar o app | Avisa e abre a página de download (sem autoatualização) | Autoatualização exigiria chaves de assinatura; fica para depois |
| Windows e Visual C++ | O instalador leva `msvcp140`/`vcruntime140(_1).dll` e o Ignis copia para cada motor | O llama-server oficial precisa dessas DLLs; evita pedir administrador ou baixar o redistribuível |
| Certificados | Se os do sistema falharem, usa a lista da Mozilla embutida | Antes travava (achado no teste do Flatpak) |
| Fixtures de placas | Montadas no formato exato das ferramentas, com IDs reais; **não capturadas** em placa real | Não há placa aqui; o TESTE_DO_CRIADOR pede as saídas reais |
| Catálogo | 11 modelos fixados por revisão do Hugging Face + SHA-256, todos sem login | Curto e escolhido a dedo |
| Visual | Fonte embutida + mesmas cores e componentes nos dois sistemas; destaque visual único na barra de memória da placa; "calor" (âmbar→brasa) só nas barras de velocidade | Pedido do criador: muito mais bonito, fácil e **igual** no Windows e no Linux |
| Escolha do motor | Por modelo (`ignis_core::escolha`): à mão > medido com o modelo (2+ motores) > teste geral se o modelo for do tamanho do de teste (até 2x) > placa se ≥ 50% das camadas couberem > processador | Relatório do Valkode na RX 580: o teste com 0,6B deu processador, mas no 8B a placa é 2x mais rápida |
| Modelos já baixados por outros programas | Achar pelo SHA-256 no nome do arquivo (Ollama `blobs/sha256-…`, Hugging Face `blobs/<hash>`); link físico na pasta do Ignis quando der, senão lê no lugar; nunca apaga nada lá | O criador viu o "Preparar" baixar de novo um modelo que já tinha |
| Barra de título | **Integrada** (`decorations: false` + `shadow: true`; topo do app arrasta, botões próprios; arraste só começa ao mover o mouse para o duplo clique funcionar no Linux) | O criador mostrou que a barra do sistema (cinza, separada) é a "barra esquisita"; quer o topo embutido no app. Os botões seguem o sistema (o Linux dele usa bolinhas tipo Mac): lido de `gtk-decoration-layout` e do tema GTK, regra em `ignis_core::janela` |
| Ollama | Opcional e discreto ("Mais opções"); o Ignis nunca precisa dele | O motor e os modelos são do próprio Ignis |
| Modelos com termos | Conta da pessoa (token do Hugging Face), guardada só no computador (arquivo `hf-token`, permissão 600 no Linux) | Cada pessoa usa a própria conta; o token só vai ao huggingface.co |
| Contexto padrão | 8.192 tokens (reduz sozinho se não couber; mínimo 2.048) | Bom para conversa e código numa placa de 8 GB |

## Como testar
```bash
# tudo que não precisa de placa nem internet (+ integração se as variáveis existirem)
cargo test --workspace
```
```bash
# integração com o llama-server oficial
IGNIS_TEST_ENGINE_ARCHIVE=llama-b12271-bin-ubuntu-x64.tar.gz IGNIS_TEST_MODEL=qwen2.5-0.5b-instruct-q4_k_m.gguf cargo test -p fake-llama-server --test integracao_real -- --nocapture
```
```bash
# app de verdade
cargo run -p ignis-app
```
Pacotes: GitHub Actions → workflow **Instaladores** → artefatos.
