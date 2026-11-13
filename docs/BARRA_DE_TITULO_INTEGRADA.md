# Barra de título integrada: como pedir para qualquer IA

## O nome certo
A barra cinza separada, com o nome do app, é a **barra padrão do sistema**.
Tecnicamente ela é a barra "nativa", e por isso, quando você pede "barra
nativa", a IA mantém justamente ela.

O que você quer se chama **barra de título integrada** (em inglês, *custom
title bar* ou *integrated title bar*):
- o app ocupa a janela inteira, até o topo, na cor do próprio app;
- os botões minimizar, maximizar e fechar ficam embutidos no canto;
- a janela continua se comportando como qualquer outra: arrasta, maximiza,
  redimensiona e encaixa.

Exemplos que usam: VS Code, Discord, Spotify, Microsoft Teams, Notion e os apps
do próprio Windows 11 (Configurações, Explorador de Arquivos).

**Palavras para usar no pedido:**
- "barra de título integrada" ou "custom title bar";
- "sem a barra padrão do sistema";
- "conteúdo estendido até o topo".

## Texto pronto para colar no começo de qualquer projeto

```
Janela com BARRA DE TÍTULO INTEGRADA (custom title bar), igual VS Code, Discord e os apps do Windows 11. NÃO use a barra padrão do sistema (aquela faixa separada com o nome do app).

Visual:
- O app ocupa a janela inteira, até o topo, na mesma cor do app (tema claro e escuro).
- Botões minimizar, maximizar/restaurar e fechar no canto superior direito, desenhados no estilo do Windows 11: 46 px de largura cada, ícones finos de 10 px, fundo leve ao passar o mouse e o fechar vermelho (#C42B1C) com X branco.
- O botão maximizar vira "restaurar" (dois quadradinhos) quando a janela está maximizada.
- Com a janela fora de foco, os ícones ficam mais apagados.
- Os botões seguem o sistema:
  - Windows: estilo Windows 11, à direita;
  - Mac: as três bolinhas coloridas à esquerda;
  - Linux: leia o lado, a ordem e quais botões o sistema usa (configuração gtk-decoration-layout do GTK, que o GNOME e o KDE preenchem) e o tema. Botões à esquerda ou tema estilo Mac (WhiteSur, Mojave…) → bolinhas coloridas; GNOME → botões redondos do Adwaita; KDE → estilo Breeze. Se o GNOME mostra só o fechar, mostre só o fechar.
  - Matsu OS: reconheça pelo nome em /etc/os-release (no Flatpak, /run/host/os-release) e use as bolinhas com estas cores:
    - em uso: vermelha #ff5f57, amarela #febc2e, verde #28c840;
    - mouse em cima do grupo: × #4d0000, − #995700 e dois triângulos #006500 no verde;
    - janela em segundo plano: cinza #59545d no tema escuro e #c9c9cf no claro.
  - Tenha uma opção nas configurações para escolher o estilo à mão (Automático, Windows, GNOME, KDE, Mac, Matsu OS).
- O resto do app é igual em todos os sistemas.

Comportamento (tem que continuar igual a uma janela normal):
- Arrastar pela faixa de cima (e por áreas vazias como o logo) move a janela.
- Duplo clique nessa faixa maximiza e restaura.
- Botões, campos e links dentro da faixa continuam clicáveis e não arrastam.
- Redimensionar pelas 4 bordas e pelos 4 cantos.
- No Windows 11: sombra e cantos arredondados do sistema, e arrastar até a borda da tela encaixa a janela (Aero Snap).
- Fechar respeita o comportamento do app (se o app fica na bandeja, fechar só esconde a janela).

Antes de entregar, teste: arrastar, duplo clique, maximizar e restaurar, minimizar, fechar, redimensionar, temas claro e escuro, Windows e Linux.
```

## Dicas por tecnologia (para a IA)

### Tauri 2 (o que o Ignis usa)
- **Janela:** `.decorations(false)` e `.shadow(true)` no `WebviewWindowBuilder`
  (ou `"decorations": false, "shadow": true` no `tauri.conf.json`).
- **Botões:**
  - `getCurrentWindow().minimize()`, `.toggleMaximize()`, `.close()`;
  - `.isMaximized()` e `.onResized()` para trocar o ícone de maximizar/restaurar.
- **Permissões** (em `capabilities`):
  - `core:window:allow-start-dragging`
  - `core:window:allow-minimize`
  - `core:window:allow-toggle-maximize`
  - `core:window:allow-close`
  - `core:window:allow-start-resize-dragging`
- **Armadilha do duplo clique no Linux:** o `data-tauri-drag-region` começa a
  arrastar já no primeiro clique, o sistema pega o mouse e o duplo clique não
  chega. Faça o arraste à mão:
  1. no `mousedown` da faixa, guarde a posição; se `e.detail === 2`, chame
     `toggleMaximize()`;
  2. no `mousemove` com o botão apertado e mais de 3 px de distância, chame
     `startDragging()`.

  Veja `app/ui/app.js` (barra de título integrada).
- **Linux:** sem moldura do sistema não há sombra; um contorno de 1 px no
  conteúdo ajuda a separar a janela do fundo.
- **Mac:** `"titleBarStyle": "Overlay"` e `"hiddenTitle": true`.

### Seguir o estilo do Linux (o que o Ignis faz)
- **Onde o sistema guarda isso:**
  - GNOME: `gsettings get org.gnome.desktop.wm.preferences button-layout`
    (ex.: `appmenu:close`, ou `close,minimize,maximize:` com os botões à esquerda);
  - KDE: `ButtonsOnLeft`/`ButtonsOnRight` no `kwinrc`, que o KDE copia para o GTK.
- **Como ler de dentro do app:** pela configuração `gtk-decoration-layout` do
  GTK. Ela já junta o GNOME, o KDE e o portal do Flatpak.
- **No Tauri** (Linux, na thread principal):
  `gtk::Settings::default()` e depois
  `property::<Option<String>>("gtk-decoration-layout")` e `"gtk-theme-name"`.
- **Regra completa e testes:** `crates/ignis-core/src/janela.rs`. Os desenhos
  de cada estilo estão em `app/ui/app.js` e `app/ui/style.css`.

### Electron
- `titleBarStyle: 'hidden'` com
  `titleBarOverlay: { color: '<cor do app>', symbolColor: '<cor dos ícones>', height: 40 }`.
  Mantém os botões do próprio Windows, inclusive o menu de layouts do Windows 11.
- Atualize as cores no tema escuro com `win.setTitleBarOverlay(...)`.
- CSS:
  - `-webkit-app-region: drag` na faixa de cima;
  - `no-drag` nos botões e campos;
  - `env(titlebar-area-width)` para não ficar embaixo dos botões.
- Mac: `titleBarStyle: 'hiddenInset'`.

### Outras
| Tecnologia | Como fazer |
|---|---|
| WinUI 3 (.NET) | `ExtendsContentIntoTitleBar = true`, `SetTitleBar(elemento)` e fundo `MicaBackdrop` |
| WPF | `WindowChrome` (ou a biblioteca WPF UI, `FluentWindow`) |
| Flutter | pacote `window_manager` com `TitleBarStyle.hidden` e `DragToMoveArea` |
| Qt | `Qt::FramelessWindowHint` com `startSystemMove()` / `startSystemResize()` |
