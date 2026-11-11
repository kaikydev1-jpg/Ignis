// Janela do Ignis. Fala com o núcleo pelo Tauri (sem rede); fora do Tauri
// (desenvolvimento no navegador), usa a API local em 127.0.0.1.
"use strict";

const tauri = window.__TAURI__ && window.__TAURI__.core;
const $ = (id) => document.getElementById(id);
let estado = null;
let modelos = null;
let secao = "placa";
let avisoFechado = 0;

async function api(method, path, body) {
  let status, corpo;
  if (tauri) {
    const r = await tauri.invoke("api", { method, path, body: body ?? null });
    status = r.status; corpo = r.corpo;
  } else {
    const r = await fetch("http://127.0.0.1:11470" + path, {
      method, headers: { "content-type": "application/json" },
      body: body ? JSON.stringify(body) : undefined,
    });
    status = r.status; corpo = await r.json().catch(() => ({}));
  }
  if (status >= 400) throw new Error((corpo && (corpo.erro || (corpo.error && corpo.error.message))) || ("Erro " + status));
  return corpo;
}

function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
function tamanho(b) {
  if (!b) return "—";
  const gb = b / 1073741824;
  return gb >= 1 ? gb.toFixed(1).replace(".", ",") + " GB" : Math.round(b / 1048576) + " MB";
}
function mb(v) { return v == null ? "—" : (v >= 1024 ? (v / 1024).toFixed(1).replace(".", ",") + " GB" : v + " MB"); }
function tps(v) { return v == null ? "—" : String(v).replace(".", ",") + " tokens/s"; }
function hora(t) { return t ? new Date(t * 1000).toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" }) : "—"; }
function horaCurta(t) { return new Date(t * 1000).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" }); }

function mensagem(texto, nivel) {
  const el = $("aviso");
  $("aviso-texto").textContent = texto;
  el.className = "aviso" + (nivel === "erro" ? " erro-nivel" : nivel === "aviso" ? " aviso-nivel" : "");
}
async function acao(btn, fn) {
  if (btn) btn.disabled = true;
  try { await fn(); } catch (e) { mensagem(e.message, "erro"); }
  finally { if (btn) btn.disabled = false; await atualizar(); }
}
async function copiar(texto) {
  try { await navigator.clipboard.writeText(texto); mensagem("Copiado.", "info"); }
  catch { mensagem("Não consegui copiar. Selecione o texto e use Ctrl+C.", "aviso"); }
}

// ---------- navegação ----------
document.querySelectorAll(".menu button").forEach((b) => {
  b.addEventListener("click", () => {
    secao = b.dataset.secao;
    document.querySelectorAll(".menu button").forEach((x) => x.classList.toggle("ativo", x === b));
    document.querySelectorAll(".secao").forEach((s) => s.classList.toggle("escondido", s.id !== "sec-" + secao));
    atualizar();
  });
});

// ---------- topo, aviso e preparação ----------
const FASES = { pronto: "Rodando", parado: "Rodando (sem modelo)", carregando: "Carregando…", pausado: "Pausado", testando: "Testando motores…", preparando: "Preparando…" };

function renderTopo() {
  const s = estado;
  const pil = $("situacao");
  pil.textContent = s.api.rodando ? FASES[s.fase] || s.fase : "API parada";
  pil.className = "pilula " + (!s.api.rodando ? "erro" : s.fase === "pausado" ? "alerta" : "ok");
  const ultimo = s.avisos[s.avisos.length - 1];
  if (s.api.erro) {
    $("aviso").classList.remove("escondido");
    mensagem(s.api.erro, "erro");
  } else if (ultimo && ultimo.quando > avisoFechado && Date.now() / 1000 - ultimo.quando < 120) {
    $("aviso").classList.remove("escondido");
    mensagem(ultimo.texto, ultimo.nivel);
  }
}
$("aviso-fechar").addEventListener("click", () => {
  $("aviso").classList.add("escondido");
  avisoFechado = Date.now() / 1000;
});

async function renderPreparar() {
  const s = estado;
  const ocupado = s.fase === "preparando" || s.fase === "testando";
  const precisa = s.detectado && s.precisa_preparar;
  $("preparar").classList.toggle("escondido", !precisa && !ocupado);
  if (!precisa && !ocupado) return;
  if (ocupado) {
    $("preparar-texto").textContent = s.fase_texto || "Trabalhando…";
    $("btn-preparar").disabled = true;
    $("preparar-lista").innerHTML = s.tarefas.filter((t) => t.estado === "andamento")
      .map((t) => `<li>${esc(t.nome)} — ${tamanho(t.baixado)} de ${tamanho(t.total)}<div class="barra"><div style="width:${t.total ? (100 * t.baixado / t.total).toFixed(1) : 0}%"></div></div></li>`).join("");
    return;
  }
  const m = await api("GET", "/motor/motores");
  const p = m.preparacao;
  const itens = p.motores.map((x) => `<li>Motor ${esc(x.nome)} — ${x.instalado ? "já instalado" : tamanho(x.tamanho)}</li>`);
  if (p.modelo_teste) itens.push(`<li>Modelo pequeno para o teste (${esc(p.modelo_teste.nome)}) — ${tamanho(p.modelo_teste.tamanho)}</li>`);
  $("preparar-lista").innerHTML = itens.join("");
  $("preparar-texto").textContent = p.total > 0
    ? `O Ignis vai baixar só o que serve para a sua placa (${tamanho(p.total)} no total) e depois testar qual motor é mais rápido. Leva poucos minutos.`
    : "A placa ou o driver mudou. O Ignis vai testar de novo qual motor é mais rápido.";
  $("btn-preparar").disabled = false;
}
$("btn-preparar").addEventListener("click", (e) => acao(e.target, () => api("POST", "/motor/preparar")));

// ---------- PLACA ----------
function renderPlaca() {
  const s = estado, g = s.placa;
  const linhas = [];
  if (g) {
    linhas.push(["Placa", esc(g.nome)], ["Fabricante", esc({ amd: "AMD", nvidia: "NVIDIA", intel: "Intel" }[g.fabricante] || "Outro")]);
    linhas.push(["Memória de vídeo", `${mb(g.vram_total_mb)} no total, ${mb(g.vram_livre_mb)} livre`]);
    linhas.push(["Driver", esc(g.versao_driver || g.driver || "—")]);
    if (g.alvo_gfx) linhas.push(["Modelo do chip (gfx)", esc(g.alvo_gfx)]);
  } else {
    linhas.push(["Placa", s.detectado ? "Nenhuma placa de vídeo encontrada — o Ignis vai usar o processador." : "Detectando…"]);
  }
  if (s.ram) linhas.push(["Memória do computador", `${mb(s.ram.total_mb)} no total, ${mb(s.ram.livre_mb)} livre`]);
  if (s.sistema) linhas.push(["Sistema", esc(s.sistema.nome) + (s.sistema.flatpak ? " (Flatpak)" : s.sistema.appimage ? " (AppImage)" : "")]);
  linhas.push(["Motor em uso", s.motor ? esc(s.motor.nome) : (s.proximo_motor ? `nenhum agora (o próximo será ${esc(s.proximo_motor.nome)})` : "nenhum")]);
  $("placa-dados").innerHTML = linhas.map(([a, b]) => `<dt>${a}</dt><dd>${b}</dd>`).join("");

  const t = s.teste;
  $("teste-situacao").textContent = !t.feito ? "O teste ainda não foi feito." :
    (t.atual ? `Testado em ${hora(t.quando)}.` : `Testado em ${hora(t.quando)} — a placa ou o driver mudou desde então; teste de novo.`);
  $("teste-tabela").innerHTML = t.resultados.map((r) => `<tr>
      <td>${esc(r.nome)}${t.ordem[0] === r.variante ? " <strong>(escolhido)</strong>" : ""}</td>
      <td>${tps(r.leitura_tps)}</td><td>${tps(r.geracao_tps)}</td>
      <td>${r.ok ? '<span class="ok-texto">funcionou</span>' : '<span class="erro-texto">' + esc(r.erro || "falhou") + "</span>"}</td></tr>`).join("")
    || '<tr><td colspan="4" class="nota">Sem resultados ainda.</td></tr>';
  $("btn-testar").disabled = s.fase === "testando" || s.fase === "preparando";
}

let candidatosCache = null;
async function renderCandidatos() {
  const m = await api("GET", "/motor/motores");
  candidatosCache = m;
  $("candidatos").innerHTML = m.candidatos.map((c) => `<li>
      <strong>${esc(c.nome)}</strong> — ${c.disponivel ? (c.instalado ? '<span class="ok-texto">instalado</span>' : "disponível, não baixado (" + tamanho(c.tamanho_download) + ")") : '<span class="alerta-texto">indisponível</span>'}
      <div class="nota">${esc(c.motivo)}</div>
      ${c.dica ? `<div class="nota">${esc(c.dica)} <button class="link copiar-dica" data-texto="${esc(comandoDe(c.dica))}">copiar comando</button></div>` : ""}
    </li>`).join("");
  document.querySelectorAll(".copiar-dica").forEach((b) => b.addEventListener("click", () => copiar(b.dataset.texto)));
}
function comandoDe(dica) {
  const m = dica.match(/(sudo [^—(]+?)(\s+\(|\s+—|$)/);
  return m ? m[1].trim() : dica;
}
$("btn-testar").addEventListener("click", (e) => acao(e.target, () => api("POST", "/motor/testar")));
$("btn-detectar").addEventListener("click", (e) => acao(e.target, () => api("POST", "/motor/detectar")));
$("btn-relatorio").addEventListener("click", async () => {
  try { const r = await api("GET", "/motor/relatorio"); await copiar(JSON.stringify(r, null, 2)); }
  catch (e) { mensagem(e.message, "erro"); }
});

// ---------- MODELOS ----------
const ORIGEM = { "catalogo": "Catálogo", "ollama-referencia": "Ollama (lido no lugar)", "ollama-copia": "Ollama (cópia)", "arquivo": "Arquivo seu" };
const ENCAIXE = { "placa": "placa", "dividido": "dividido", "processador": "processador", "nao-cabe": "nao-cabe" };

async function renderModelos() {
  modelos = await api("GET", "/motor/modelos");
  const inst = modelos.instalados;
  $("instalados-vazio").classList.toggle("escondido", inst.length > 0);
  $("instalados").innerHTML = inst.map((m) => `<tr>
      <td><strong>${esc(m.nome)}</strong>${m.carregado ? ' <span class="ok-texto">(carregado)</span>' : ""}${m.padrao ? " <span class='nota'>(padrão)</span>" : ""}${m.embeddings ? " <span class='nota'>(embeddings)</span>" : ""}
        ${m.existe ? "" : '<div class="erro-texto">O arquivo sumiu do disco.</div>'}<div class="nota">${esc(m.id)}</div></td>
      <td>${tamanho(m.tamanho)}</td>
      <td>${esc(ORIGEM[m.origem.tipo] || m.origem.tipo)}</td>
      <td><span class="encaixe ${ENCAIXE[m.encaixe]}">${esc(m.rotulo)}</span></td>
      <td class="botoes">
        ${m.carregado ? '<button class="pequeno" data-acao="descarregar">Descarregar</button>' : `<button class="pequeno" data-acao="carregar" data-id="${esc(m.id)}" ${m.existe ? "" : "disabled"}>Carregar</button>`}
        ${m.padrao || m.embeddings ? "" : `<button class="pequeno" data-acao="padrao" data-id="${esc(m.id)}">Usar como padrão</button>`}
        <button class="pequeno" data-acao="remover" data-id="${esc(m.id)}" data-nome="${esc(m.nome)}" data-origem="${esc(m.origem.tipo)}">Remover</button>
      </td></tr>`).join("");

  const tarefas = modelos.tarefas.filter((t) => t.estado !== "concluida");
  $("cartao-tarefas").classList.toggle("escondido", tarefas.length === 0);
  $("tarefas").innerHTML = tarefas.map((t) => {
    const pct = t.total ? (100 * t.baixado / t.total) : 0;
    const situ = t.estado === "andamento" ? `${tamanho(t.baixado)} de ${tamanho(t.total)}` :
      t.estado === "pausada" ? "Pausado — o que já foi baixado está guardado." : '<span class="erro-texto">' + esc(t.erro || "erro") + "</span>";
    const id = t.id.startsWith("modelo:") ? t.id.slice(7) : null;
    return `<li><strong>${esc(t.nome)}</strong> — ${situ}<div class="barra"><div style="width:${pct.toFixed(1)}%"></div></div>
      ${t.estado === "andamento" ? `<button class="pequeno" data-acao="cancelar" data-id="${esc(t.id)}">Cancelar</button>` : ""}
      ${t.estado !== "andamento" && id ? `<button class="pequeno" data-acao="baixar" data-id="${esc(id)}">Retomar</button> <button class="pequeno" data-acao="remover" data-id="${esc(id)}" data-nome="${esc(t.nome)}">Apagar o que foi baixado</button>` : ""}</li>`;
  }).join("");

  $("titulo-recomendados").textContent = estado && estado.placa ? "Recomendados para a sua placa" : "Recomendados para este computador";
  const todos = $("mostrar-todos").checked;
  const cat = modelos.catalogo.filter((c) => todos || c.recomendado || c.instalado);
  $("catalogo").innerHTML = cat.map((c) => {
    const t = c.tarefa && c.tarefa.estado === "andamento";
    const botao = c.instalado ? '<span class="ok-texto">Instalado</span>' :
      t ? `<button class="pequeno" data-acao="cancelar" data-id="modelo:${esc(c.id)}">Cancelar download</button>` :
      `<button class="pequeno primario" data-acao="baixar" data-id="${esc(c.id)}" data-encaixe="${esc(c.encaixe)}" data-aviso="${esc(c.aviso || "")}">${c.parcial > 0 ? "Retomar download" : "Baixar"}</button>`;
    return `<div class="modelo">
      <h3>${esc(c.nome)}</h3>
      <div class="meta">${tamanho(c.tamanho)} · ${esc(c.quantizacao)} · ${esc({ conversa: "conversa", codigo: "código", embeddings: "embeddings" }[c.tipo] || c.tipo)}</div>
      <div>${esc(c.uso)}</div>
      <div class="encaixe ${ENCAIXE[c.encaixe]}">${esc(c.rotulo)}</div>
      <div class="meta">Licença: <button class="link" data-link="${esc(c.licenca_url)}">${esc(c.licenca)}</button></div>
      ${t ? `<div class="barra"><div style="width:${(100 * c.tarefa.baixado / c.tamanho).toFixed(1)}%"></div></div>` : ""}
      <div class="acoes">${botao}</div></div>`;
  }).join("") || '<p class="nota">Nenhum modelo do catálogo roda bem nesta placa. Marque "Mostrar todos" para ver os outros.</p>';
}

document.addEventListener("click", async (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  if (b.dataset.link) {
    if (tauri) tauri.invoke("abrir_link", { url: b.dataset.link }).catch((err) => mensagem(String(err), "erro"));
    else window.open(b.dataset.link, "_blank");
    return;
  }
  const a = b.dataset.acao;
  if (!a) return;
  const id = b.dataset.id;
  if (a === "baixar") {
    if (b.dataset.encaixe === "nao-cabe" && !confirm((b.dataset.aviso || "Este modelo não cabe neste computador.") + "\n\nBaixar mesmo assim?")) return;
    if (b.dataset.encaixe === "dividido" && !confirm((b.dataset.aviso || "Este modelo não cabe inteiro na placa.") + "\n\nBaixar mesmo assim?")) return;
    return acao(b, () => api("POST", "/motor/modelos/baixar", { id }));
  }
  if (a === "cancelar") return acao(b, () => api("POST", "/motor/tarefas/cancelar", { id }));
  if (a === "carregar") return acao(b, async () => { mensagem("Carregando o modelo…", "info"); await api("POST", "/motor/modelos/carregar", { id }); });
  if (a === "descarregar") return acao(b, () => api("POST", "/motor/modelos/descarregar"));
  if (a === "padrao") return acao(b, () => api("POST", "/motor/modelos/padrao", { id }));
  if (a === "remover") {
    const ref = b.dataset.origem === "ollama-referencia" || b.dataset.origem === "arquivo";
    const msg = ref ? `Tirar "${b.dataset.nome}" da lista? O arquivo original não será apagado.` : `Apagar "${b.dataset.nome}" do disco?`;
    if (!confirm(msg)) return;
    return acao(b, async () => { const r = await api("POST", "/motor/modelos/remover", { id }); mensagem(r.mensagem, "info"); });
  }
  if (a === "ollama-usar" || a === "ollama-copiar") {
    const modo = a === "ollama-usar" ? "usar" : "copiar";
    return acao(b, async () => { await api("POST", "/motor/modelos/importar", { referencia: b.dataset.ref, modo }); await procurarOllama(); });
  }
});

$("mostrar-todos").addEventListener("change", renderModelos);

async function procurarOllama() {
  const r = await api("GET", "/motor/modelos/ollama");
  $("ollama-tabela").classList.toggle("escondido", r.modelos.length === 0);
  $("ollama-nota").textContent = r.modelos.length ? "" : "Nenhum modelo do Ollama encontrado. Pastas procuradas: " + r.pastas_procuradas.join(", ");
  $("ollama-lista").innerHTML = r.modelos.map((m) => `<tr><td>${esc(m.referencia)}</td><td>${tamanho(m.tamanho)}</td>
    <td class="botoes">${m.ja_importado ? '<span class="ok-texto">Já está no Ignis</span>' :
      `<button class="pequeno" data-acao="ollama-usar" data-ref="${esc(m.referencia)}">Usar daqui</button> <button class="pequeno" data-acao="ollama-copiar" data-ref="${esc(m.referencia)}">Copiar</button>`}</td></tr>`).join("");
}
$("btn-ollama").addEventListener("click", (e) => acao(e.target, procurarOllama));
$("btn-arquivo").addEventListener("click", (e) => acao(e.target, async () => {
  const caminho = $("arquivo-gguf").value.trim();
  if (!caminho) throw new Error("Escreva o caminho do arquivo .gguf.");
  await api("POST", "/motor/modelos/importar", { caminho });
  $("arquivo-gguf").value = "";
  mensagem("Modelo adicionado.", "info");
}));
$("btn-pasta-modelos").addEventListener("click", () => tauri && tauri.invoke("abrir_pasta", { qual: "modelos" }));

// ---------- MOTOR ----------
let configCarregada = false;
async function renderMotor() {
  const m = candidatosCache || await api("GET", "/motor/motores");
  const forcado = m.forcado;
  const opcoes = [`<label><input type="radio" name="motor" value="auto" ${!forcado ? "checked" : ""}> <span><strong>Automático</strong> (recomendado)</span></label>`];
  for (const c of m.candidatos) {
    opcoes.push(`<label class="${c.disponivel ? "" : "indisponivel"}"><input type="radio" name="motor" value="${esc(c.variante)}" ${forcado === c.variante ? "checked" : ""} ${c.disponivel ? "" : "disabled"}>
      <span><strong>${esc(c.nome)}</strong> — ${esc(c.motivo)}</span></label>`);
  }
  $("escolha-motor").innerHTML = opcoes.join("");
  document.querySelectorAll('input[name="motor"]').forEach((r) => r.addEventListener("change", () =>
    acao(null, () => api("POST", "/motor/motores/forcar", { variante: r.value === "auto" ? null : r.value }))));

  const v = await api("GET", "/motor/versao");
  const instalados = m.candidatos.filter((c) => c.instalado).map((c) => c.nome).join(", ") || "nenhum";
  $("versoes").innerHTML = `<dt>Ignis</dt><dd>${esc(v.app)} (API ${v.api})</dd>
    <dt>llama.cpp</dt><dd>${esc(v.llama_cpp.versao)} (commit ${esc(v.llama_cpp.commit)})</dd>
    <dt>Motores baixados</dt><dd>${esc(instalados)}</dd>`;

  const att = estado.atualizacao;
  if (att) {
    const partes = [];
    if (att.app) partes.push(`<p>Nova versão do aplicativo: <strong>${esc(att.app.versao)}</strong>. <button class="link" data-link="${esc(att.app.pagina)}">Abrir página de download</button></p>`);
    if (att.motor) partes.push(`<p>Novo motor: <strong>llama.cpp ${esc(att.motor)}</strong>. <button class="pequeno" id="btn-att-motor">Atualizar motor</button></p>`);
    if (att.catalogo) partes.push(`<p>Novo catálogo de modelos (versão ${att.catalogo}). <button class="pequeno" id="btn-att-catalogo">Atualizar catálogo</button></p>`);
    $("atualizacao-info").innerHTML = partes.join("") || `<p class="nota">Tudo em dia (verificado em ${hora(att.verificado_em)}).</p>`;
    const bm = $("btn-att-motor"), bc = $("btn-att-catalogo");
    if (bm) bm.onclick = (e) => acao(e.target, () => api("POST", "/motor/atualizacoes/aplicar", { item: "motor" }));
    if (bc) bc.onclick = (e) => acao(e.target, () => api("POST", "/motor/atualizacoes/aplicar", { item: "catalogo" }));
  }

  if (!configCarregada) {
    const c = await api("GET", "/motor/config");
    $("cfg-porta").value = c.porta;
    $("cfg-descarregar").value = Math.round(c.descarregar_apos_seg / 60);
    $("cfg-contexto").value = String(c.contexto_preferido);
    $("cfg-autostart").checked = c.abrir_com_computador;
    $("cfg-att-auto").checked = c.procurar_atualizacoes_sozinho;
    $("cfg-proprio").value = c.llama_server_proprio || "";
    configCarregada = true;
  }
  const linux = estado.sistema && !["windows", "mac"].includes(estado.sistema.familia);
  $("dica-gnome").classList.toggle("escondido", !linux);
}
$("btn-salvar-cfg").addEventListener("click", (e) => acao(e.target, async () => {
  await api("POST", "/motor/config", {
    porta: Number($("cfg-porta").value),
    descarregar_apos_min: Number($("cfg-descarregar").value),
    contexto_preferido: Number($("cfg-contexto").value),
    abrir_com_computador: $("cfg-autostart").checked,
    llama_server_proprio: $("cfg-proprio").value,
  });
  configCarregada = false;
  mensagem("Configurações salvas.", "info");
}));
$("cfg-att-auto").addEventListener("change", (e) => acao(null, () => api("POST", "/motor/config", { procurar_atualizacoes_sozinho: e.target.checked })));
$("btn-procurar-att").addEventListener("click", (e) => acao(e.target, () => api("POST", "/motor/atualizacoes/procurar")));

// ---------- STATUS ----------
async function renderStatus() {
  const s = estado;
  $("status-titulo").textContent = !s.api.rodando ? "Parado" : s.pausado ? "Pausado" : (s.modelo ? "Rodando" : "Rodando, sem modelo carregado");
  $("status-texto").textContent = s.api.erro || s.fase_texto || (s.pausado ? "Novos pedidos são recusados até você retomar." : "");
  const d = [];
  d.push(["Modelo carregado", s.modelo ? esc(s.modelo.nome) : "nenhum"]);
  if (s.modelo) {
    d.push(["Motor", esc(s.motor.nome)]);
    d.push(["Contexto", s.modelo.contexto.toLocaleString("pt-BR") + " tokens"]);
    const naPlaca = s.motor.backend === "cpu" || s.modelo.camadas_na_placa === 0 ? "nenhuma — rodando só no processador"
      : s.modelo.todas_na_placa ? "todas as camadas" : s.modelo.camadas_na_placa + " camadas (o resto no processador)";
    d.push(["Na placa", naPlaca]);
  }
  d.push(["Velocidade (último pedido)", s.velocidade ? `lendo ${tps(s.velocidade.leitura_tps)}, escrevendo ${tps(s.velocidade.geracao_tps)}` : "—"]);
  d.push(["Conversas em andamento", String(s.requisicoes_ativas)]);
  d.push(["Porta", String(s.porta) + " (só neste computador)"]);
  $("status-dados").innerHTML = d.map(([a, b]) => `<dt>${a}</dt><dd>${b}</dd>`).join("");
  $("btn-pausar").textContent = s.pausado ? "Retomar" : "Pausar";
  $("btn-descarregar").disabled = !s.modelo;
  $("btn-parar").disabled = s.requisicoes_ativas === 0;
  $("endereco").value = s.api.endereco;
  $("avisos").innerHTML = s.avisos.slice().reverse().map((a) =>
    `<li class="${a.nivel === "erro" ? "erro" : a.nivel === "aviso" ? "aviso-nivel" : ""}"><span class="quando">${horaCurta(a.quando)}</span>${esc(a.texto)}</li>`).join("")
    || '<li class="nota">Nenhum aviso.</li>';
  if ($("registro-det").open) {
    const r = await api("GET", "/motor/registro");
    $("registro").textContent = r.linhas.join("\n") || "(nenhum motor rodando)";
  }
}
$("btn-pausar").addEventListener("click", (e) => acao(e.target, () => api("POST", estado.pausado ? "/motor/retomar" : "/motor/pausar")));
$("btn-descarregar").addEventListener("click", (e) => acao(e.target, () => api("POST", "/motor/modelos/descarregar")));
$("btn-parar").addEventListener("click", (e) => acao(e.target, () => api("POST", "/motor/parar", {})));
$("btn-copiar-endereco").addEventListener("click", () => copiar($("endereco").value));
$("btn-pasta-registros").addEventListener("click", () => tauri && tauri.invoke("abrir_pasta", { qual: "registros" }));
$("registro-det").addEventListener("toggle", () => renderStatus());

// ---------- ciclo ----------
let ocupado = false;
async function atualizar() {
  if (ocupado) return;
  ocupado = true;
  try {
    estado = await api("GET", "/motor/estado");
    renderTopo();
    await renderPreparar();
    if (secao === "placa") { renderPlaca(); await renderCandidatos(); }
    if (secao === "modelos") await renderModelos();
    if (secao === "motor") { candidatosCache = null; await renderMotor(); }
    if (secao === "status") await renderStatus();
  } catch (e) {
    $("situacao").textContent = "Sem conexão com o núcleo";
    $("situacao").className = "pilula erro";
  } finally {
    ocupado = false;
  }
}

// Só atualiza com a janela visível (quase zero de processador escondido).
setInterval(() => { if (document.visibilityState === "visible") atualizar(); }, 1500);
document.addEventListener("visibilitychange", () => { if (document.visibilityState === "visible") atualizar(); });
atualizar();
