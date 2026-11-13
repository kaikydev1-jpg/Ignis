// Janela do Ignis. Fala com o núcleo pelo Tauri (sem rede); fora do Tauri
// (desenvolvimento no navegador), usa a API local em 127.0.0.1.
"use strict";

const tauri = window.__TAURI__ && window.__TAURI__.core;
const $ = (id) => document.getElementById(id);
let estado = null;
let modelos = null;
let motores = null;
let motoresEm = 0;
let secao = "placa";
let avisoFechado = 0;
let filtroTipo = "todos";
let exemplo = "curl";
let hfRepoAberto = null;

// ---------- comportamento de aplicativo (não de página) ----------
document.addEventListener("contextmenu", (e) => {
  if (!e.target.closest("input, textarea, pre, code")) e.preventDefault();
});
document.addEventListener("keydown", (e) => {
  const k = e.key.toLowerCase();
  const ctrl = e.ctrlKey || e.metaKey;
  if (k === "f5" || (ctrl && ["r", "p", "u", "+", "-", "=", "0"].includes(k)) || (ctrl && e.shiftKey && k === "i")) e.preventDefault();
});
document.addEventListener("wheel", (e) => { if (e.ctrlKey) e.preventDefault(); }, { passive: false });
document.addEventListener("dragstart", (e) => { if (!e.target.closest("input, textarea")) e.preventDefault(); });

// ---------- barra de título integrada ----------
// A janela não usa a barra do sistema: o topo do próprio Ignis arrasta a
// janela (duplo clique maximiza) e tem os botões minimizar, maximizar e fechar.
document.documentElement.dataset.so = /windows/i.test(navigator.userAgent) ? "windows" : /mac os/i.test(navigator.userAgent) ? "mac" : "linux";
// Desenhos dos botões em cada estilo (o do maximizar tem também o "restaurar").
const ICONES_JANELA = {
  windows: { caixa: 10,
    minimize: '<path d="M0 5.5h10"/>',
    maximize: '<rect x=".5" y=".5" width="9" height="9" rx="1"/>',
    restaurar: '<path d="M2.5 2.5V1.5a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1"/><rect x=".5" y="2.5" width="7" height="7" rx="1"/>',
    close: '<path d="M.5.5l9 9M9.5.5l-9 9"/>' },
  gnome: { caixa: 16,
    minimize: '<path d="M4.5 11.5h7"/>',
    maximize: '<rect x="4" y="4" width="8" height="8" rx="1.5"/>',
    restaurar: '<rect x="3.5" y="6" width="6.5" height="6.5" rx="1.2"/><path d="M6 4h5a1 1 0 0 1 1 1v5"/>',
    close: '<path d="M4.5 4.5l7 7M11.5 4.5l-7 7"/>' },
  kde: { caixa: 16,
    minimize: '<path d="M4 6.5l4 4 4-4"/>',
    maximize: '<path d="M4 10l4-4 4 4"/>',
    restaurar: '<path d="M8 3.5l4.5 4.5L8 12.5 3.5 8z"/>',
    close: '<path d="M4.5 4.5l7 7M11.5 4.5l-7 7"/>' },
  // Bolinhas do macOS: × e − finos, e o verde com os dois triângulos (tela cheia).
  mac: { caixa: 12,
    minimize: '<path d="M2.8 6h6.4"/>',
    maximize: '<path d="M3.4 3.3h3.3L3.4 6.6zM8.6 8.7H5.3l3.3-3.3z" fill="currentColor" stroke="none"/>',
    restaurar: '<path d="M5.7 5.7H2.9l2.8-2.8zM6.3 6.3h2.8L6.3 9.1z" fill="currentColor" stroke="none"/>',
    close: '<path d="M3.7 3.7l4.6 4.6M8.3 3.7L3.7 8.3"/>' },
};
ICONES_JANELA.matsu = ICONES_JANELA.mac; // Matsu OS: mesmos desenhos, cores exatas do sistema (style.css)
const NOMES_JANELA = { minimize: "Minimizar", maximize: "Maximizar", close: "Fechar (o Ignis continua perto do relógio)" };

(function barraDeTitulo() {
  const w = window.__TAURI__ && window.__TAURI__.window && window.__TAURI__.window.getCurrentWindow();
  if (!w) return; // fora do Tauri (navegador) não há janela para controlar
  const raiz = document.documentElement;
  const svg = (estilo, d) => { const c = ICONES_JANELA[estilo].caixa; return `<svg viewBox="0 0 ${c} ${c}" aria-hidden="true">${d}</svg>`; };
  const marcar = async () => {
    const max = await w.isMaximized().catch(() => false);
    raiz.toggleAttribute("data-maximizada", max);
    for (const b of document.querySelectorAll('[data-janela="maximize"]')) {
      const nome = max ? "Restaurar" : "Maximizar";
      b.title = nome;
      b.setAttribute("aria-label", nome);
    }
  };
  // Monta os botões no estilo, lado e ordem que o sistema usa (ou a escolha da pessoa).
  window.montarBotoesJanela = async (preferido) => {
    const b = await tauri.invoke("botoes_janela", { preferido: preferido || "auto" });
    const ic = ICONES_JANELA[b.estilo] || ICONES_JANELA.windows;
    raiz.dataset.botoes = b.estilo;
    raiz.toggleAttribute("data-botoes-esquerda", b.esquerda.length > 0);
    for (const [lado, lista] of [["esquerda", b.esquerda], ["direita", b.direita]]) {
      const caixa = $("controles-" + lado);
      caixa.innerHTML = lista.map((nome) => {
        const desenho = nome === "maximize"
          ? `<span class="i-max">${svg(b.estilo, ic.maximize)}</span><span class="i-rest">${svg(b.estilo, ic.restaurar)}</span>`
          : svg(b.estilo, ic[nome]);
        return `<button data-janela="${nome}" class="jan-${nome}" title="${NOMES_JANELA[nome]}" aria-label="${NOMES_JANELA[nome]}" tabindex="-1">${desenho}</button>`;
      }).join("");
      caixa.hidden = lista.length === 0;
    }
    marcar();
  };
  document.addEventListener("click", (e) => {
    const b = e.target.closest("[data-janela]");
    if (!b) return;
    if (b.dataset.janela === "minimize") w.minimize();
    else if (b.dataset.janela === "maximize") w.toggleMaximize();
    else if (b.dataset.janela === "close") w.close();
  });
  api("GET", "/motor/config")
    .then((c) => montarBotoesJanela(c.botoes_janela))
    .catch(() => montarBotoesJanela("auto"));
  marcar();
  w.onResized(marcar);

  // Arrastar e duplo clique. Áreas com data-arrastar (só o próprio elemento) ou
  // data-arrastar="tudo" (também o que está dentro); botões e campos não arrastam.
  // O arraste só começa quando o mouse se mexe com o botão apertado: assim o
  // segundo clique de um duplo clique não se perde (no Linux, começar a arrastar
  // no primeiro clique entrega o mouse ao sistema e o duplo clique não chega).
  const clicavel = "a, button, input, select, textarea, label, summary, [contenteditable], [role=button], [role=tab], [tabindex]:not([tabindex='-1'])";
  const podeArrastar = (alvo) => {
    if (!(alvo instanceof Element) || alvo.closest(clicavel)) return false;
    const area = alvo.closest("[data-arrastar]");
    return !!area && (area === alvo || area.dataset.arrastar === "tudo");
  };
  let inicio = null;
  document.addEventListener("mousedown", (e) => {
    inicio = null;
    if (e.button !== 0 || !podeArrastar(e.target)) return;
    e.preventDefault(); // sem seleção de texto
    if (e.detail === 2) w.toggleMaximize();
    else inicio = { x: e.screenX, y: e.screenY };
  });
  document.addEventListener("mousemove", (e) => {
    if (!inicio) return;
    if (!(e.buttons & 1)) { inicio = null; return; }
    if (Math.abs(e.screenX - inicio.x) + Math.abs(e.screenY - inicio.y) >= 3) {
      inicio = null;
      w.startDragging();
    }
  });
  document.addEventListener("mouseup", () => { inicio = null; });
  window.addEventListener("blur", () => raiz.toggleAttribute("data-inativa", true));
  window.addEventListener("focus", () => raiz.toggleAttribute("data-inativa", false));
})();

// ---------- utilidades ----------
async function api(method, path, body) {
  let status, corpo;
  if (tauri) {
    const r = await tauri.invoke("api", { method, path, body: body ?? null });
    status = r.status; corpo = r.corpo;
  } else {
    const r = await fetch("http://127.0.0.1:11470" + path, {
      method, headers: { "content-type": "application/json" }, body: body ? JSON.stringify(body) : undefined,
    });
    status = r.status; corpo = await r.json().catch(() => ({}));
  }
  if (status >= 400) throw new Error((corpo && (corpo.erro || (corpo.error && corpo.error.message))) || ("Erro " + status));
  return corpo;
}
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const virgula = (n, casas = 1) => n.toFixed(casas).replace(".", ",");
function tamanho(b) {
  if (!b) return "—";
  const gb = b / 1073741824;
  return gb >= 1 ? virgula(gb) + " GB" : Math.round(b / 1048576) + " MB";
}
const gbDeMb = (v) => (v == null ? "—" : v >= 1024 ? virgula(v / 1024) + " GB" : Math.round(v) + " MB");
const tps = (v) => (v == null ? "—" : Number(v).toLocaleString("pt-BR", { minimumFractionDigits: 1, maximumFractionDigits: 1 }));
const hora = (t) => (t ? new Date(t * 1000).toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" }) : "—");
const horaCurta = (t) => new Date(t * 1000).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });
function milhares(n) {
  if (n >= 1e6) return virgula(n / 1e6) + " mi";
  if (n >= 1e3) return virgula(n / 1e3) + " mil";
  return String(n);
}
// só troca o HTML quando muda (não pisca e não perde o foco)
function pinta(el, html) {
  if (el._html !== html) { el.innerHTML = html; el._html = html; }
}
function mensagem(texto, nivel) {
  const el = $("aviso");
  $("aviso-texto").textContent = texto;
  el.className = "aviso" + (nivel === "erro" ? " erro-nivel" : nivel === "aviso" ? " aviso-nivel" : "");
  el.hidden = false;
}
async function acao(btn, fn) {
  if (btn) btn.disabled = true;
  try { await fn(); } catch (e) { mensagem(e.message, "erro"); }
  finally { if (btn) btn.disabled = false; await atualizar(true); }
}
async function copiar(texto, rotulo) {
  try { await navigator.clipboard.writeText(texto); mensagem(rotulo || "Copiado.", "info"); }
  catch { mensagem("Não consegui copiar. Selecione o texto e use Ctrl+C.", "aviso"); }
}
function abrirLink(url) {
  if (tauri) tauri.invoke("abrir_link", { url }).catch((e) => mensagem(String(e), "erro"));
  else window.open(url, "_blank");
}
const ICONES = {
  conversa: '<svg viewBox="0 0 24 24"><path d="M4 5h16v11H9l-5 4V5Z"/></svg>',
  codigo: '<svg viewBox="0 0 24 24"><path d="m9 8-4 4 4 4M15 8l4 4-4 4"/></svg>',
  embeddings: '<svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="6"/><path d="m20 20-4.5-4.5"/></svg>',
  baixando: '<svg viewBox="0 0 24 24"><path d="M12 4v11M7 10l5 5 5-5M5 20h14"/></svg>',
  motor: '<svg viewBox="0 0 24 24"><rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3"/></svg>',
  repo: '<svg viewBox="0 0 24 24"><path d="M12 3 3 7.5l9 4.5 9-4.5L12 3Z"/><path d="m3 12 9 4.5 9-4.5"/></svg>',
  ollama: '<svg viewBox="0 0 24 24"><path d="M5 12h14M12 5l7 7-7 7"/></svg>',
};
const NOME_FABRICANTE = { amd: "AMD", nvidia: "NVIDIA", intel: "Intel" };
const ORIGEM = { "catalogo": "do catálogo do Ignis", "hugging-face": "do Hugging Face", "ollama-referencia": "lido da pasta do Ollama", "ollama-copia": "copiado do Ollama", "arquivo": "arquivo seu" };
const TIPO = { conversa: "Conversa", codigo: "Código", embeddings: "Busca por significado" };

// ---------- navegação ----------
function irPara(nome) {
  secao = nome;
  document.querySelectorAll(".menu button").forEach((x) => x.classList.toggle("ativo", x.dataset.secao === nome));
  document.querySelectorAll(".secao").forEach((s) => (s.hidden = s.id !== "sec-" + nome));
  $("principal").scrollTop = 0;
  atualizar(true);
}
document.querySelectorAll(".menu button").forEach((b) => b.addEventListener("click", () => irPara(b.dataset.secao)));

// ---------- topo, rodapé e aviso ----------
const FASES = { pronto: "Rodando", parado: "Pronto", carregando: "Carregando modelo", pausado: "Pausado", testando: "Testando motores", preparando: "Preparando" };
function renderRodape() {
  const s = estado;
  const ponto = $("rodape-ponto");
  const trabalhando = ["carregando", "testando", "preparando"].includes(s.fase) || s.requisicoes_ativas > 0;
  ponto.className = "ponto " + (!s.api.rodando ? "erro" : s.fase === "pausado" ? "alerta" : trabalhando ? "trabalhando" : "ok");
  $("rodape-titulo").textContent = !s.api.rodando ? "API parada" : FASES[s.fase] || s.fase;
  $("rodape-sub").textContent = s.modelo ? s.modelo.nome : "porta " + s.porta;
  const ultimo = s.avisos[s.avisos.length - 1];
  if (s.api.erro) mensagem(s.api.erro, "erro");
  else if (ultimo && ultimo.quando > avisoFechado && Date.now() / 1000 - ultimo.quando < 90 && $("aviso-texto").textContent !== ultimo.texto) mensagem(ultimo.texto, ultimo.nivel);
}
$("aviso-fechar").addEventListener("click", () => { $("aviso").hidden = true; avisoFechado = Date.now() / 1000; });

// ---------- primeiro uso ----------
function guiaOculto() { try { return localStorage.getItem("ignis-guia-oculto") === "1"; } catch { return false; } }
$("guia-ocultar").addEventListener("click", () => { try { localStorage.setItem("ignis-guia-oculto", "1"); } catch {} $("guia").hidden = true; });

function renderGuia() {
  const s = estado;
  const temModelo = modelos ? modelos.instalados.some((m) => m.existe && !m.embeddings) : true;
  const preparado = s.detectado && !s.precisa_preparar;
  const ocupado = s.fase === "preparando" || s.fase === "testando";
  const mostrar = s.detectado && (!preparado || !temModelo || ocupado) && !guiaOculto();
  $("guia").hidden = !mostrar;
  if (!mostrar) return;
  const placa = s.placa ? esc(s.placa.nome) : "Sem placa de vídeo: o Ignis vai usar o processador";
  let p2sub = "Baixa só o motor que serve para a sua placa e mede qual é o mais rápido.";
  let p2extra = "";
  if (ocupado) {
    p2sub = esc(s.fase_texto || "Trabalhando…");
    const t = s.tarefas.find((x) => x.estado === "andamento");
    if (t && t.total) p2extra = `<div class="progresso barra-mini"><div style="width:${(100 * t.baixado / t.total).toFixed(1)}%"></div></div>`;
  } else if (motores && !preparado) {
    p2sub += ` Total: ${tamanho(motores.preparacao.total)}.`;
  }
  const passos = [
    { feito: s.detectado, titulo: "Conhecer a sua placa", sub: placa, botao: "" },
    { feito: preparado && !ocupado, titulo: "Preparar o motor", sub: p2sub + p2extra,
      botao: preparado || ocupado ? "" : '<button class="botao primario" data-acao="preparar">Preparar o Ignis</button>' },
    { feito: temModelo && preparado, titulo: "Escolher um modelo", sub: temModelo ? "Pronto. Você já pode usar o Ignis em outros programas." : "Escolha um modelo que roda bem na sua placa.",
      botao: temModelo || !preparado ? "" : '<button class="botao" data-acao="ir-modelos">Ver modelos</button>' },
  ];
  let atualMarcado = false;
  pinta($("guia-passos"), passos.map((p, i) => {
    let cls = p.feito ? "feito" : "";
    if (!p.feito && !atualMarcado) { cls = "atual"; atualMarcado = true; }
    return `<li class="passo ${cls}"><span class="numero">${p.feito ? "✓" : i + 1}</span>
      <div><div class="titulo">${p.titulo}</div><div class="sub">${p.sub}</div></div><div>${p.botao}</div></li>`;
  }).join(""));
}

// ---------- barra de memória ----------
function barraMemoria({ total, livre, modelo, rotuloTotal }) {
  if (!total) return "";
  livre = Math.max(0, Math.min(livre ?? total, total));
  const usadoModelo = Math.max(0, Math.min(modelo || 0, livre));
  const outros = Math.max(0, total - livre);
  const sobra = Math.max(0, livre - usadoModelo);
  const pct = (v) => (100 * v / total).toFixed(2) + "%";
  return `<div class="memoria">
    <div class="memoria-topo"><strong>${rotuloTotal}</strong><span>${gbDeMb(sobra)} livres</span></div>
    <div class="barra" role="img" aria-label="Memória: ${gbDeMb(outros)} outros programas, ${gbDeMb(usadoModelo)} modelo, ${gbDeMb(sobra)} livre">
      <span class="seg seg-outros" style="width:${pct(outros)}"></span><span class="seg seg-modelo" style="width:${pct(usadoModelo)}"></span>
    </div>
    <div class="legenda">
      <span><i class="cor-outros"></i>Outros programas ${gbDeMb(outros)}</span>
      ${usadoModelo ? `<span><i class="cor-modelo"></i>Modelo do Ignis ${gbDeMb(usadoModelo)}</span>` : ""}
      <span><i class="cor-livre"></i>Livre ${gbDeMb(sobra)}</span>
    </div></div>`;
}

// ---------- PLACA ----------
function renderPlaca() {
  const s = estado, g = s.placa;
  const sistema = s.sistema ? esc(s.sistema.nome) + (s.sistema.flatpak ? " (Flatpak)" : s.sistema.appimage ? " (AppImage)" : "") : "—";
  const motorUso = s.motor ? esc(s.motor.nome) : s.proximo_motor ? "Nenhum agora" : "Nenhum instalado";
  const motorSub = !s.motor && s.proximo_motor ? `<dd class="sub">Próximo: ${esc(s.proximo_motor.nome)}</dd>` : "";
  let html;
  if (!s.detectado) {
    html = `<div class="placa-topo"><div><h2>Detectando a placa…</h2></div></div>`;
  } else if (!g) {
    const ram = s.ram || {};
    const naRam = s.modelo && s.motor && s.motor.backend === "cpu" ? s.modelo.memoria_ram_mb : 0;
    html = `<div class="placa-topo"><div><h2>Sem placa de vídeo dedicada</h2><p>O Ignis usa o processador. Funciona em qualquer computador, só que mais devagar.</p></div><span class="fabricante cpu">CPU</span></div>
      ${barraMemoria({ total: ram.total_mb, livre: ram.livre_mb, modelo: naRam, rotuloTotal: gbDeMb(ram.total_mb) + " de memória do computador" })}
      <dl class="fatos"><div><dt>Motor do Ignis em uso</dt><dd>${motorUso}</dd>${motorSub}</div><div><dt>Sistema</dt><dd>${sistema}</dd></div></dl>`;
  } else {
    const fab = g.fabricante in NOME_FABRICANTE ? g.fabricante : "cpu";
    const naPlaca = s.modelo && s.motor && s.motor.backend !== "cpu" ? s.modelo.memoria_placa_mb : 0;
    html = `<div class="placa-topo"><div><h2>${esc(g.nome)}</h2><p>${g.integrada ? "Placa integrada" : "Placa de vídeo dedicada"}</p></div>
        <span class="fabricante ${fab}">${NOME_FABRICANTE[g.fabricante] || "Outra"}</span></div>
      ${barraMemoria({ total: g.vram_total_mb, livre: g.vram_livre_mb, modelo: naPlaca, rotuloTotal: gbDeMb(g.vram_total_mb) + " de memória de vídeo" })}
      <dl class="fatos">
        <div><dt>Motor do Ignis em uso</dt><dd>${motorUso}</dd>${motorSub}</div>
        <div><dt>Driver</dt><dd>${esc(g.versao_driver || g.driver || "—")}</dd></div>
        ${g.alvo_gfx ? `<div><dt>Chip</dt><dd>${esc(g.alvo_gfx)}</dd></div>` : ""}
        <div><dt>Sistema</dt><dd>${sistema}</dd></div>
      </dl>`;
  }
  html += `<div class="acoes"><button class="botao" data-acao="detectar">Detectar de novo</button><button class="botao texto" data-acao="relatorio">Copiar relatório</button></div>`;
  pinta($("placa"), html);

  const t = s.teste;
  const res = t.resultados || [];
  const max = Math.max(1, ...res.filter((r) => r.ok).map((r) => r.geracao_tps || 0));
  pinta($("velocidades"), res.length ? res.map((r) => {
    const escolhido = t.ordem[0] === r.variante;
    if (!r.ok) {
      return `<div class="vel falhou"><div class="nome">${esc(r.nome)}<small class="erro-texto">${esc(r.erro || "não funcionou")}</small></div>
        <div class="trilho"></div><div class="numero">não funcionou</div></div>`;
    }
    return `<div class="vel"><div class="nome">${esc(r.nome)}${escolhido ? '<small>o mais rápido</small>' : "<small>funcionou</small>"}</div>
      <div class="trilho"><div class="calor" style="width:${(100 * (r.geracao_tps || 0) / max).toFixed(1)}%"></div></div>
      <div class="numero">${tps(r.geracao_tps)}<small>tokens/s, lendo ${tps(r.leitura_tps)}</small></div></div>`;
  }).join("") : `<div class="vazio"><strong>O teste ainda não foi feito.</strong>Ele roda sozinho quando você prepara o Ignis.</div>`);
  $("teste-nota").textContent = !t.feito ? "" :
    (t.atual ? `Velocidade escrevendo a resposta, em tokens por segundo (um token é mais ou menos ¾ de palavra). Testado em ${hora(t.quando)}.`
      : `A placa ou o driver mudou desde o último teste (${hora(t.quando)}). Teste de novo para escolher o melhor motor.`);
  $("btn-testar").disabled = s.fase === "testando" || s.fase === "preparando" || !s.detectado;
}

function comandoDe(dica) {
  const m = dica.match(/(sudo [^(]+?)(\s+\(|$)/);
  return m ? m[1].trim() : null;
}
function renderCandidatos() {
  if (!motores) return;
  pinta($("candidatos"), motores.candidatos.map((c) => {
    const est = c.disponivel ? (c.instalado ? '<span class="etiqueta ok">Instalado</span>' : `<span class="etiqueta">Disponível (${tamanho(c.tamanho_download)})</span>`)
      : '<span class="etiqueta alerta">Indisponível</span>';
    const cmd = c.dica ? comandoDe(c.dica) : null;
    const dica = c.dica ? `<div class="dica"><span>${esc(c.dica.split(" Para instalar:")[0])}</span>${cmd ? `<code>${esc(cmd)}</code><button class="botao pequeno" data-copiar="${esc(cmd)}">Copiar</button>` : ""}</div>` : "";
    return `<div class="linha"><span class="icone-linha">${ICONES.motor}</span>
      <div><div class="titulo">${esc(c.nome)}</div><div class="sub">${esc(c.motivo)}</div>${dica}</div>
      <div class="lado">${est}</div></div>`;
  }).join("") || '<div class="vazio">Detectando…</div>');
}

// ---------- MODELOS ----------
function textoCabe(c, rec) {
  const precisa = rec.usa_placa ? c.memoria_placa_mb : c.memoria_ram_mb;
  const livre = rec.usa_placa ? rec.vram_livre_mb : rec.ram_livre_mb;
  const frase = {
    "placa": `Cabe inteiro na placa: usa cerca de ${gbDeMb(c.memoria_placa_mb)} de ${gbDeMb(rec.vram_livre_mb)} livres`,
    "dividido": "Não cabe inteiro na placa: parte roda no processador (mais devagar)",
    "processador": rec.usa_placa ? "Roda só no processador (lento)" : `Roda no processador: usa cerca de ${gbDeMb(c.memoria_ram_mb)}`,
    "nao-cabe": "Não cabe neste computador",
  }[c.encaixe] || c.rotulo;
  const pct = livre ? Math.min(100, 100 * (precisa || 0) / livre) : 0;
  const excede = c.encaixe === "dividido" || c.encaixe === "nao-cabe";
  return `<div class="cabe enc-${c.encaixe}"><div class="barra"><span class="seg ${excede ? "seg-excede" : "seg-modelo"}" style="width:${excede ? 100 : pct.toFixed(1)}%"></span></div>${frase}</div>`;
}

function renderModelos() {
  if (!modelos) return;
  const rec = modelos.recursos || {};
  const inst = modelos.instalados;
  pinta($("instalados"), inst.length ? inst.map((m) => {
    const tipo = m.embeddings ? "embeddings" : (m.id.includes("coder") ? "codigo" : "conversa");
    const etiquetas = [
      m.carregado ? '<span class="etiqueta brasa">Carregado</span>' : "",
      m.padrao ? '<span class="etiqueta">Padrão</span>' : "",
      m.visao ? '<span class="etiqueta">Visão/áudio</span>' : "",
      m.existe ? "" : '<span class="etiqueta erro">Arquivo sumiu</span>',
    ].join("");
    const cor = { "placa": "ok", "dividido": "alerta", "processador": "alerta", "nao-cabe": "erro" }[m.encaixe] || "";
    return `<div class="linha"><span class="icone-linha">${ICONES[tipo]}</span>
      <div><div class="titulo">${esc(m.nome)} <span class="etiquetas" style="display:inline-flex">${etiquetas}</span></div>
        <div class="sub">${tamanho(m.tamanho)}, ${esc(ORIGEM[m.origem.tipo] || m.origem.tipo)}. <span class="cabe enc-${m.encaixe}" style="color:var(--${cor || "suave"})">${esc(m.rotulo)}</span></div></div>
      <div class="lado">
        ${m.carregado ? '<button class="botao" data-acao="descarregar">Descarregar</button>' : `<button class="botao" data-acao="carregar" data-id="${esc(m.id)}" ${m.existe ? "" : "disabled"}>Carregar</button>`}
        ${m.padrao || m.embeddings ? "" : `<button class="botao texto" data-acao="padrao" data-id="${esc(m.id)}">Usar como padrão</button>`}
        <button class="botao texto perigo" data-acao="remover" data-id="${esc(m.id)}" data-nome="${esc(m.nome)}" data-origem="${esc(m.origem.tipo)}">Remover</button>
      </div></div>`;
  }).join("") : `<div class="vazio"><strong>Nenhum modelo ainda.</strong>Escolha um abaixo: os marcados em verde rodam bem na sua placa.</div>`);

  const tarefas = modelos.tarefas.filter((t) => t.estado !== "concluida" && t.tipo !== "motor");
  $("grupo-tarefas").hidden = tarefas.length === 0;
  pinta($("tarefas"), tarefas.map((t) => {
    const pct = t.total ? (100 * t.baixado / t.total) : 0;
    const situ = t.estado === "andamento" ? `${tamanho(t.baixado)} de ${tamanho(t.total)}` :
      t.estado === "pausada" ? "Pausado. O que já foi baixado está guardado." : esc(t.erro || "Erro");
    const idModelo = t.id.startsWith("modelo:") ? t.id.slice(7) : t.id.startsWith("hf:") ? t.id : null;
    const retomar = t.id.startsWith("hf:") ? `data-acao="hf-retomar" data-tarefa="${esc(t.id)}"` : `data-acao="baixar" data-id="${esc(idModelo)}"`;
    return `<div class="linha"><span class="icone-linha">${ICONES.baixando}</span>
      <div><div class="titulo">${esc(t.nome)}</div><div class="sub ${t.estado === "erro" ? "erro-texto" : ""}">${situ}</div>
        ${t.estado === "andamento" ? `<div class="progresso" style="margin-top:7px"><div style="width:${pct.toFixed(1)}%"></div></div>` : ""}</div>
      <div class="lado">
        ${t.estado === "andamento" ? `<button class="botao" data-acao="cancelar" data-id="${esc(t.id)}">Cancelar</button>` : ""}
        ${t.estado !== "andamento" && idModelo ? `<button class="botao" ${retomar}>${t.estado === "erro" ? "Tentar de novo" : "Retomar"}</button>
          <button class="botao texto perigo" data-acao="remover" data-id="${esc(idModelo)}" data-nome="${esc(t.nome)}">Apagar parte baixada</button>` : ""}
      </div></div>`;
  }).join(""));

  $("titulo-catalogo").textContent = estado && estado.placa ? "Para a sua placa" : "Para este computador";
  const soBons = $("so-bons").checked;
  const cat = modelos.catalogo.filter((c) => (filtroTipo === "todos" || c.tipo === filtroTipo) && (!soBons || c.recomendado || c.instalado));
  pinta($("catalogo"), cat.map((c) => {
    const t = c.tarefa && c.tarefa.estado === "andamento";
    const botao = c.instalado ? '<span class="etiqueta ok">Instalado</span>' :
      t ? `<button class="botao" data-acao="cancelar" data-id="modelo:${esc(c.id)}">Cancelar</button>` :
      `<button class="botao primario" data-acao="baixar" data-id="${esc(c.id)}" data-encaixe="${esc(c.encaixe)}" data-aviso="${esc(c.aviso || "")}">${c.parcial > 0 ? "Retomar" : "Baixar"}</button>`;
    return `<div class="modelo">
      <h3>${esc(c.nome)}</h3>
      <div class="etiquetas"><span class="etiqueta">${tamanho(c.tamanho)}</span><span class="etiqueta">${esc(c.quantizacao)}</span><span class="etiqueta">${esc(TIPO[c.tipo] || c.tipo)}</span></div>
      <div class="uso">${esc(c.uso)}</div>
      ${t ? `<div class="progresso"><div style="width:${(100 * c.tarefa.baixado / c.tamanho).toFixed(1)}%"></div></div>` : textoCabe(c, rec)}
      <div class="rodape-modelo"><span class="licenca">Licença: <button class="botao link" data-link="${esc(c.licenca_url)}">${esc(c.licenca)}</button></span>${botao}</div>
    </div>`;
  }).join("") || `<div class="vazio caixa"><strong>Nenhum modelo deste tipo roda bem aqui.</strong>Desligue "Só os que rodam bem" para ver todos.</div>`);
}
document.querySelectorAll(".filtro").forEach((b) => b.addEventListener("click", () => {
  filtroTipo = b.dataset.tipo;
  document.querySelectorAll(".filtro").forEach((x) => x.classList.toggle("ativo", x === b));
  renderModelos();
}));
$("so-bons").addEventListener("change", renderModelos);

// ---------- Hugging Face ----------
async function hfBuscar() {
  const termo = $("hf-termo").value.trim();
  if (termo.length < 2) throw new Error("Escreva pelo menos 2 letras para buscar.");
  const r = await api("GET", "/motor/hf/buscar?q=" + encodeURIComponent(termo));
  $("hf-resultados").hidden = false;
  $("hf-repo").hidden = true;
  pinta($("hf-resultados"), r.modelos.map((m) => `<div class="linha"><span class="icone-linha">${ICONES.repo}</span>
    <div><div class="titulo">${esc(m.repo)}</div>
      <div class="etiquetas" style="margin-top:4px"><span class="etiqueta">${milhares(m.downloads)} downloads</span>
        ${m.licenca ? `<span class="etiqueta">${esc(m.licenca)}</span>` : ""}
        ${m.multimodal ? '<span class="etiqueta">Visão/áudio</span>' : ""}
        ${m.pede_aceite ? '<span class="etiqueta alerta">Pede aceite dos termos</span>' : ""}</div></div>
    <div class="lado"><button class="botao" data-acao="hf-abrir" data-repo="${esc(m.repo)}">Ver arquivos</button></div></div>`).join("")
    || `<div class="vazio"><strong>Nada encontrado.</strong>Tente outro nome, por exemplo "qwen3" ou "gemma".</div>`);
}
async function hfAbrir(repo) {
  const r = await api("GET", "/motor/hf/repo?repo=" + encodeURIComponent(repo));
  hfRepoAberto = r;
  const conta = r.conta || {};
  const aviso = r.pede_aceite ? `<div class="repo-aviso">Este modelo pede para aceitar os termos de uso.
      ${conta.configurado ? "Aceite na página do modelo com a sua conta e depois baixe aqui." : "Aceite na página do modelo e conecte a sua conta do Hugging Face em Motor."}
      <button class="botao link" data-link="${esc(r.pagina)}">Abrir a página</button></div>` : "";
  const mm = r.mmproj.length ? `<div class="ajuste"><div><strong>Incluir visão/áudio</strong><small>Arquivo extra (${tamanho(r.mmproj[0].tamanho)}) para o modelo entender imagens ou som.</small></div>
      <input type="checkbox" class="interruptor" id="hf-mmproj" checked></div>` : "";
  $("hf-repo").hidden = false;
  pinta($("hf-repo"), `<div class="repo-topo"><div><h3>${esc(r.repo)}</h3><div class="sub nota" style="margin:2px 0 0">${r.arquivos.length} versões${r.licenca ? ", licença " + esc(r.licenca) : ""}</div></div>
      <button class="botao texto" data-link="${esc(r.pagina)}">Abrir no site</button></div>
    ${aviso}${mm}
    ${r.arquivos.map((f) => {
      const cor = { "placa": "ok", "dividido": "alerta", "processador": "alerta", "nao-cabe": "erro" }[f.encaixe] || "";
      return `<div class="linha"><span class="icone-linha">${ICONES.baixando}</span>
        <div><div class="titulo">${esc(f.quantizacao || f.nome)}</div><div class="sub">${tamanho(f.tamanho)}${f.partes.length > 1 ? ` em ${f.partes.length} partes` : ""}. <span class="hf-cabe" data-arquivo="${esc(f.primeira_parte)}" style="color:var(--${cor})">${esc(f.rotulo)} (estimativa)</span></div></div>
        <div class="lado"><button class="botao texto" data-acao="hf-cabe" data-arquivo="${esc(f.primeira_parte)}">Ver se cabe</button>
          <button class="botao primario" data-acao="hf-baixar" data-arquivo="${esc(f.nome)}">Baixar</button></div></div>`;
    }).join("") || '<div class="vazio">Este repositório não tem arquivos GGUF.</div>'}`);
}
$("btn-hf-buscar").addEventListener("click", (e) => acao(e.target, hfBuscar));
$("hf-termo").addEventListener("keydown", (e) => { if (e.key === "Enter") acao($("btn-hf-buscar"), hfBuscar); });

// ---------- MOTOR ----------
let configCarregada = false;
function renderMotor() {
  if (!motores) return;
  const forcado = motores.forcado;
  const opcao = (valor, titulo, sub, marcada, disponivel, extra = "") => `<label class="opcao ${marcada ? "marcada" : ""} ${disponivel ? "" : "indisponivel"}">
    <input type="radio" name="motor" value="${esc(valor)}" ${marcada ? "checked" : ""} ${disponivel ? "" : "disabled"}>
    <div><div class="titulo">${titulo}</div><div class="sub">${sub}</div>${extra}</div><div></div></label>`;
  const html = [opcao("auto", 'Automático <span class="etiqueta brasa">Recomendado</span>', "Usa o mais rápido do teste e passa para o próximo sozinho se ele falhar.", !forcado, true)]
    .concat(motores.candidatos.map((c) => opcao(c.variante, esc(c.nome), esc(c.motivo), forcado === c.variante, c.disponivel)))
    .join("");
  pinta($("escolha-motor"), html);

  const instalados = motores.candidatos.filter((c) => c.instalado).map((c) => c.nome).join(", ") || "nenhum";
  pinta($("sobre"), `<div class="ajuste"><div><strong>Ignis ${esc(estado.app.versao)}</strong><small>Motor de IA local. Sem telemetria.</small></div></div>
    <div class="ajuste"><div><strong>Motor: llama.cpp ${esc(motores.llama_cpp.versao)}</strong><small>Versão oficial, conferida por SHA-256 (licença MIT). Baixados: ${esc(instalados)}.</small></div></div>`);

  const att = estado.atualizacao;
  if (att) {
    const partes = [];
    if (att.app) partes.push(`Aplicativo ${esc(att.app.versao)} <button class="botao link" data-link="${esc(att.app.pagina)}">baixar</button>`);
    if (att.motor) partes.push(`Motor llama.cpp ${esc(att.motor)} <button class="botao link" data-acao="att" data-item="motor">atualizar</button>`);
    if (att.catalogo) partes.push(`Catálogo de modelos <button class="botao link" data-acao="att" data-item="catalogo">atualizar</button>`);
    pinta($("att-info"), partes.length ? `<div><strong>Atualização disponível</strong><small>${partes.join("; ")}</small></div>`
      : `<div><strong>Tudo em dia</strong><small>Verificado em ${hora(att.verificado_em)}.</small></div>`);
  }
  $("dica-gnome").classList.toggle("escondido", !(estado.sistema && !["windows", "mac"].includes(estado.sistema.familia)));
}
async function carregarConfig() {
  if (configCarregada) return;
  const c = await api("GET", "/motor/config");
  $("cfg-porta").value = c.porta;
  const min = Math.round(c.descarregar_apos_seg / 60);
  const sel = $("cfg-descarregar");
  if (![...sel.options].some((o) => Number(o.value) === min)) sel.add(new Option(`Depois de ${min} min`, String(min)));
  sel.value = String(min);
  $("cfg-contexto").value = String(c.contexto_preferido);
  $("cfg-botoes").value = c.botoes_janela || "auto";
  $("cfg-autostart").checked = c.abrir_com_computador;
  $("cfg-att-auto").checked = c.procurar_atualizacoes_sozinho;
  $("cfg-proprio").value = c.llama_server_proprio || "";
  const conta = await api("GET", "/motor/hf/conta");
  renderConta(conta);
  configCarregada = true;
}
function renderConta(c) {
  pinta($("hf-conta-estado"), c.configurado
    ? `<div><strong>${c.nome ? "Conectada como " + esc(c.nome) : "Token salvo"}</strong><small>Usado só para baixar modelos que pedem aceite dos termos.</small></div><button class="botao texto perigo" data-acao="hf-sair">Desconectar</button>`
    : `<div><strong>Não conectada</strong><small>Opcional. Só é preciso para modelos que pedem para aceitar termos de uso.</small></div>`);
}
async function salvar(patch, texto) {
  await api("POST", "/motor/config", patch);
  mensagem(texto || "Salvo.", "info");
}
$("cfg-autostart").addEventListener("change", (e) => acao(null, () => salvar({ abrir_com_computador: e.target.checked }, e.target.checked ? "O Ignis vai abrir com o computador." : "O Ignis não vai mais abrir com o computador.")));
$("cfg-att-auto").addEventListener("change", (e) => acao(null, () => salvar({ procurar_atualizacoes_sozinho: e.target.checked })));
$("cfg-descarregar").addEventListener("change", (e) => acao(null, () => salvar({ descarregar_apos_min: Number(e.target.value) })));
$("cfg-botoes").addEventListener("change", (e) => acao(null, async () => {
  await salvar({ botoes_janela: e.target.value }, "Botões da janela atualizados.");
  if (window.montarBotoesJanela) await window.montarBotoesJanela(e.target.value);
}));
$("cfg-contexto").addEventListener("change", (e) => acao(null, () => salvar({ contexto_preferido: Number(e.target.value) }, "Vale a partir do próximo modelo carregado.")));
$("cfg-porta").addEventListener("change", (e) => acao(null, async () => {
  const p = Number(e.target.value);
  if (!(p >= 1024 && p <= 65535)) throw new Error("Use uma porta entre 1024 e 65535.");
  await salvar({ porta: p }, `A API agora atende na porta ${p}.`);
}));
$("btn-proprio").addEventListener("click", (e) => acao(e.target, () => salvar({ llama_server_proprio: $("cfg-proprio").value }, "llama-server salvo. Teste de novo para comparar.")));
$("btn-procurar-att").addEventListener("click", (e) => acao(e.target, () => api("POST", "/motor/atualizacoes/procurar")));
$("btn-hf-token").addEventListener("click", (e) => acao(e.target, async () => {
  const token = $("hf-token").value.trim();
  if (!token) throw new Error("Cole o token do Hugging Face.");
  const c = await api("POST", "/motor/hf/conta", { token });
  $("hf-token").value = "";
  renderConta(c);
  mensagem("Conta do Hugging Face conectada.", "info");
}));
document.addEventListener("change", (e) => {
  if (e.target.name === "motor") acao(null, () => api("POST", "/motor/motores/forcar", { variante: e.target.value === "auto" ? null : e.target.value }));
});

// ---------- STATUS ----------
function modeloExemplo() {
  if (estado.modelo) return estado.modelo.id;
  const m = modelos && modelos.instalados.find((x) => x.padrao) || modelos && modelos.instalados.find((x) => !x.embeddings && x.existe);
  return m ? m.id : "nome-do-modelo";
}
function textoExemplo() {
  const base = estado.api.endereco;
  const modelo = modeloExemplo();
  if (exemplo === "python") return `from openai import OpenAI\n\ncliente = OpenAI(base_url="${base}", api_key="ignis")\nr = cliente.chat.completions.create(\n    model="${modelo}",\n    messages=[{"role": "user", "content": "Olá!"}],\n)\nprint(r.choices[0].message.content)`;
  if (exemplo === "js") return `const r = await fetch("${base}/chat/completions", {\n  method: "POST",\n  headers: { "content-type": "application/json" },\n  body: JSON.stringify({\n    model: "${modelo}",\n    messages: [{ role: "user", content: "Olá!" }],\n  }),\n});\nconsole.log((await r.json()).choices[0].message.content);`;
  return `curl ${base}/chat/completions \\\n  -H "content-type: application/json" \\\n  -d '{"model": "${modelo}", "messages": [{"role": "user", "content": "Olá!"}]}'`;
}
function renderStatus() {
  const s = estado;
  const trabalhando = ["carregando", "testando", "preparando"].includes(s.fase) || s.requisicoes_ativas > 0;
  const ponto = !s.api.rodando ? "erro" : s.pausado ? "alerta" : trabalhando ? "trabalhando" : "ok";
  const titulo = !s.api.rodando ? "A API está parada" : s.pausado ? "Pausado" : s.modelo ? "Rodando " + esc(s.modelo.nome) : { carregando: "Carregando modelo", testando: "Testando motores", preparando: "Preparando" }[s.fase] || "Pronto, sem modelo carregado";
  const sub = s.api.erro ? esc(s.api.erro) : s.pausado ? "Novos pedidos são recusados até você retomar. A placa está livre." :
    s.modelo ? `No motor ${esc(s.motor.nome)}, ${s.motor.backend === "cpu" || s.modelo.camadas_na_placa === 0 ? "rodando no processador" : s.modelo.todas_na_placa ? "todo na placa" : s.modelo.camadas_na_placa + " camadas na placa"}.` :
    esc(s.fase_texto || "O modelo carrega sozinho no primeiro pedido.");
  let barra = "";
  if (s.modelo && s.placa && s.motor.backend !== "cpu") {
    barra = barraMemoria({ total: s.placa.vram_total_mb, livre: s.placa.vram_livre_mb, modelo: s.modelo.memoria_placa_mb, rotuloTotal: "Memória de vídeo" });
  }
  pinta($("status-cartao"), `<div class="status-topo"><span class="ponto ${ponto}"></span><div><h2>${titulo}</h2><p>${sub}</p></div></div>${barra}
    <div class="acoes">
      <button class="botao ${s.pausado ? "primario" : ""}" data-acao="${s.pausado ? "retomar" : "pausar"}">${s.pausado ? "Retomar" : "Pausar"}</button>
      <button class="botao" data-acao="descarregar" ${s.modelo ? "" : "disabled"}>Descarregar modelo</button>
      <button class="botao" data-acao="parar" ${s.requisicoes_ativas ? "" : "disabled"}>Parar conversas</button>
    </div>`);
  const v = s.velocidade;
  pinta($("medidas"), `
    <dl class="medida"><dt>Escrevendo</dt><dd>${v ? tps(v.geracao_tps) : "—"}<small>tokens/s</small></dd></dl>
    <dl class="medida"><dt>Lendo</dt><dd>${v ? tps(v.leitura_tps) : "—"}<small>tokens/s</small></dd></dl>
    <dl class="medida"><dt>Memória da conversa</dt><dd>${s.modelo ? s.modelo.contexto.toLocaleString("pt-BR") : "—"}<small>tokens</small></dd></dl>
    <dl class="medida"><dt>Conversas agora</dt><dd>${s.requisicoes_ativas}</dd></dl>`);
  if (document.activeElement !== $("endereco")) $("endereco").value = s.api.endereco;
  pinta($("exemplo"), esc(textoExemplo()));
  pinta($("avisos"), s.avisos.slice().reverse().map((a) => `<div class="aviso-item ${a.nivel === "erro" ? "erro" : a.nivel === "aviso" ? "aviso-nivel" : ""}"><time>${horaCurta(a.quando)}</time><span>${esc(a.texto)}</span></div>`).join("")
    || '<div class="vazio">Nenhum aviso por enquanto.</div>');
}
document.querySelectorAll(".aba").forEach((b) => b.addEventListener("click", () => {
  exemplo = b.dataset.exemplo;
  document.querySelectorAll(".aba").forEach((x) => x.classList.toggle("ativo", x === b));
  renderStatus();
}));
$("btn-copiar-endereco").addEventListener("click", () => copiar($("endereco").value, "Endereço copiado."));
$("btn-copiar-exemplo").addEventListener("click", () => copiar(textoExemplo(), "Exemplo copiado."));
$("registro-det").addEventListener("toggle", async () => {
  if ($("registro-det").open) {
    const r = await api("GET", "/motor/registro").catch(() => ({ linhas: [] }));
    $("registro").textContent = r.linhas.join("\n") || "(nenhum motor rodando agora)";
  }
});
$("btn-pasta-registros").addEventListener("click", () => tauri && tauri.invoke("abrir_pasta", { qual: "registros" }));
$("btn-pasta-modelos").addEventListener("click", () => tauri && tauri.invoke("abrir_pasta", { qual: "modelos" }));

// ---------- Ollama e arquivo ----------
async function procurarOllama() {
  const r = await api("GET", "/motor/modelos/ollama");
  $("ollama-lista").hidden = r.modelos.length === 0;
  $("ollama-nota").textContent = r.modelos.length ? "" : "Nenhum modelo do Ollama encontrado. Pastas procuradas: " + r.pastas_procuradas.join(", ");
  pinta($("ollama-lista"), r.modelos.map((m) => `<div class="linha"><span class="icone-linha">${ICONES.ollama}</span>
    <div><div class="titulo">${esc(m.referencia)}</div><div class="sub">${tamanho(m.tamanho)}</div></div>
    <div class="lado">${m.ja_importado ? '<span class="etiqueta ok">Já está no Ignis</span>' :
      `<button class="botao" data-acao="ollama" data-modo="usar" data-ref="${esc(m.referencia)}">Usar daqui</button><button class="botao texto" data-acao="ollama" data-modo="copiar" data-ref="${esc(m.referencia)}">Copiar</button>`}</div></div>`).join(""));
}
$("btn-ollama").addEventListener("click", (e) => acao(e.target, procurarOllama));
$("btn-arquivo").addEventListener("click", (e) => acao(e.target, async () => {
  const caminho = $("arquivo-gguf").value.trim();
  if (!caminho) throw new Error("Escreva o caminho do arquivo .gguf.");
  await api("POST", "/motor/modelos/importar", { caminho });
  $("arquivo-gguf").value = "";
  mensagem("Modelo adicionado.", "info");
}));

// ---------- cliques (delegados) ----------
document.addEventListener("click", async (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  if (b.dataset.link) return abrirLink(b.dataset.link);
  if (b.dataset.copiar) return copiar(b.dataset.copiar, "Comando copiado.");
  const a = b.dataset.acao;
  if (!a) return;
  const id = b.dataset.id;
  switch (a) {
    case "preparar": return acao(b, () => api("POST", "/motor/preparar"));
    case "ir-modelos": return irPara("modelos");
    case "detectar": return acao(b, () => api("POST", "/motor/detectar"));
    case "relatorio": return acao(b, async () => copiar(JSON.stringify(await api("GET", "/motor/relatorio"), null, 2), "Relatório copiado."));
    case "baixar": {
      const enc = b.dataset.encaixe;
      if ((enc === "nao-cabe" || enc === "dividido") && !confirm((b.dataset.aviso || "Este modelo não cabe inteiro na placa.") + "\n\nBaixar mesmo assim?")) return;
      return acao(b, () => api("POST", "/motor/modelos/baixar", { id }));
    }
    case "cancelar": return acao(b, () => api("POST", "/motor/tarefas/cancelar", { id }));
    case "carregar": return acao(b, async () => { mensagem("Carregando o modelo…", "info"); await api("POST", "/motor/modelos/carregar", { id }); });
    case "descarregar": return acao(b, () => api("POST", "/motor/modelos/descarregar"));
    case "padrao": return acao(b, () => api("POST", "/motor/modelos/padrao", { id }));
    case "remover": {
      const ref = b.dataset.origem === "ollama-referencia" || b.dataset.origem === "arquivo";
      if (!confirm(ref ? `Tirar "${b.dataset.nome}" da lista? O arquivo original continua onde está.` : `Apagar "${b.dataset.nome}" do disco?`)) return;
      return acao(b, async () => { const r = await api("POST", "/motor/modelos/remover", { id }); mensagem(r.mensagem, "info"); });
    }
    case "ollama": return acao(b, async () => { await api("POST", "/motor/modelos/importar", { referencia: b.dataset.ref, modo: b.dataset.modo }); await procurarOllama(); });
    case "pausar": return acao(b, () => api("POST", "/motor/pausar"));
    case "retomar": return acao(b, () => api("POST", "/motor/retomar"));
    case "parar": return acao(b, () => api("POST", "/motor/parar", {}));
    case "att": return acao(b, () => api("POST", "/motor/atualizacoes/aplicar", { item: b.dataset.item }));
    case "hf-abrir": return acao(b, () => hfAbrir(b.dataset.repo));
    case "hf-cabe": return acao(b, async () => {
      const r = await api("POST", "/motor/hf/info", { repo: hfRepoAberto.repo, revisao: hfRepoAberto.revisao, arquivo: b.dataset.arquivo });
      const alvo = document.querySelector(`.hf-cabe[data-arquivo="${CSS.escape(b.dataset.arquivo)}"]`);
      const cor = { "placa": "ok", "dividido": "alerta", "processador": "alerta", "nao-cabe": "erro" }[r.plano.encaixe] || "suave";
      if (alvo) { alvo.textContent = r.rotulo + ` (memória da conversa: ${r.plano.contexto.toLocaleString("pt-BR")} tokens)`; alvo.style.color = `var(--${cor})`; }
    });
    case "hf-baixar": {
      const mm = $("hf-mmproj") && $("hf-mmproj").checked && hfRepoAberto.mmproj.length ? hfRepoAberto.mmproj[0].nome : null;
      return acao(b, async () => { await api("POST", "/motor/hf/baixar", { repo: hfRepoAberto.repo, arquivo: b.dataset.arquivo, mmproj: mm }); mensagem("Download começou. Acompanhe em Baixando.", "info"); });
    }
    case "hf-retomar": {
      const resto = b.dataset.tarefa.slice(3);
      const partes = resto.split("/");
      return acao(b, () => api("POST", "/motor/hf/baixar", { repo: partes.slice(0, 2).join("/"), arquivo: partes.slice(2).join("/") }));
    }
    case "hf-sair": return acao(b, async () => { renderConta(await api("POST", "/motor/hf/conta", { token: "" })); mensagem("Conta do Hugging Face desconectada.", "info"); });
  }
});

// ---------- ciclo ----------
let ocupado = false;
let ultimoModelos = 0;
async function atualizar(forcar) {
  if (ocupado) return;
  ocupado = true;
  try {
    estado = await api("GET", "/motor/estado");
    renderRodape();
    const agora = Date.now();
    const precisaMotores = secao === "placa" || secao === "motor" || (estado.precisa_preparar && !guiaOculto());
    if (precisaMotores && (forcar || agora - motoresEm > 4000)) { motores = await api("GET", "/motor/motores"); motoresEm = agora; }
    const baixando = estado.tarefas.some((t) => t.estado === "andamento");
    if (!modelos || secao === "modelos" || secao === "status" || forcar || (baixando && agora - ultimoModelos > 2000)) {
      if (forcar || agora - ultimoModelos > 1500) { modelos = await api("GET", "/motor/modelos"); ultimoModelos = agora; }
    }
    renderGuia();
    if (secao === "placa") { renderPlaca(); renderCandidatos(); }
    if (secao === "modelos") renderModelos();
    if (secao === "motor") { await carregarConfig(); renderMotor(); }
    if (secao === "status") renderStatus();
  } catch (e) {
    $("rodape-titulo").textContent = "Sem conexão com o núcleo";
    $("rodape-ponto").className = "ponto erro";
  } finally {
    ocupado = false;
  }
}
// só atualiza com a janela visível
setInterval(() => { if (document.visibilityState === "visible") atualizar(false); }, 1500);
document.addEventListener("visibilitychange", () => { if (document.visibilityState === "visible") atualizar(true); });
atualizar(true);
