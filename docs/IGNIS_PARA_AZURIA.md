# Ignis + Azúria Voice: como ligar os dois

Para o chat do Azúria Voice (repositório `kaikydev1-jpg/azuria-voice`). Escrito
depois de ler o código do Azúria Voice 0.5.13 e o do Ignis 0.2.0.

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
| Modelos de linguagem (tradução da dublagem, `speech_rate`, skills) | `backend/services/llm_providers.py`, `translator.py` | `POST /v1/chat/completions` | **Funciona hoje** (Ignis 0.2.0) |
| Voz: falar e clonar, o motor padrão OmniVoice | `backend/engines/omnivoice_gguf/` | `POST /v1/audio/speech` e `/v1/audio/voices` (omnivoice.cpp) | Próxima versão do Ignis |
| Transcrição (ditado, base da dublagem) | motor "OpenAI-compatible ASR" | `POST /v1/audio/transcriptions` e `/v1/audio/translations` (whisper.cpp) | Depois da voz |
| Música e efeitos (ACE-Step, MusicGen, Stable Audio, AudioLDM2) | `backend/engines/audiogen/` | — | Fica no PyTorch: hoje não há versão GGML desses modelos |

---

## 2. Já funciona hoje: o Ignis como modelo de linguagem do Azúria

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

## 3. Próxima versão do Ignis: voz (OmniVoice na placa)

O Azúria já conhece o **omnivoice.cpp** (`ServeurpersoCom/omnivoice.cpp`, MIT):
é o motor do `omnivoice-gguf`, só que os instaladores do Azúria não trazem o
binário, e o build de Windows dele é só processador. O omnivoice.cpp roda em
CPU, CUDA, ROCm e Vulkan, e tem um servidor compatível com a OpenAI
(`tts-server`).

**O que o Ignis vai fazer:**
- compilar o omnivoice.cpp a partir de um commit fixado, nas versões Vulkan (AMD e Intel), CUDA (NVIDIA), ROCm (Linux) e processador, com SHA-256 publicado;
- baixar só a versão da placa da pessoa e os pesos de `Serveurperso/OmniVoice-GGUF`, com revisão fixada e a quantização escolhida pela memória de vídeo (a mesma tabela do `omnivoice_gguf/quant_map.json` do Azúria: BF16 com 12 GB ou mais, Q8_0 de 4 a 12 GB, Q4_K_M abaixo disso ou no processador);
- testar a velocidade e cair para o próximo motor se um falhar, como já faz com os modelos de texto;
- subir o servidor de voz só quando pedido e liberar a placa depois de um tempo parado.

**Contrato que o Azúria vai usar** (o mesmo do `tts-server`, repassado pelo Ignis):

| Rota | Corpo | Resposta |
|---|---|---|
| `POST /v1/audio/voices` | `{"name": "minha-voz", "ref_text": "<transcrição>", "wav_b64": "<WAV em base64>"}` (ou `"rvq_b64"`) | voz registrada, guardada pelo Ignis |
| `GET /v1/audio/voices` | — | `{"data": [{"name": …}]}` |
| `POST /v1/audio/speech` | `{"model": "omnivoice", "input": "texto", "voice": "minha-voz", "language": "Portuguese", "response_format": "wav" \| "pcm", "seed": 42}` | WAV 24 kHz mono (`"pcm"` = s16le em streaming) |
| `GET /motor/estado` | — | `voz.motor` (Vulkan/CUDA/ROCm/processador) e `voz.placa` |

**No Azúria:** um motor novo, `ignis-tts`, no estilo do cliente
"OpenAI-compatible ASR" que ele já tem:
1. Quando a pessoa escolhe uma voz clonada, manda o clipe de referência (já cortado pela regra `best_window` de até 20 s) para `/v1/audio/voices`, uma vez por voz.
2. Gera com `/v1/audio/speech`.

Disponível só quando `GET /motor/versao` responder e `GET /v1/models` listar
`omnivoice`. Nada muda para quem não tem o Ignis.

---

## 4. Depois: transcrição na placa

O Ignis vai supervisionar o `whisper-server` (whisper.cpp) e responder em
`POST /v1/audio/transcriptions` e `POST /v1/audio/translations`, no formato da
OpenAI, com `response_format=verbose_json` e segmentos com tempo.

**No Azúria não precisa de código novo:** o motor "OpenAI-compatible (local or
remote)" já existe. Basta configurar `Server URL = http://127.0.0.1:11470/v1` e
`Model = default`, de preferência com um botão pronto "Usar o Ignis".

---

## 5. A opção "Usar a placa pelo Ignis" no Azúria

Sugestão para **Settings → Performance & Device**:
- **Interruptor "Usar a placa pelo Ignis (AMD, NVIDIA, Intel)"**, que aparece quando `GET /motor/versao` responde. Ligado, ele:
  - escolhe o provedor de linguagem `ignis`;
  - aponta a transcrição "OpenAI-compatible" para o Ignis, quando disponível;
  - escolhe o motor de voz `ignis-tts`, quando disponível.
- **Linha de estado:** "Ignis: Vulkan na AMD Radeon RX 580", lida de `GET /motor/estado`.
- **No `GET /api/settings/gpu-report`:** os motores servidos pelo Ignis marcados como "GPU (pelo Ignis)".
- **Ignis fechado:** "Abra o Ignis (ele fica perto do relógio)", com o link de download.

A regra de paridade do Azúria vale: a opção é a mesma no Windows, no Linux e no macOS (no macOS o Ignis ainda não tem versão; a opção só não aparece).

---

## 6. Texto pronto para colar no chat do Azúria Voice

```
Leia docs/IGNIS_PARA_AZURIA.md do repositório kaikydev1-jpg/Ignis (o motor de IA local do criador). Implemente no Azúria Voice:
1. Agora: o provedor de LLM "Ignis (local)" em backend/services/llm_providers.py (seção 2 do documento), com textos pelo i18n e teste.
2. Agora: em Settings → Performance & Device, o interruptor "Usar a placa pelo Ignis" e a linha de estado (seção 5), lendo GET /motor/versao e GET /motor/estado; sem o Ignis, nada muda.
3. Quando o Ignis lançar a voz: o motor ignis-tts (seção 3).
4. Quando o Ignis lançar a transcrição: o botão "Usar o Ignis" no motor OpenAI-compatible ASR (seção 4).
Siga as regras do repositório (CLAUDE.md, AGENTS.md): paridade entre sistemas, i18n, changelog, testes.
```
