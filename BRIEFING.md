# Ignis: briefing

**Para quem:** o criador (instalar e usar) e os aplicativos **Azura Voice** e
**Valkode** (ligar ao motor).
**Versão:** app 0.1.0 · API versão 1 · motor llama.cpp b12271 · novembro de 2026.

---

## 1. O que é o Ignis

O Ignis é o **motor de IA local** do Azura Voice e do Valkode. Ele roda modelos
de IA no próprio computador, sem internet e sem mensalidade, usando a placa de
vídeo do melhor jeito possível.

- **Onde roda:** Windows e Linux (Fedora 44, Fedora Atomic, Ubuntu e outras),
  iguais em visual, recursos e otimização.
- **Placas:** foco em AMD; também NVIDIA, Intel ou só o processador. Ele testa
  sozinho qual motor é mais rápido na sua placa (ROCm, CUDA, Vulkan ou
  processador) e troca sozinho se um falhar.
- **Modelos:** catálogo filtrado pela sua placa ou **qualquer modelo GGUF do
  Hugging Face**, inclusive os que pedem aceite dos termos (com a conta de cada
  pessoa). **Não precisa do Ollama.**
- **Leve:** fica perto do relógio, ≈60 MB de memória e processador perto de 0%
  quando parado. Libera a placa sozinho depois de 10 minutos sem uso.
- **Privado:** sem telemetria, e a API só responde neste computador.

**Para os aplicativos, em uma frase:** o Ignis é um servidor compatível com a
OpenAI rodando no computador da pessoa. Quem já fala com a OpenAI fala com o
Ignis trocando só o endereço para `http://127.0.0.1:11470/v1`.

---

## 2. Links

| O quê | Link |
|---|---|
| Página do projeto | https://github.com/kaikydev1-jpg/Ignis |
| **Baixar (versão oficial)** | https://github.com/kaikydev1-jpg/Ignis/releases/latest |
| Contrato completo da API | https://github.com/kaikydev1-jpg/Ignis/blob/claude/admiring-mendel-0vs7n5/CONTRATO_VALKODE.md |
| Testes na máquina real | https://github.com/kaikydev1-jpg/Ignis/blob/claude/admiring-mendel-0vs7n5/TESTE_DO_CRIADOR.md |
| Versões de teste (Actions) | https://github.com/kaikydev1-jpg/Ignis/actions/workflows/instaladores.yml |

A versão oficial baixa **sem login e sem prazo**. As versões de teste são
geradas a cada mudança, pedem login no GitHub e ficam disponíveis 90 dias.

---

## 3. Como instalar

Links diretos (sempre a última versão):

| Sistema | Baixe | Como instalar |
|---|---|---|
| Windows 10/11 | [Ignis-Windows.exe](https://github.com/kaikydev1-jpg/Ignis/releases/latest/download/Ignis-Windows.exe) | Abra o arquivo. Não pede administrador. |
| Fedora 44 | [Ignis-Fedora.rpm](https://github.com/kaikydev1-jpg/Ignis/releases/latest/download/Ignis-Fedora.rpm) | `sudo dnf install ./Ignis-Fedora.rpm` |
| Fedora Atomic (Silverblue, Kinoite, Bazzite) | [Ignis-Flatpak.flatpak](https://github.com/kaikydev1-jpg/Ignis/releases/latest/download/Ignis-Flatpak.flatpak) | `flatpak install --user ./Ignis-Flatpak.flatpak` |
| Matsu OS (Matsu 27 OS e próximos) | [Ignis-Matsu-OS.flatpak](https://github.com/kaikydev1-jpg/Ignis/releases/latest/download/Ignis-Matsu-OS.flatpak) | Abra na loja do sistema, ou `flatpak install --user ./Ignis-Matsu-OS.flatpak` |
| Ubuntu, Debian, Mint | [Ignis-Ubuntu-Debian.deb](https://github.com/kaikydev1-jpg/Ignis/releases/latest/download/Ignis-Ubuntu-Debian.deb) | `sudo apt install ./Ignis-Ubuntu-Debian.deb` |
| Outras distros | [Ignis-Linux.AppImage](https://github.com/kaikydev1-jpg/Ignis/releases/latest/download/Ignis-Linux.AppImage) | `chmod +x Ignis-Linux.AppImage` e abra o arquivo |

**Pelo terminal (qualquer Linux):** descobre o sistema, baixa, confere e instala.
```bash
curl -fsSL https://github.com/kaikydev1-jpg/Ignis/releases/latest/download/instalar.sh | bash
```

**Avisos:**
- **Windows:** o instalador ainda não tem assinatura digital. Se aparecer "O
  Windows protegeu o computador", clique em **Mais informações → Executar
  assim mesmo**.
- **Flatpak:** na primeira vez ele baixa a base do GNOME pelo Flathub (se
  perguntar, aceite).
- **GNOME (Fedora Workstation/Silverblue):** o ícone perto do relógio precisa da
  extensão AppIndicator (`sudo dnf install gnome-shell-extension-appindicator`,
  depois sair e entrar na sessão). O KDE mostra sem extensão.
- **AMD no Linux:** sem fazer nada, ele já usa o Vulkan (vem com o Mesa). O
  ROCm é opcional e às vezes mais rápido:
  1. Instale com `sudo dnf install rocm-hip hipblas rocblas`.
  2. Rode `sudo usermod -aG render,video $USER`.
  3. Saia e entre de novo na sessão.

  O próprio Ignis mostra o comando certo para a sua distro.

---

## 4. Como usar (para a pessoa)

1. **Abra o Ignis.** Na primeira vez, clique em **Preparar o Ignis**. Ele:
   - baixa só o motor da sua placa e um modelo pequeno de teste (Qwen3 0.6B,
     610 MB);
   - mede qual motor é mais rápido.

   Leva alguns minutos.
2. **Modelos → Recomendados:** baixe um modelo marcado "Roda bem na sua
   placa". Sugestões:

   | Para quê | Modelo | Tamanho |
   |---|---|---|
   | Conversa e voz, rápido | Qwen3 4B | 2,5 GB |
   | Conversa, mais inteligente (placa de 8 GB ou mais) | Qwen3 8B | 5,0 GB |
   | Código (Valkode) | Qwen2.5 Coder 7B | 4,7 GB |
   | Completar código enquanto digita | Qwen2.5 Coder 1.5B | 1,6 GB |
   | Busca em documentos (embeddings) | Qwen3 Embedding 0.6B | 0,6 GB |

   Qualquer outro modelo: **Modelos → Qualquer modelo do Hugging Face**.
3. **Carregue o modelo** (botão **Carregar**) ou deixe o primeiro pedido do
   aplicativo carregar sozinho.
4. **Status** mostra se está rodando, o modelo, o motor e a velocidade, com
   exemplos prontos para copiar.

**No dia a dia:**
- Fechar a janela deixa o Ignis perto do relógio.
- O menu do ícone tem **Abrir**, **Pausar/Retomar**, **Descarregar modelo** e
  **Sair**.
- Ele abre junto com o computador; dá para desligar em **Motor**.
- Se algo der errado: **Placa → Copiar relatório**, e mande o texto.

---

## 5. Como ligar um aplicativo ao Ignis (Azura Voice, Valkode e outros)

### 5.1 O básico
| Item | Valor |
|---|---|
| `base_url` | `http://127.0.0.1:11470/v1` |
| `api_key` | qualquer texto (é ignorada) |
| `model` | um `id` de `GET /v1/models` (ex.: `qwen3-8b`), ou `"default"` para usar o modelo carregado ou o padrão da pessoa |
| Formato | igual ao da OpenAI: `chat/completions` (com streaming), `completions`, `embeddings`, `tools`, imagens |

**Porta:** a pessoa pode trocar a porta 11470. A porta escolhida fica no campo
`porta` do arquivo de configuração:

| Sistema | Arquivo |
|---|---|
| Windows | `%APPDATA%\ignis\config.json` |
| Linux | `~/.config/ignis/config.json` |
| Linux (Flatpak) | `~/.var/app/io.github.kaikydev1jpg.Ignis/config/ignis/config.json` |

### 5.2 Fluxo recomendado
1. **Ao abrir o app:** `GET /motor/versao`.
   - Sem resposta: mostrar "O Ignis não está aberto. Abra o Ignis (ele fica
     perto do relógio)."
   - `"api"` diferente de `1`: mostrar "Versão do Ignis incompatível; atualize
     o Ignis."
2. **Listar modelos:** `GET /v1/models`.
   - Para conversa, ignore os modelos com `ignis.embeddings: true`.
   - Lista vazia: mostrar "Baixe um modelo no Ignis (seção Modelos)."
3. **Pré-carregar** (recomendado para voz): `POST /motor/modelos/carregar`
   com `{"id": "qwen3-4b"}`. Chame ao abrir o app ou ao ligar o microfone,
   para a primeira fala não esperar o modelo carregar.
4. **Conversar** com `"stream": true`.
5. **Parar** uma resposta: feche a conexão; o motor para na hora. Outra forma:
   `POST /motor/parar` com `{"id": "<cabeçalho X-Ignis-Requisicao>"}`.
6. **Mostrar o estado** (opcional): `GET /motor/estado` traz `fase`
   (`parado`, `carregando`, `pronto`, `pausado`…), modelo, motor e velocidade.

### 5.3 Exemplos

**Python** (cliente oficial da OpenAI):
```python
from openai import OpenAI

ignis = OpenAI(base_url="http://127.0.0.1:11470/v1", api_key="ignis")
resposta = ignis.chat.completions.create(
    model="qwen3-4b",
    messages=[{"role": "user", "content": "Olá! Quem é você?"}],
    stream=True,
    extra_body={"chat_template_kwargs": {"enable_thinking": False}},  # ver 5.4
)
for pedaco in resposta:
    print(pedaco.choices[0].delta.content or "", end="", flush=True)
```

**JavaScript/TypeScript** (Node, Electron no processo principal, Tauri),
com streaming e botão Parar:
```js
const parar = new AbortController(); // parar.abort() interrompe na hora
const r = await fetch("http://127.0.0.1:11470/v1/chat/completions", {
  method: "POST",
  headers: { "content-type": "application/json" },
  signal: parar.signal,
  body: JSON.stringify({
    model: "qwen3-4b",
    stream: true,
    chat_template_kwargs: { enable_thinking: false },
    messages: [{ role: "user", content: "Olá!" }],
  }),
});
const leitor = r.body.getReader();
const dec = new TextDecoder();
let resto = "";
for (;;) {
  const { value, done } = await leitor.read();
  if (done) break;
  resto += dec.decode(value, { stream: true });
  const linhas = resto.split("\n");
  resto = linhas.pop();
  for (const l of linhas) {
    if (!l.startsWith("data: ") || l === "data: [DONE]") continue;
    const texto = JSON.parse(l.slice(6)).choices[0]?.delta?.content;
    if (texto) process.stdout.write(texto); // ou mande para a tela / para a voz
  }
}
```

**Terminal:**
```bash
curl http://127.0.0.1:11470/v1/chat/completions -H 'content-type: application/json' -d '{"model":"default","messages":[{"role":"user","content":"Olá!"}]}'
```

### 5.4 Modelos que "pensam" (Qwen3 e outros)
- O raciocínio vem **separado** da resposta:
  - sem streaming: `message.reasoning_content` (raciocínio) e
    `message.content` (resposta);
  - com streaming: `delta.reasoning_content` e `delta.content`.
- Mostre ou fale só o `content`.
- Para respostas rápidas (essencial na voz), desligue o pensamento mandando
  `"chat_template_kwargs": {"enable_thinking": false}` no pedido.

Testado no motor oficial com o Qwen3.

### 5.5 Ferramentas (function calling)
- Mande `tools` no formato da OpenAI; a resposta vem com `tool_calls` e
  `finish_reason: "tool_calls"`.
- Testado no motor oficial com o Qwen3.
- A qualidade depende do modelo: os de 4B para cima vão melhor.

### 5.6 Imagens e áudio (entrada)
- Funciona com modelos baixados com **"Incluir visão/áudio"** ligado (campo
  `visao: true` em `GET /motor/modelos`).
- **Imagem:**
  ```json
  {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,..."}}
  ```
- **Áudio**, só em modelos que entendem áudio:
  ```json
  {"type": "input_audio", "input_audio": {"data": "<base64>", "format": "wav"}}
  ```
  Formatos: `wav` ou `mp3`.
- Pedidos de até 256 MB.
- **Situação:** o caminho está pronto e testado com o motor simulado, mas
  ainda não com um modelo de visão de verdade (está no teste do criador).

### 5.7 Embeddings
`POST /v1/embeddings` com um modelo de embeddings (ex.: `qwen3-embedding-0.6b`).
Sem `model`, ele usa o modelo de embeddings instalado.

### 5.8 Erros e o que mostrar
Formato da OpenAI, mais o campo `erro` com a mensagem em português:
```json
{ "error": { "message": "...", "type": "...", "code": "..." }, "erro": "..." }
```

| HTTP | `code` | O que mostrar |
|---|---|---|
| sem resposta | — | "O Ignis não está aberto" |
| 503 | `sem_motor` | "Abra o Ignis e clique em Preparar o Ignis" |
| 503 | `sem_modelo` | "Baixe um modelo no Ignis" |
| 503 | `ignis_pausado` | "O Ignis está pausado (retome pelo ícone perto do relógio)" |
| 404 | `model_not_found` | listar `/v1/models` de novo e pedir para escolher |
| 409 | `ocupado` | tentar de novo em alguns segundos |
| 502 | `motor_sem_resposta` | tentar de novo (o Ignis troca de motor sozinho) |
| 500 | `motor_falhou` | mostrar o campo `erro` (já diz o que fazer) |

### 5.9 De onde chamar
- **Funcionam direto:** programas (Node, Python, Rust, C#) e o processo
  principal do Electron.
- **De dentro de uma página** (webview, Tauri, Electron): só origens locais
  (`http://localhost:*`, `http://127.0.0.1:*`) e as do Tauri.
- **Recusados por segurança (403):** página aberta como arquivo (`file://`,
  origem `null`) e sites da internet. No Electron, faça os pedidos pelo
  processo principal.

### 5.10 Rotas próprias mais úteis
| Rota | Para quê |
|---|---|
| `GET /motor/versao` | Saber se o Ignis está aberto e a versão |
| `GET /motor/estado` | Fase, modelo carregado, motor, velocidade |
| `GET /motor/modelos` | Modelos instalados (com `visao`, `tipo`, se cabe na placa) e catálogo |
| `POST /motor/modelos/carregar` `{"id"}` | Carregar já |
| `POST /motor/modelos/descarregar` | Liberar a placa |
| `POST /motor/modelos/padrao` `{"id"}` | Modelo usado quando o pedido diz `"default"` |
| `POST /motor/parar` `{"id"}` | Parar uma resposta (sem `id`, para todas) |
| `POST /motor/pausar` / `POST /motor/retomar` | Pausar e retomar o Ignis |

A lista completa está no contrato (link na seção 2).

---

## 6. Notas para o Azura Voice

- **O que o Ignis faz:** a parte de texto, o "cérebro" que entende e responde.
- **O que ele ainda não faz:** voz sintetizada (TTS) e transcrição dedicada
  (tipo Whisper). O Azura Voice continua usando o que já usa para ouvir e
  falar. Modelos que entendem áudio direto estão na seção 5.6.
- **Para responder rápido:**
  - modelo pequeno (Qwen3 4B);
  - `stream: true`;
  - pensamento desligado (5.4);
  - pré-carregar ao ligar o microfone (5.2);
  - começar a falar a cada frase completa que chega no streaming.
- **A pessoa falou por cima:** feche a conexão, e a geração para na hora.
- **Depois de 10 minutos sem uso:** o modelo sai da memória e o próximo pedido
  recarrega (alguns segundos). O pré-carregar resolve, e a pessoa pode
  aumentar esse tempo em **Motor**.
- **Um modelo por vez:** se o Azura Voice e o Valkode pedirem modelos
  diferentes, o Ignis troca de modelo (espera a resposta em andamento
  terminar). Para evitar trocas, usem `"default"` ou o mesmo modelo.

## 7. Notas para o Valkode

- **Provedor:** "OpenAI compatível", com `base_url` `http://127.0.0.1:11470/v1`.
  Streaming e Parar como em 5.2 e 5.3.
- **Completar código** (FIM):
  - `POST /v1/completions` com o Qwen2.5 Coder 1.5B;
  - prompt no formato do modelo:
    `<|fim_prefix|>…código antes…<|fim_suffix|>…código depois…<|fim_middle|>`.
- **Agente com ferramentas:** veja 5.5.
- **Indexar o projeto:** embeddings (5.7).
- **Versão mínima do motor:** se o Valkode precisar de um motor mínimo, compare
  `llama_cpp.numero` de `/motor/versao` e peça para a pessoa atualizar em
  **Motor → Atualizações**.

---

## 8. Limites e o que falta provar

- **Ainda não faz:** voz sintetizada, transcrição dedicada, geração de imagem.
- **Um modelo carregado por vez.**
- **Falta provar em placa de vídeo real:** os testes em AMD (ROCm e Vulkan) e
  NVIDIA (CUDA) estão em `TESTE_DO_CRIADOR.md`. O que foi provado sem placa:
  - testes automáticos no Windows e no Linux;
  - o motor oficial respondendo nos dois sistemas;
  - downloads reais e busca no Hugging Face.
- **Distribuição:** o instalador do Windows ainda não tem assinatura digital.

---

## 9. Pedido para os chats do Azura Voice e do Valkode

Leiam este briefing e respondam com **o que falta no Ignis** para o seu
aplicativo funcionar do melhor jeito, neste formato:

```
App: (Azura Voice ou Valkode)
Como o app vai chamar o Ignis: (linguagem, biblioteca, de onde: processo principal ou página)
Modelos que pretende usar:
O que já dá para fazer com o que está aqui:
O que falta (um item por linha, com prioridade alta/média/baixa e o porquê):
-
Rotas ou campos novos desejados (nome, o que entra, o que sai):
Dúvidas:
```

O criador traz essa resposta de volta para o chat do Ignis, que atualiza o
motor antes de gerar a próxima versão.
