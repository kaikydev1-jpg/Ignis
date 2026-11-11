# Contrato Ignis ↔ Valkode (API local, versão 1)

É por aqui que o Valkode (ou qualquer outro programa) se liga ao Ignis.

## Onde fica
- Endereço: `http://127.0.0.1:<porta>` — **só no próprio computador**, nunca na rede.
- Porta padrão: **11470** (configurável na janela, seção Motor). Para descobrir
  a porta escolhida pela pessoa, leia `"porta"` em:
  - Linux: `~/.config/ignis/config.json`
  - Linux (Flatpak): `~/.var/app/io.github.kaikydev1jpg.Ignis/config/ignis/config.json`
  - Windows: `%APPDATA%\ignis\config.json`
- Sem senha (acesso local). Pedidos com cabeçalho `Host` diferente de
  `127.0.0.1`/`localhost`/`[::1]` são recusados (proteção contra DNS rebinding).
- Navegadores: só origens locais (`http://localhost:*`, `http://127.0.0.1:*`)
  ou do Tauri são aceitas; sites da internet recebem 403. Programas de desktop
  (Node, Rust, Python, Electron no processo principal) não mandam `Origin` e
  funcionam normalmente.

## 1. Saber se o motor serve: `GET /motor/versao`
```json
{
  "nome": "Ignis",
  "app": "0.1.0",
  "api": 1,
  "llama_cpp": { "versao": "b12271", "numero": 12271, "commit": "3e86ce2c2" },
  "motor_em_uso": { "variante": "linux-vulkan", "backend": "vulkan" }
}
```
Regra para o Valkode:
- `api` diferente da versão que o Valkode conhece → avisar "Ignis incompatível; atualize".
- `llama_cpp.numero` menor que o mínimo que o Valkode precisa → avisar
  "O motor do Ignis está desatualizado. Abra o Ignis → Motor → Procurar atualizações".
- Sem resposta → "O Ignis não está aberto" (sugerir abrir; ele fica perto do relógio).

## 2. API compatível com a OpenAI
Use qualquer cliente OpenAI com `base_url = http://127.0.0.1:11470/v1` e
qualquer `api_key` (é ignorada).

### `GET /v1/models`
```json
{ "object": "list", "data": [
  { "id": "qwen3-8b", "object": "model", "created": 1794370000, "owned_by": "ignis",
    "ignis": { "nome": "Qwen3 8B", "tamanho": 5027783488, "embeddings": false,
               "carregado": true, "contexto_max": 40960 } }
] }
```
- `id` é o que vai no campo `model`. Modelos do catálogo usam o id do catálogo
  (`qwen3-8b`), importados do Ollama usam `ollama/<nome:tag>`, arquivos usam
  `arquivo/<nome>`.
- `ignis.embeddings = true` → modelo só de embeddings (não conversa).

### `POST /v1/chat/completions` (com e sem streaming)
Corpo igual ao da OpenAI (`messages`, `max_tokens`, `temperature`, `stream`,
`tools`, etc.); o Ignis repassa ao llama-server oficial.
- `model`: id de `/v1/models`. Vazio/`"default"` → usa o modelo carregado, ou o
  padrão escolhido pela pessoa, ou o primeiro de conversa instalado.
- O primeiro pedido carrega o modelo (pode levar alguns segundos). Um modelo
  por vez: pedir outro modelo espera as conversas em andamento terminarem e troca.
- Streaming: `text/event-stream` com `data: {...}` e `data: [DONE]` no fim.
- A resposta traz o cabeçalho **`X-Ignis-Requisicao`** (número do pedido).
- O llama-server inclui `timings` (tokens/s) no fim; o Ignis mostra isso na janela.

### Parar uma resposta
Duas formas (use a que for mais fácil):
1. **Fechar a conexão** do streaming — o motor para de gerar na hora.
2. `POST /motor/parar` com `{"id": "<X-Ignis-Requisicao>"}` — o streaming
   termina com `data: [DONE]`. Sem `id`, para todas as conversas.
   Pedido sem streaming parado responde **499** com código `parado`.

### `POST /v1/completions`
Repasse direto (útil para completar código com FIM, ex.: modelo
`qwen2.5-coder-1.5b`).

### `POST /v1/embeddings`
Só com modelo de embeddings (ex.: `qwen3-embedding-0.6b`). Sem `model` → usa o
modelo de embeddings instalado. Modelo de conversa → 400 explicando.

## 3. Erros
Sempre neste formato (compatível com a OpenAI, mais `erro` em português):
```json
{ "error": { "message": "…", "type": "…", "code": "…" }, "erro": "…" }
```
| HTTP | `code` | Quando | O que o Valkode deve fazer |
|---|---|---|---|
| 400 | `invalid_request_error`, `json_invalido` | pedido inválido, modelo do tipo errado | mostrar a mensagem |
| 403 | `acesso_negado`, `origem_negada` | pedido de fora do computador / site | — |
| 404 | `model_not_found` | modelo não instalado | listar `/v1/models` |
| 409 | `ocupado` | outro modelo respondendo há muito tempo / teste rodando | tentar de novo depois |
| 499 | `parado` | pedido parado pela pessoa | — |
| 500 | `motor_falhou`, `arquivo_sumiu` | nenhum motor conseguiu abrir o modelo | mostrar a mensagem (já diz o que fazer) |
| 502 | `motor_sem_resposta` | o motor caiu no meio | tentar de novo (o Ignis troca de motor sozinho) |
| 503 | `ignis_pausado` | pessoa pausou o Ignis | avisar "Ignis pausado" |
| 503 | `sem_motor` | Ignis ainda não foi preparado | pedir para abrir o Ignis e clicar em Preparar |
| 503 | `sem_modelo` | nenhum modelo instalado | pedir para baixar um modelo no Ignis |

## 4. Rotas próprias (`/motor/*`)
Todas respondem JSON. `POST` aceita corpo JSON.

| Rota | O que faz |
|---|---|
| `GET /motor/estado` | Tudo de uma vez: `fase` (`parado`, `carregando`, `pronto`, `pausado`, `testando`, `preparando`), `fase_texto`, `placa`, `motor` em uso, `modelo` carregado (contexto, camadas na placa), `velocidade` (`leitura_tps`, `geracao_tps`), `porta`, `api.endereco`, `teste` (resultados do teste de velocidade), `precisa_preparar`, `tarefas` (downloads), `avisos`, `requisicoes_ativas` |
| `GET /motor/versao` | Versões (seção 1) |
| `GET /motor/modelos` | `instalados` (com `encaixe`: `placa`/`dividido`/`processador`/`nao-cabe`), `catalogo` (com `recomendado`, `rotulo`, licença, tamanho, `parcial` baixado) e `tarefas` |
| `POST /motor/modelos/baixar` `{"id":"qwen3-8b"}` | Baixa do catálogo (retoma se já houver parte) |
| `POST /motor/modelos/cancelar` `{"id":"qwen3-8b"}` | Cancela (a parte baixada fica guardada) |
| `POST /motor/modelos/remover` `{"id":"…"}` | Remove (apaga só arquivos da pasta do Ignis; do Ollama/arquivos próprios só tira da lista) |
| `GET /motor/modelos/ollama` | Modelos GGUF achados na pasta do Ollama |
| `POST /motor/modelos/importar` `{"referencia":"llama3.2:3b","modo":"usar"\|"copiar"}` ou `{"caminho":"/x/m.gguf"}` | Importa do Ollama (nunca apaga nada lá) ou um arquivo |
| `POST /motor/modelos/carregar` `{"id":"…"}` | Carrega já (sem esperar o primeiro pedido) |
| `POST /motor/modelos/descarregar` | Libera a placa |
| `POST /motor/modelos/padrao` `{"id":"…"}` | Modelo usado quando o pedido não diz qual |
| `POST /motor/parar` `{"id":"…"}` | Para uma conversa (ou todas) |
| `POST /motor/pausar` / `POST /motor/retomar` | Pausar descarrega e recusa pedidos até retomar |
| `POST /motor/preparar` | Baixa os motores da placa + modelo de teste e testa a velocidade |
| `POST /motor/testar` | Roda o teste de velocidade de novo |
| `POST /motor/detectar` | Detecta a placa de novo |
| `GET /motor/motores` | Candidatos (disponível/instalado/motivo/dica), ordem de uso, motor forçado |
| `POST /motor/motores/forcar` `{"variante":"linux-vulkan"}` ou `{"variante":null}` | Força um motor (ou volta ao automático) |
| `GET /motor/config` / `POST /motor/config` | Configurações (`porta`, `descarregar_apos_min`, `contexto_preferido`, `abrir_com_computador`, `procurar_atualizacoes_sozinho`, `llama_server_proprio`, `modelo_padrao`) |
| `GET /motor/atualizacoes`, `POST /motor/atualizacoes/procurar`, `POST /motor/atualizacoes/aplicar` `{"item":"motor"\|"catalogo"}` | Atualizações (nunca no meio de uma conversa) |
| `GET /motor/relatorio` | Diagnóstico completo (detecção, candidatos, teste) |
| `GET /motor/registro` | Últimas linhas do registro do llama-server |

## 5. Exemplos
```bash
curl http://127.0.0.1:11470/motor/versao
```
```bash
curl http://127.0.0.1:11470/v1/chat/completions -H 'content-type: application/json' -d '{"model":"qwen3-8b","stream":true,"messages":[{"role":"user","content":"Olá!"}]}'
```
```python
from openai import OpenAI
c = OpenAI(base_url="http://127.0.0.1:11470/v1", api_key="ignis")
r = c.chat.completions.create(model="qwen3-8b", messages=[{"role": "user", "content": "Olá!"}])
print(r.choices[0].message.content)
```
```js
const r = await fetch("http://127.0.0.1:11470/v1/chat/completions", {
  method: "POST", headers: { "content-type": "application/json" },
  body: JSON.stringify({ model: "qwen3-8b", stream: true, messages: [{ role: "user", content: "Olá!" }] }),
});
const id = r.headers.get("x-ignis-requisicao"); // para POST /motor/parar
```

## 6. Compatibilidade
- Mudanças que **quebram** o contrato sobem `api` (1 → 2). Campos novos podem
  aparecer a qualquer momento: ignore o que não conhecer.
- `llama_cpp.numero` sobe quando o motor é atualizado (atualização aceita pela pessoa).
