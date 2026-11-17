# Resposta ao Valkode: contexto maior, cache de 8 bits e atenção rápida

Pedido de 15/11/2026 ("PEDIDO DO VALKODE PARA O IGNIS"). Feito no Ignis 0.3.1.
Contrato completo em [CONTRATO_VALKODE.md](../CONTRATO_VALKODE.md), seção 4. A `api` continua **1**: todos os campos são novos e opcionais.

## O que mudou para o Valkode

| Pedido | Como ficou |
|---|---|
| 1) Cache em 8 bits por modelo e geral | `POST /motor/carregar {"modelo", "contexto"?, "cache_kv": "f16"\|"q8_0"}` (também em `/motor/modelos/carregar`). Fica guardado para o modelo e vale quando ele recarrega. Geral: `POST /motor/config {"cache_kv": "q8_0"}`. Padrão: `f16`. |
| 1) Contexto pedido no carregamento | `"contexto": 16384` na mesma rota. Maior do que cabe: carrega o maior que cabe, sem erro. `null` ou `0` = automático. |
| 1) Cache V exige atenção rápida | O llama.cpp b12271 liga a atenção rápida sozinho quando o cache é de 8 bits. Se o motor recusar o cache de 8 bits, o Ignis sobe com 16 bits no mesmo motor e avisa. |
| 2) Informar o contexto real | Em `/v1/models` (`ignis.*`) e em `/motor/estado` (`modelo.*`): `contexto_carregado` (o `n_ctx` que o llama-server abriu, lido de `/props`), `contexto_max` (treino), `contexto_possivel`, `cache_kv`, `atencao_rapida`, `vram_livre_bytes`. Também `memoria_cache_bytes` (memória do cache, lida do registro). |
| 3a) Atenção rápida automática | Fica no automático do llama.cpp. Se o motor quebrar ao subir com erro de atenção rápida, o Ignis tenta com `-fa off` e avisa. O estado real vem em `atencao_rapida`. |
| 3b) Contexto automático | Padrão: o maior que cabe, de 8.192 a 32.768 tokens, deixando 1 GB livre acima de 8.192 (para jogos e outros programas). |
| 3c) Reaproveitar o começo | Já acontece (`-np 1`). Medido: no 2º pedido com o mesmo começo, `timings.cache_n` = 3.724 de 3.746 tokens. |
| 3d) Avisos | Em `/motor/estado.avisos`, em português: "não aceitou o cache de 8 bits", "A atenção rápida falhou…", "Atenção rápida desligada…". |

**Para o Valkode ler:** use `ignis.contexto_carregado` de `/v1/models`. Sem o campo (Ignis antigo), continue com 8.192.

## Testes (os do pedido, a até d)

Com o servidor simulado, que imita o llama-server b12271: 7 testes em `crates/fake-llama-server/tests/contexto.rs`.
- a) padrão f16 informa contexto e cache;
- b) q8_0 recarrega com `-ctk/-ctv q8_0`, atenção rápida ligada, contexto maior;
- c) pedido maior do que cabe carrega o maior possível;
- d) placa que recusa 8 bits volta para 16 bits e avisa; atenção rápida que quebra é desligada e avisada.

## Medição real (llama-server oficial b12271, Qwen3 1.7B, processador do servidor de testes)

| | 16 bits (f16) | 8 bits (q8_0) |
|---|---|---|
| Contexto pedido e carregado | 16.384 | 16.384 |
| Memória do cache (16.384 tokens) | 1.792 MB | **952 MB (53%)** |
| Atenção rápida | ligada | ligada |
| Leitura de 3.746 tokens (sem cache) | 135 tokens/s | **46,7 tokens/s** |
| Escrita | 10,4 tokens/s | 10,8 tokens/s |
| Código anotado no começo de 7.556 tokens | achou ("AZUL-7342") | achou ("AZUL-7342") |
| 10 perguntas, temperatura 0 | — | 7 respostas idênticas; 3 com outras palavras, todas certas |
| Automático (sem placa, muita RAM) | 32.768 | 32.768 |

**Atenção:** no **processador**, o cache de 8 bits deixou a **leitura do texto quase 3 vezes mais lenta**. A escrita ficou igual. Com o reaproveitamento do começo (3c), o começo fixo do Valkode (6.238 tokens) só é lido uma vez por conversa, então o efeito diminui. Mesmo assim, por isso o padrão continua **16 bits** e o 8 bits é escolha (por modelo ou geral).

Na placa (Vulkan, RX 580), o impacto na leitura costuma ser bem menor, mas **ainda não foi medido**. O teste está no TESTE_DO_CRIADOR.md, seção 12, e decide se vale ligar 8 bits por padrão na placa.

## Na RX 580 de 8 GB (pelas contas do Ignis; falta medir na placa)

- Qwen3 8B com 16.384 tokens: **não cabe em 16 bits** (o Ignis carrega o maior que cabe) e **cabe em 8 bits**.
- No automático: 8.192 em 16 bits; um pouco acima de 10.000 em 8 bits (por causa da folga de 1 GB).
