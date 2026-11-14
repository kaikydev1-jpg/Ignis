# Ignis + Azúria Voice: como ligar os dois

Para o chat do Azúria Voice (repositório `kaikydev1-jpg/azuria-voice`). Escrito
depois de ler o código do Azúria Voice 0.5.13. Atualizado para o **Ignis 0.3.0**, que já traz
voz (OmniVoice, com clonagem) e transcrição (Whisper) na placa.

**Objetivo:** o Azúria usar a placa de vídeo de verdade (AMD, NVIDIA e Intel,
no Windows e no Linux) por meio do Ignis.

---

## 1. O problema que o Ignis resolve

O Azúria Voice é uma distribuição do VoiceStudio. Os motores dele rodam em
**PyTorch**, e no Windows o PyTorch só acelera **NVIDIA (CUDA)**. Placas AMD e
Intel rodam no processador, e o próprio `README` avisa que a RX 580 não tem
aceleração.

O Ignis roda motores nativos da família GGML (llama.cpp e afins), que usam a
placa por **Vulkan** (AMD e Intel, inclusive a RX 580), **CUDA** (NVIDIA),
**ROCm** (AMD no Linux) ou o processador. O Ignis escolhe sozinho o mais rápido
para cada modelo e cada placa.

Três partes do Azúria podem passar a usar a placa pelo Ignis:

| Parte do Azúria | Onde fica no Azúria | Pelo Ignis | Situação |
|---|---|---|---|
| Modelos de linguagem (tradução da dublagem, `speech_rate`, skills) | `backend/services/llm_providers.py`, `translator.py` | `POST /v1/chat/completions` | **Funciona** (desde o Ignis 0.2.0) |
| Voz: falar e clonar, o motor padrão OmniVoice | `backend/engines/omnivoice_gguf/` | `POST /v1/audio/speech` e `/v1/audio/voices` (omnivoice.cpp) | **Funciona no Ignis 0.3.0** |
| Transcrição (ditado, base da dublagem) | motor "OpenAI-compatible ASR" | `POST /v1/audio/transcriptions` e `/v1/audio/translations` (whisper.cpp) | **Funciona no Ignis 0.3.0** |
| Música e efeitos (ACE-Step, MusicGen, Stable Audio, AudioLDM2) | `backend/engines/audiogen/` | — | Fica no PyTorch: hoje não há versão GGML desses modelos |

---

## 2. O Ignis como modelo de linguagem do Azúria

O Ignis é um servidor compatível com a OpenAI em `http://127.0.0.1:11470/v1`,
sem chave. No Azúria basta um provedor a mais em
`backend/services/llm_providers.py`, ao lado de Ollama e LM Studio:

```python
Provider("ignis", "Ignis (local)", "http://127.0.0.1:11470/v1",
         "default", local=True, key_envs=("IGNIS_API_KEY",),
         base_url_env="IGNIS_BASE_URL", model_env="IGNIS_MODEL",
         signup_url="https://github.com/kaikydev1-jpg/Ignis/releases/latest",
         notes="Fully offline. Uses your AMD, NVIDIA or Intel GPU through Ignis."),
```

| Item | Valor |
|---|---|
| Modelo | `default` = o modelo carregado no Ignis (ou o padrão escolhido nele). Também aceita qualquer `id` de `GET /v1/models`. |
| Chave | qualquer texto; é ignorada |
| Saber se o Ignis está aberto | `GET http://127.0.0.1:11470/motor/versao` (responde `{"nome":"Ignis","api":1,…}`); sem resposta = fechado |
| Placa e motor em uso | `GET /motor/estado` → `placa.nome`, `motor.nome`, `modelo.motivo_motor` |
| Modelos que "pensam" (Qwen3) | o raciocínio vem em `reasoning_content`, separado do `content`. Para responder rápido (dublagem), mande `"chat_template_kwargs": {"enable_thinking": false}` (no SDK da OpenAI: `extra_body=`) |
| Porta diferente | a pessoa pode trocar a porta 11470. Ela fica em `porta` no `config.json` do Ignis (Windows `%APPDATA%\ignis\config.json`, Linux `~/.config/ignis/config.json`, Flatpak `~/.var/app/io.github.kaikydev1jpg.Ignis/config/ignis/config.json`) |

Os textos de interface vão pelo i18n (`locales/*.json`), como pede a regra do
Azúria.

---

## 3. Voz: falar e clonar com o OmniVoice (Ignis 0.3.0)

O Ignis compila o **omnivoice.cpp** (`ServeurpersoCom/omnivoice.cpp`, MIT) a
partir de um commit fixado, nas versões **Vulkan** (AMD, NVIDIA e Intel,
inclusive a RX 580) e **processador**, para Windows e Linux, com SHA-256
publicado. Ele baixa os pesos de `Serveurperso/OmniVoice-GGUF` (revisão
fixada), com a quantização escolhida pela memória de vídeo, a mesma tabela do
`omnivoice_gguf/quant_map.json` do Azúria: BF16 com 12 GB ou mais, Q8_0 de 4 a
12 GB, Q4_K_M abaixo disso ou no processador.

O servidor de voz sobe sozinho no primeiro pedido, na placa. Se a placa falhar
(falta de memória, driver), o Ignis sobe de novo no processador e avisa. Depois
de um tempo parado (o mesmo "descarregar depois de" dos modelos de texto) ele
sai da memória e libera a placa.

**Licença:** os pesos do OmniVoice são **CC BY-NC 4.0 (uso não comercial)**. O
Ignis mostra isso antes de baixar. Se o Azúria tiver versão paga, a voz pelo
Ignis precisa ficar fora dela ou trocar de modelo.

### Contrato

Sem chave. Endereço padrão `http://127.0.0.1:11470` (a porta pode mudar; veja a seção 2).

| Rota | Corpo | Resposta |
|---|---|---|
| `POST /v1/audio/speech` | JSON `{"model": "omnivoice", "input": "texto", "voice": "minha-voz", "language": "Portuguese", "response_format": "wav" \| "pcm", "seed": 42}`. Só `input` é obrigatório; sem `voice` usa a voz padrão do modelo | `audio/wav` 24 kHz mono (também sem formato ou com outro formato); `"pcm"` = s16le cru em streaming |
| `POST /v1/audio/voices` | JSON `{"name": "minha-voz", "ref_text": "<o que foi falado>", "wav_b64": "<WAV em base64>"}` (ou `"rvq_b64"`) | `200` voz guardada no Ignis (sobrevive a reinícios) e registrada no servidor de voz; `400` se o servidor recusar o áudio |
| `GET /v1/audio/voices` | — | `{"object": "list", "data": [{"name": "minha-voz", "ref_text": "…"}]}` |
| `DELETE /v1/audio/voices/{nome}` | — | `200` apagada; `404` se não existir |

Nome da voz: letras, números, espaço, `-`, `_` e `.`, até 64 caracteres.

### No Azúria: motor `ignis-tts`

No estilo do cliente "OpenAI-compatible ASR" que ele já tem:
1. Disponível quando `GET /motor/versao` responde e `GET /v1/models` lista um item com `"id": "omnivoice"`.
2. Ao usar uma voz clonada pela primeira vez, manda o clipe de referência (já cortado pela regra `best_window` de até 20 s) para `POST /v1/audio/voices`, uma vez por voz. Pode conferir antes em `GET /v1/audio/voices`.
3. Gera com `POST /v1/audio/speech`.
4. O primeiro pedido demora mais (carrega o modelo na placa). Use um tempo-limite generoso (2 minutos) e mostre "Carregando a voz no Ignis…".

Nada muda para quem não tem o Ignis.

---

## 4. Transcrição na placa (Ignis 0.3.0)

O Ignis supervisiona o `whisper-server` (whisper.cpp v1.9.5, compilado do mesmo
jeito: Vulkan e processador). Modelos (`ggerganov/whisper.cpp`, revisão
fixada, MIT): **large-v3-turbo (Q5_0)** quando há placa com 2 GB ou mais, **small
(Q5_1)** no processador.

| Rota | Corpo (multipart, igual à OpenAI) | Resposta |
|---|---|---|
| `POST /v1/audio/transcriptions` | `file` (WAV, MP3, FLAC…), `model` (qualquer, ex.: `whisper-1`), `language` (ex.: `pt`), `response_format` (`json`, `text`, `srt`, `vtt`, `verbose_json`), `prompt`, `temperature` | `{"text": "…"}`; com `verbose_json`, também `segments` com `start` e `end` |
| `POST /v1/audio/translations` | os mesmos campos | o texto em inglês |

**No Azúria não precisa de motor novo:** o motor "OpenAI-compatible (local or
remote)" já existe. Basta `Server URL = http://127.0.0.1:11470/v1` e
`Model = whisper-1`, de preferência com um botão pronto "Usar o Ignis".
Aparece em `GET /v1/models` como `"id": "whisper-1"`.

---

## 5. Preparar, estado e erros

A voz e a transcrição não vêm baixadas: a pessoa prepara uma vez (na janela do
Ignis, seção **Voz**, ou pelo Azúria).

| Rota | O que faz |
|---|---|
| `GET /motor/audio` | Plano por família (`voz`, `transcricao`): `preparado`, `falta_baixar` (bytes), `modelo` (`nome`, `licenca`, `uso_comercial`, `tamanho`), `motores` (Vulkan/processador, instalado, falhou), `rodando` (`backend`: `vulkan` ou `cpu`, `motor_nome`, `modelo_nome`); e `vozes` (nomes) |
| `POST /motor/audio/preparar` `{"familias": ["voz", "transcricao"]}` | Baixa em segundo plano (o progresso aparece em `GET /motor/estado` → `tarefas`, ids `audio:…`) |
| `POST /motor/audio/descarregar` | Tira a voz e a transcrição da memória agora |
| `GET /motor/estado` | `audio.voz` e `audio.transcricao` com `preparado` e `rodando` |

Erros no formato da OpenAI (`{"error": {"message", "code"}}`):
- `503` com `code: "audio_nao_preparado"`: falta preparar. Mostre "Prepare a voz no Ignis" e, se quiser, ofereça chamar `POST /motor/audio/preparar` (mostrando antes a licença de `GET /motor/audio`).
- `400`: pedido errado (sem `input`, voz que não existe, áudio recusado). A mensagem já vem em português.
- `502 motor_sem_resposta`: o servidor de voz caiu; tente de novo (o Ignis sobe outro, no processador se a placa falhou).

---

## 6. A opção "Usar a placa pelo Ignis" no Azúria

Sugestão para **Settings → Performance & Device**:
- **Interruptor "Usar a placa pelo Ignis (AMD, NVIDIA, Intel)"**, que aparece quando `GET /motor/versao` responde. Ligado, ele:
  - escolhe o provedor de linguagem `ignis`;
  - escolhe o motor de voz `ignis-tts`;
  - aponta a transcrição "OpenAI-compatible" para o Ignis.
- **Linha de estado:** "Ignis: voz na placa (Vulkan), transcrição pronta", lida de `GET /motor/audio`; o nome da placa vem de `GET /motor/estado` → `placa.nome`.
- **Falta preparar:** botão "Preparar no Ignis", que mostra o tamanho e a licença (`GET /motor/audio`) e chama `POST /motor/audio/preparar`.
- **No `GET /api/settings/gpu-report`:** os motores servidos pelo Ignis marcados como "GPU (pelo Ignis)".
- **Ignis fechado:** "Abra o Ignis (ele fica perto do relógio)", com o link de download.

A regra de paridade do Azúria vale: a opção é a mesma no Windows, no Linux e no
macOS (no macOS o Ignis ainda não tem versão; a opção só não aparece).

O Ignis se atualiza sozinho (aviso de versão nova e um clique), então o Azúria
não precisa cuidar disso. Para conferir a versão: `GET /motor/versao`.

---

## 7. Texto pronto para colar no chat do Azúria Voice

```
Leia docs/IGNIS_PARA_AZURIA.md do repositório kaikydev1-jpg/Ignis (o motor de IA local do criador; versão 0.3.0, já com voz e transcrição na placa). Implemente no Azúria Voice, junto com a mudança de visual:
1. O provedor de LLM "Ignis (local)" em backend/services/llm_providers.py (seção 2), com textos pelo i18n e teste.
2. O motor de voz ignis-tts (seção 3): registra a voz clonada em /v1/audio/voices e gera com /v1/audio/speech; disponível só quando /v1/models listar "omnivoice".
3. O botão "Usar o Ignis" no motor OpenAI-compatible ASR (seção 4), com Model = whisper-1.
4. Em Settings → Performance & Device, o interruptor "Usar a placa pelo Ignis", a linha de estado e o botão "Preparar no Ignis" com a licença (seções 5 e 6). Sem o Ignis aberto, nada muda.
5. Trate os erros da seção 5 (503 audio_nao_preparado, 400, 502) com mensagens claras.
Siga as regras do repositório (CLAUDE.md, AGENTS.md): paridade entre sistemas, i18n, changelog, testes. A voz pelo Ignis usa o OmniVoice (CC BY-NC 4.0, não comercial): deixe isso visível.
```
