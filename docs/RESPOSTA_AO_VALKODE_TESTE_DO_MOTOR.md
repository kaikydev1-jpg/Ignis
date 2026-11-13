# Resposta do Ignis ao pedido "o teste de velocidade escolhe o processador para modelos grandes"

Pedido de 13/11/2026 (RX 580 2048SP, gfx803, 8 GB, Windows 11, Ignis 0.1.1).
**Corrigido no Ignis 0.2.0.**

## 1. O motor agora é escolhido pelo modelo

O teste do "Preparar" continua rodando com um modelo pequeno, mas o resultado
só vale para modelos parecidos com ele (até 2x o tamanho). Para cada modelo, a
ordem dos motores segue estas regras:

1. **Motor escolhido à mão** vem primeiro. Motor que falhou nesta sessão vai por último.
2. **Medido com este modelo:** se o modelo já foi medido em dois ou mais motores, vale o mais rápido. A medida vem do uso real (cada resposta com 16 tokens ou mais anota os tokens/s do motor em uso) ou do botão "Testar velocidade".
3. **Modelo pequeno** (até 2x o tamanho do modelo do teste): vale o teste geral.
4. **Modelo maior:** a placa vem primeiro se pelo menos metade das camadas couber nela; senão, o processador.

No caso relatado, o Qwen3 8B (5 GB) cabe inteiro nos 8 GB da RX 580, então vai
para o **Vulkan** ("escolhido porque o modelo cabe na placa"). O modelo pequeno
continua no processador, porque nele o processador foi mesmo mais rápido.

Também há o botão **"Testar velocidade"** em cada modelo instalado. A rota é
`POST /motor/testar` com `{"modelo": "<id>"}`. O botão mede todos os motores
com o próprio modelo (camadas na placa e contexto como no uso real), e o mais
rápido passa a valer para ele.

Corrigido junto: depois de escolher um motor à mão, o modelo já carregado
agora troca de motor no próximo pedido. Antes, ele ficava no motor anterior até
descarregar.

**Como conferir na API:**
- `GET /motor/estado` → `modelo.motivo_motor` (por que esse motor) e `ultimo_carregamento` (abaixo);
- `GET /motor/modelos` → `instalados[].velocidades`: tokens/s por motor, do mais rápido ao mais lento.

## 2. Carga fria (137 s pelo Ignis contra 76 s direto)

**O que foi medido aqui**, com o llama-server oficial b12271 no processador e
o Qwen3 0.6B:

| | 1ª vez | Depois |
|---|---|---|
| llama-server direto | 23,5 s | 1,5–1,6 s |
| Pelo Ignis | — | 1,9–2,1 s |

- **O que o Ignis acrescenta:** cerca de 0,4 s (abrir o processo e checar quando ficou pronto).
- **De onde vem a diferença grande:** da primeira carga, quando o arquivo ainda não está na memória do sistema.
- **Na RX 580 há mais um motivo:** a primeira vez que um build novo do Vulkan roda, o driver da AMD prepara as rotinas da placa.
  - O Ignis usa o b12271 e o Valkode o b11936: são builds diferentes, cada um com essa preparação inicial.
  - A comparação justa é a segunda carga de cada um.

**O Ignis não usa `--mlock`.** Os argumentos são:

```
-m <modelo> --host 127.0.0.1 --port <porta> -c <contexto> -ngl <camadas> --alias <id> --no-webui -np 1
```

O arquivo é mapeado na memória, que é o padrão do llama.cpp.

**Agora cada carregamento fica registrado, etapa por etapa,** em
`GET /motor/estado` → `ultimo_carregamento`:
- `segundos_total` e `segundos_deteccao`;
- `tentativas[]`, com motor, segundos, camadas na placa, contexto e erro.

A janela mostra "Carregou em N s" em Status.

Pedido para a próxima prova: medir **duas cargas seguidas** de cada lado, e
mandar o `ultimo_carregamento` se a diferença continuar.

## 3. ROCm na gfx803

Sem mudança: o Ignis continua avisando que a gfx803 não é suportada pelo ROCm e usa o Vulkan.

## Também nesta versão

- **"Preparar" não baixa de novo um modelo que já está no computador.**
  - **Mesmo arquivo:** o Ignis procura o arquivo do catálogo (mesmo SHA-256) no Ollama e no cache do Hugging Face; o nome do arquivo nessas pastas é o próprio hash.
  - **Outro modelo:** sem o mesmo arquivo, o teste usa um modelo pequeno que já esteja no Ollama, no Hugging Face ou no LM Studio.
- **Nova rota `GET /motor/modelos/computador`:** lista os modelos já baixados por esses três programas.
- **Tempo-limite de carga:** passou de 3 para 10 minutos, para modelos grandes vindos de disco lento.
