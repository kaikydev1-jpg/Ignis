# Teste do criador

Coisas que **só uma máquina de verdade prova** (placa de vídeo real, bandeja
real, reiniciar o computador). Tudo o que dava para testar sem placa já foi
testado (veja ESTADO.md). Cada passo diz o que fazer e o que deve acontecer.

Se algo der diferente: na janela do Ignis, seção **Placa → Copiar relatório**,
e cole o relatório junto com o que aconteceu.

---

## 0. Instalar
Baixe os instaladores no GitHub Actions (workflow **Instaladores**, artefatos) ou
no rascunho de release.

| Sistema | Como instalar | Deve acontecer |
|---|---|---|
| Windows 10/11 | Rodar `Ignis_0.1.0_x64-setup.exe` | **Não** pede administrador. Ao terminar, o Ignis abre. |
| Fedora 44 Workstation | `sudo dnf install ./Ignis-0.1.0-1.x86_64.rpm` | Instala puxando `webkit2gtk4.1` e `libayatana-appindicator-gtk3`. |
| Fedora Atomic (Silverblue/Kinoite/Bazzite) | `flatpak install --user ./Ignis.flatpak` (recomendado) | Instala sem reiniciar. Abra pelo menu de aplicativos. |
| Fedora Atomic, alternativa | `sudo rpm-ostree install ./Ignis-0.1.0-1.x86_64.rpm` e reiniciar | Funciona como no Fedora normal (e permite ROCm). |
| Outras distros | `chmod +x Ignis_0.1.0_amd64.AppImage` e abrir | Abre sem instalar nada (não precisa de libfuse2). |

**GNOME (Fedora Workstation/Silverblue):** o ícone perto do relógio precisa da
extensão AppIndicator.
- Fedora: `sudo dnf install gnome-shell-extension-appindicator`, sair e entrar
  de novo na sessão, e ligar em "Extensões".
- Atomic: instale a extensão pelo app "Gerenciador de Extensões" (Flathub) ou
  `sudo rpm-ostree install gnome-shell-extension-appindicator` e reinicie.
- KDE (Kinoite, Bazzite KDE) já mostra o ícone sem extensão.

## 1. Ícone na bandeja e janela (Windows e Fedora)
- A barra de título deve ser a **nativa do sistema** (a mesma de qualquer outro aplicativo do Windows ou do GNOME/KDE), clara ou escura conforme o sistema.
- O visual da janela deve ser **igual** nos dois sistemas: mesma fonte, cores e layout.

1. Abra o Ignis.
   - **Esperado:** ícone de chama perto do relógio e a janela aberta.
2. Feche a janela no X.
   - **Esperado:** a janela some e o ícone continua lá.
3. Clique com o botão direito no ícone.
   - **Esperado:** menu com **Abrir Ignis**, **Modelo: nenhum** (cinza), **Pausar**, **Descarregar modelo** (cinza) e **Sair**.
4. Clique em **Abrir Ignis**.
   - **Esperado:** a janela volta.
5. Abra o Ignis de novo pelo menu Iniciar ou de aplicativos.
   - **Esperado:** não abre um segundo Ignis; só traz a janela para a frente.
6. Gerenciador de Tarefas (Windows) ou `ps -o rss,cmd -C ignis` (Linux), **com a janela fechada e sem modelo**.
   - **Esperado:** o Ignis usa menos de 100 MB (≈60 MB no teste em Linux).

## 2. Preparar e testar a velocidade
1. Na janela, clique em **Preparar agora**.
   - **Esperado:** baixa só os motores da sua placa e o Qwen3 0.6B (610 MB), depois "Testando o motor…" e um aviso "Teste concluído: o motor X foi o mais rápido".
2. Veja a seção **Placa**.
   - **Esperado:** nome da placa, memória de vídeo, driver e tabela com tokens/s de cada motor.

### 2a. RX 580 (Vulkan)
- **Esperado em "Motores para esta máquina":**
  - ROCm/HIP **indisponível** ("Sua placa (gfx803) não é suportada pelo ROCm");
  - Vulkan **disponível**;
  - Processador disponível.
- **Esperado:** o teste escolhe o Vulkan, bem mais rápido que o processador.
- Fedora sem Vulkan: aparece a dica `sudo dnf install vulkan-loader mesa-vulkan-drivers`.

### 2b. AMD com ROCm (RX 6600 / 7600 / 9060 XT)
- **Fedora 44:** instale o ROCm do Fedora com `sudo dnf install rocm-hip hipblas rocblas`, depois `sudo usermod -aG render,video $USER`, saia e entre de novo na sessão.
  - **Esperado:** ROCm/HIP **disponível**. O teste mede ROCm e Vulkan e escolhe o mais rápido (as duas velocidades aparecem na tabela).
- **Windows** (RX 6600/7600/9060 XT, driver Adrenalin recente):
  - **Esperado:** ROCm/HIP disponível (placas da lista oficial). O teste compara com o Vulkan.
- Se o ROCm falhar, o esperado é ele aparecer com o erro na tabela e o Vulkan ser escolhido.

### 2c. NVIDIA (CUDA)
- **Esperado:** "CUDA (NVIDIA)" disponível (CUDA 13 com driver 580 ou mais novo; CUDA 12 com driver mais antigo) e Vulkan disponível.
- **Esperado:** o teste escolhe o mais rápido, normalmente o CUDA.

## 3. Conversar e medir
1. Seção **Modelos** → **Recomendados**: baixe um modelo marcado "Roda bem na sua placa" (ex.: Qwen3 8B numa placa de 8 GB).
2. No meio do download, clique em **Cancelar** e depois em **Retomar download**.
   - **Esperado:** continua de onde parou e termina com o modelo em "Instalados".
3. Clique em **Carregar**.
   - **Esperado:** em **Status** aparecem "Rodando", o modelo, o motor e "Na placa: todas as camadas".
4. Converse pelo terminal (troque o modelo se baixou outro):
   ```bash
   curl http://127.0.0.1:11470/v1/chat/completions -H 'content-type: application/json' -d '{"model":"qwen3-8b","messages":[{"role":"user","content":"Explique o que é uma GPU em 2 frases. /no_think"}]}'
   ```
   - **Esperado:** a resposta chega, e em **Status** aparece a velocidade (tokens/s).

## 3b. Qualquer modelo do Hugging Face
1. Em **Modelos → Qualquer modelo do Hugging Face**, busque "gemma 3" e clique em **Ver arquivos** em `ggml-org/gemma-3-4b-it-GGUF`.
   - **Esperado:** as versões (Q4_K_M, Q8_0, F16) com tamanho, e o interruptor "Incluir visão/áudio".
2. Clique em **Ver se cabe** no Q4_K_M.
   - **Esperado:** a avaliação muda de "(estimativa)" para o resultado exato.
3. Baixe com "Incluir visão/áudio" ligado e carregue. Mande uma imagem pela API (formato OpenAI, `image_url` em base64).
   - **Esperado:** o modelo descreve a imagem.
4. Modelo que pede aceite: busque "google gemma qat gguf" e tente baixar `google/gemma-3-4b-it-qat-q4_0-gguf` **sem conta**.
   - **Esperado:** a mensagem diz para aceitar os termos e conectar a conta.
5. Aceite os termos no site e crie um token de leitura em huggingface.co/settings/tokens. Cole o token em **Motor → Conta do Hugging Face** e baixe de novo.
   - **Esperado:** "Conectada como <seu usuário>" e o download funciona.

## 4. Queda para o próximo motor
1. Com um modelo carregado numa placa (ROCm, CUDA ou Vulkan), mate o processo `llama-server` (Gerenciador de Tarefas, ou `pkill llama-server`).
   - **Esperado:** em poucos segundos aparece uma linha "O motor X parou de repente (…) Passei para Y." e o modelo volta a funcionar no motor seguinte.
2. Em **Motor → Escolha do motor**, escolha manualmente outro motor.
   - **Esperado:** o próximo pedido usa esse motor.

## 5. Descarregar sozinho e pausar
1. Em **Motor → Configurações**, coloque "Descarregar depois de **1** minuto" e clique em Salvar. Carregue um modelo e espere 1 minuto.
   - **Esperado:** aviso "Modelo … descarregado depois de 1 min parado"; a memória da placa é liberada (veja no Gerenciador de Tarefas ou no `nvtop`/`radeontop`).
2. No ícone da bandeja, clique em **Pausar**.
   - **Esperado:** o item vira **Retomar**, e pedidos à API recebem "O Ignis está pausado".
3. Clique em **Retomar**.
   - **Esperado:** volta a responder.

## 6. Abrir com o computador
1. Reinicie o computador (a opção vem ligada).
   - **Esperado:** depois do login, o ícone aparece perto do relógio **sem** abrir a janela.
   - Windows: Gerenciador de Tarefas → Inicializar mostra "Ignis".
   - Linux: existe `~/.config/autostart/ignis.desktop`.
2. Desligue a opção em **Motor → Configurações** e reinicie.
   - **Esperado:** o Ignis não abre sozinho.

## 7. Importar do Ollama
1. Com o Ollama instalado e algum modelo baixado, vá em **Modelos → Procurar modelos do Ollama**.
   - **Esperado:** a lista dos modelos do Ollama.
2. Clique em **Usar daqui** num deles e depois em **Carregar**.
   - **Esperado:** funciona sem copiar o arquivo.
3. Remova o modelo do Ignis.
   - **Esperado:** o Ollama continua com o modelo (`ollama list` mostra igual).

## 8. Ligar no Valkode
1. No Valkode, configure um provedor "OpenAI compatível" com endereço `http://127.0.0.1:11470/v1` (qualquer chave). O modelo é o id que aparece em `http://127.0.0.1:11470/v1/models`.
   - **Esperado:** o Valkode conversa com streaming e o botão de parar interrompe na hora.
2. O Valkode pode checar `http://127.0.0.1:11470/motor/versao` (detalhes em CONTRATO_VALKODE.md).

## 9. Salvar as saídas reais da placa (para os testes automáticos)
As fixtures em `crates/ignis-core/tests/fixtures/` seguem o formato real das
ferramentas, mas não foram capturadas numa placa de verdade. Nas máquinas de
teste, rode o que existir e mande os arquivos:
```bash
vulkaninfo --summary > vulkaninfo.txt
```
```bash
rocminfo > rocminfo.txt
```
```bash
nvidia-smi --query-gpu=name,memory.total,memory.free,driver_version,pci.device_id --format=csv,noheader,nounits > nvidia-smi.csv
```
Também vale a saída de `ignis-cli detectar` (compilado a partir do código) ou o
**Copiar relatório** da janela.
