# ESTADO do projeto Ignis

> Toda retomada começa por aqui. Atualizado antes de cada pausa.

## Situação
**Construção completa; aguardando o CI do último envio** (veja "Próxima ação").
Depois disso: **pronto para testar** na placa real (TESTE_DO_CRIADOR.md).

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
  - atualizações separadas (app, motores, catálogo);
  - dicas por distro (dnf, rpm-ostree, apt, pacman, zypper).
- **Aplicativo** (`app/`, Tauri 2):
  - ícone na bandeja com menu Abrir / Modelo / Pausar-Retomar / Descarregar modelo / Sair;
  - janela clássica em português (Placa, Modelos, Motor, Status), clara ou escura conforme o sistema;
  - janela em **processo separado** (núcleo leve em repouso);
  - instância única.
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
| Testes automáticos | 97 passando (unidades, candidatos, detecção com fixtures, placa simulada, downloads/Ollama, llama-server oficial) |
| CI GitHub Linux | fmt, clippy e todos os testes passaram (inclusive o llama-server oficial) |
| llama-server oficial b12271 (CPU) + Qwen2.5 0.5B | instalado e conferido por SHA-256; respondeu "Olá!" em 1,0 s; 44 tokens/s; Parar em 6 ms; descarregar por tempo; teste de velocidade |
| Preparação real (GitHub + Hugging Face) | baixou o motor CPU e o Qwen3 0.6B, conferiu os SHA-256 e testou sozinho (39,6 tokens/s) |
| App real em Xvfb | janela com as 4 seções (capturas de tela conferidas) |
| Bandeja via StatusNotifier (D-Bus) | registrou o ícone com o menu certo; Pausar→Retomar, Descarregar, Abrir e Sair funcionaram (Sair encerra o llama-server) |
| Memória em repouso (build release) | núcleo **≈60 MB RSS** (12 MB próprios), **0,05% de CPU**; ao fechar a janela, o processo dela some (sem WebKit restante) |
| Instância única | abrir de novo traz a janela; não duplica |
| Abrir com o computador | `.desktop` criado (binário, AppImage e Flatpak) |
| Pacotes locais | .deb, .rpm, AppImage (runtime estático: dispensa libfuse2) e Flatpak (GNOME 51) gerados |
| Flatpak de verdade | instalou, rodou, leu a distro real do sistema (`/run/host`), gravou o autostart com `flatpak run` e rodou o llama-server oficial dentro do sandbox (41 tokens/s) |

## O que só a placa real prova
Está em **TESTE_DO_CRIADOR.md**:
- RX 580 com Vulkan; AMD com ROCm (Fedora e Windows); NVIDIA com CUDA;
- bandeja no Windows e no Fedora (GNOME + extensão AppIndicator, KDE);
- abrir com o computador depois de reiniciar;
- importar do Ollama real;
- ligar no Valkode;
- capturar saídas reais de vulkaninfo/rocminfo/nvidia-smi para trocar as fixtures.

## Próxima ação exata
1. Conferir o resultado do CI e dos Instaladores no GitHub Actions para o último commit. Se algo falhar no Windows, corrigir pelos logs.
2. Quando tudo estiver verde, marcar "pronto para testar" aqui e avisar o criador.

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
