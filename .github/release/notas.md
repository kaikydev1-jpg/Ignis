**Ignis {{VERSAO}}**: motor de IA local, leve, para Windows e Linux. Roda modelos de IA no seu computador com a melhor aceleração da sua placa (AMD, NVIDIA, Intel ou só o processador) e atende outros programas por uma porta local compatível com a API da OpenAI.

## Baixe para o seu sistema

### Windows 10 e 11
**[Ignis-Windows.exe](https://github.com/kaikydev1-jpg/Ignis/releases/download/v{{VERSAO}}/Ignis-Windows.exe)**
- Abra o arquivo e siga o instalador. **Não pede administrador.**
- Se aparecer "O Windows protegeu o computador", clique em **Mais informações → Executar assim mesmo** (o instalador ainda não tem assinatura digital).

### Fedora 44 (Workstation, KDE e outras edições)
**[Ignis-Fedora.rpm](https://github.com/kaikydev1-jpg/Ignis/releases/download/v{{VERSAO}}/Ignis-Fedora.rpm)**
```bash
sudo dnf install ./Ignis-Fedora.rpm
```

### Fedora Atomic (Silverblue, Kinoite, Bazzite, Bluefin, Aurora)
**[Ignis-Flatpak.flatpak](https://github.com/kaikydev1-jpg/Ignis/releases/download/v{{VERSAO}}/Ignis-Flatpak.flatpak)**
```bash
flatpak install --user ./Ignis-Flatpak.flatpak
```
Instala sem reiniciar. Na primeira vez, baixa a base do GNOME pelo Flathub. Serve também para qualquer distro com Flatpak.

### Ubuntu, Debian, Linux Mint, Pop!_OS, Zorin
**[Ignis-Ubuntu-Debian.deb](https://github.com/kaikydev1-jpg/Ignis/releases/download/v{{VERSAO}}/Ignis-Ubuntu-Debian.deb)**
```bash
sudo apt install ./Ignis-Ubuntu-Debian.deb
```

### Arch, openSUSE e outras distros
**[Ignis-Linux.AppImage](https://github.com/kaikydev1-jpg/Ignis/releases/download/v{{VERSAO}}/Ignis-Linux.AppImage)**
```bash
chmod +x Ignis-Linux.AppImage && ./Ignis-Linux.AppImage
```
Não instala nada e não precisa de libfuse2. Também dá para usar o Flatpak.

> **GNOME:** o ícone perto do relógio precisa da extensão **AppIndicator** (no Fedora: `sudo dnf install gnome-shell-extension-appindicator`, depois sair e entrar na sessão). O KDE mostra sem extensão.

## Primeiro uso
1. Abra o Ignis e clique em **Preparar o Ignis**. Ele baixa só o motor que serve para a sua placa e testa qual é o mais rápido.
2. Em **Modelos**, baixe um marcado "Roda bem na sua placa".
3. Aponte o seu programa para `http://127.0.0.1:11470/v1` (qualquer chave de API).

Fechar a janela deixa o Ignis perto do relógio. Para encerrar, use **Sair** no menu do ícone.

## O que vem nesta versão
- **Placa e motor:** detecta a placa (fabricante, modelo, memória de vídeo, driver) e escolhe sozinho o motor mais rápido (ROCm, CUDA, Vulkan ou processador). Se um motor falhar, passa para o próximo.
- **Modelos:** catálogo filtrado pela sua placa e **qualquer modelo GGUF do Hugging Face**, inclusive os que pedem aceite dos termos (com a sua conta) e os que entendem imagem e áudio. **Não precisa do Ollama** (dá para trazer modelos de lá, se quiser).
- **Memória:** calcula quanto do modelo cabe na placa, avisa antes quando não cabe e libera a placa depois de 10 minutos parado.
- **API local** compatível com a OpenAI: streaming, botão Parar, embeddings e ferramentas. Só responde neste computador.
- **Janela:** barra de título integrada, com os botões no jeito de cada sistema (Windows 11; no Linux, bolinhas estilo Mac, GNOME ou KDE, conforme o seu); temas claro e escuro.
- **Leve:** cerca de 60 MB de memória e processador perto de 0% quando parado.
- **Privado:** sem telemetria. Só se conecta ao GitHub (motores) e ao Hugging Face (modelos), quando você pede.

## Para desenvolvedores
- Contrato da API: [CONTRATO_VALKODE.md](https://github.com/kaikydev1-jpg/Ignis/blob/v{{VERSAO}}/CONTRATO_VALKODE.md)
- Como ligar um aplicativo: [BRIEFING.md](https://github.com/kaikydev1-jpg/Ignis/blob/v{{VERSAO}}/BRIEFING.md)

## Conferir os arquivos
As somas SHA-256 de todos os arquivos estão em **SHA256SUMS.txt**.

<sub>Testado automaticamente no Windows e no Linux, com o motor oficial do llama.cpp ({{LLAMA}}). Licença MIT; licenças de terceiros em AVISOS.md.</sub>
