// Breezedesk website: translations (EN/PT), live demo widgets, playground.
(() => {
  "use strict";

  // ------------------------------------------------------------ translations
  const PT = {
    "nav.why": "Por que o Breezedesk",
    "nav.widgets": "Widgets",
    "nav.try": "Experimente",
    "nav.faq": "Dúvidas",
    "cta.short": "Baixar",
    "cta.download": "Baixar para Windows",
    "cta.why": "Por que o Breezedesk?",
    "hero.pill": "Grátis e open source · Windows 10 e 11",
    "hero.h1a": "Sua área de trabalho,",
    "hero.h1b": "viva.",
    "hero.lead": "Widgets bonitos para o Windows que quase não pesam no seu PC. Relógio, uso do sistema, música, clima, notícias e mais, prontos em um clique e grátis para sempre.",
    "trust.admin": "Não pede administrador",
    "trust.updates": "Se atualiza sozinho",
    "trust.ads": "Sem anúncios nem rastreamento",
    "stats.draw": "para desenhar um widget",
    "stats.size": "o programa inteiro",
    "stats.ready": "widgets prontos para usar",
    "stats.ads": "anúncios, contas ou rastreamento",
    "why.kicker": "Por que as pessoas usam o Breezedesk",
    "why.title": "Quatro motivos para colocar na sua área de trabalho",
    "why.light.t": "Leve como uma pena",
    "why.light.p": "Seus jogos e programas continuam com o computador só para eles. O Breezedesk desenha um widget em menos de meio milissegundo e dorme até o próximo segundo.",
    "why.light.1": "Programa nativo feito em Rust: sem navegador embutido, sem .NET",
    "why.light.2": "Sensores que você não mostra nunca são lidos",
    "why.light.3": "Widgets escondidos e telas que não mudaram não custam nada",
    "why.pretty.t": "Bonito desde o primeiro minuto",
    "why.pretty.p": "Dez widgets caprichados já vêm junto. Ligue pelo ícone da bandeja e pronto.",
    "why.pretty.1": "Fundos de vidro (acrílico) e desfoque, animações suaves",
    "why.pretty.2": "Nítido em telas 4K e com escala de 125%/150%",
    "why.pretty.3": "Datas no seu idioma, automaticamente",
    "why.easy.t": "Do seu jeito, sem programar",
    "why.easy.p": "Mude cores, textos e tamanhos no Breezedesk Editor e veja a área de trabalho mudar enquanto você arrasta os valores.",
    "why.easy.1": "Editor visual com seletor de cores e prévia ao vivo",
    "why.easy.2": "Compartilhe widgets em um arquivo .breeze e instale com dois cliques",
    "why.easy.3": "Quem quer mais tem fórmulas e scripts LuaJIT",
    "why.safe.t": "Seguro, privado e seu",
    "why.safe.p": "Sem conta, sem anúncios, sem coleta de dados. O código é aberto para qualquer pessoa ler.",
    "why.safe.1": "Open source com licença MIT",
    "why.safe.2": "Atualizações conferidas (SHA-256) antes de instalar",
    "why.safe.3": "Scripts de widgets rodam isolados, sem acesso aos seus arquivos",
    "show.kicker": "O que você pode ter na área de trabalho",
    "show.title": "Tudo à vista, nada no caminho",
    "show.sys.t": "Saiba o que o seu PC está fazendo",
    "show.sys.p": "CPU (total e por núcleo), RAM, GPU e memória de vídeo, discos, rede, bateria, temperaturas e os programas que mais consomem. As cores mudam quando algo esquenta.",
    "show.music.t": "Sua música na área de trabalho",
    "show.music.p": "Veja o que está tocando no Spotify, no navegador ou em qualquer player, com a capa do álbum, botões e um visualizador que dança com o som.",
    "show.info.t": "Clima, notícias e seu calendário",
    "show.info.p": "O clima da sua cidade ao vivo, manchetes de qualquer site de notícias e um calendário no seu idioma. Clique numa manchete para ler.",
    "show.editor.t": "Um editor feito para gente",
    "show.editor.p": "Ligue e desligue widgets, escolha cores, arraste números. Com o salvamento automático, cada mudança aparece na hora na sua área de trabalho.",
    "try.kicker": "Experimente aqui mesmo",
    "try.title": "Do seu jeito em segundos",
    "try.sub": "É assim que se personaliza um widget. No Breezedesk você faz o mesmo no editor, ou em poucas linhas de texto.",
    "try.label": "Nome",
    "try.accent": "Cor de destaque",
    "try.glass": "Fundo de vidro",
    "try.ring": "Mostrar um anel",
    "try.hot": "Vermelho acima de 80%",
    "try.load": "Simular uso da CPU",
    "try.hint": "Arraste acima de 80% para ver a cor mudar.",
    "steps.title": "Funcionando em um minuto",
    "steps.1.t": "Baixe",
    "steps.1.p": "Rode o instalador. Sem administrador, sem conta.",
    "steps.2.t": "Escolha seus widgets",
    "steps.2.p": "Clique com o botão direito na pena da bandeja e ligue ou desligue widgets.",
    "steps.3.t": "Deixe do seu jeito",
    "steps.3.p": "Arraste para onde quiser. Abra o editor para mudar cores e textos.",
    "faq.title": "Dúvidas",
    "faq.1.q": "É grátis mesmo?",
    "faq.1.a": "Sim. O Breezedesk é open source com licença MIT: sem anúncios, sem versão paga, sem conta.",
    "faq.2.q": "O Windows diz que o download não é seguro. É perigoso?",
    "faq.2.a": "O Breezedesk é novo e ainda não tem assinatura digital, então o SmartScreen mostra um aviso (clique em <em>Mais informações → Executar assim mesmo</em>), e alguns antivírus podem marcá-lo por engano com nomes que terminam em <code>!ml</code>, que é um palpite de aprendizado de máquina, não um vírus conhecido. Todo o código é público, e cada versão é gerada automaticamente pelo GitHub a partir desse código.",
    "faq.3.q": "Vai deixar meus jogos mais lentos?",
    "faq.3.a": "Ele foi feito para não deixar. O Breezedesk só acorda quando um widget precisa atualizar (normalmente uma vez por segundo), desenha em menos de meio milissegundo e só lê os sensores que seus widgets mostram. Você também pode esconder todos os widgets com um clique no ícone da bandeja.",
    "faq.4.q": "Ele envia meus dados para algum lugar?",
    "faq.4.a": "Não. O Breezedesk não tem telemetria. Ele só acessa a internet para procurar atualizações no GitHub e para buscar o que seus widgets mostram (como clima ou notícias).",
    "faq.5.q": "Posso usar skins do Rainmeter?",
    "faq.5.a": "Não diretamente: o Breezedesk tem o próprio formato, mais simples. A maioria das skins é rápida de refazer com o editor ou um arquivo de texto curto.",
    "faq.6.q": "Como desinstalo?",
    "faq.6.a": "Configurações do Windows → Aplicativos → Aplicativos instalados → Breezedesk → Desinstalar. Ele pergunta antes de apagar seus widgets.",
    "final.title": "Dê um respiro para a sua área de trabalho.",
    "final.sub": "Grátis, open source e pronto em um minuto.",
    "final.portable": "Versão portátil e notas da versão",
    "footer.mit": "Licença MIT",
    "footer.by": "feito por",
    "footer.source": "Código-fonte",
    "footer.releases": "Versões",
    "footer.changes": "Novidades",
    "footer.docs": "Guia de widgets",
    "version": "Versão",
  };

  const nodes = Array.from(document.querySelectorAll("[data-i18n]"));
  const EN = {};
  nodes.forEach((n) => { EN[n.dataset.i18n] = n.innerHTML; });
  EN.version = "Version";
  let lang = "en";

  function setLang(l) {
    lang = l === "pt" ? "pt" : "en";
    const dict = lang === "pt" ? PT : EN;
    nodes.forEach((n) => { const v = dict[n.dataset.i18n]; if (v) n.innerHTML = v; });
    document.documentElement.lang = lang === "pt" ? "pt-BR" : "en";
    document.querySelectorAll(".lang button").forEach((b) => b.classList.toggle("on", b.dataset.lang === lang));
    try { localStorage.setItem("breezedesk-lang", lang); } catch (e) { /* storage unavailable */ }
    showVersion();
    tick();
    renderPlayground();
  }

  document.querySelectorAll(".lang button").forEach((b) => b.addEventListener("click", () => setLang(b.dataset.lang)));

  // ------------------------------------------------------------ version
  let version = null;
  function showVersion() {
    const el = document.getElementById("version");
    if (el && version) el.textContent = `${(lang === "pt" ? PT : EN).version} ${version}`;
  }
  fetch("https://api.github.com/repos/JoaoVictorCoder/Breezedesk/releases/latest")
    .then((r) => (r.ok ? r.json() : null))
    .then((r) => { if (r && r.tag_name) { version = r.tag_name.replace(/^v/, ""); showVersion(); } })
    .catch(() => {});

  // ------------------------------------------------------------ live clock
  function tick() {
    const now = new Date();
    const loc = lang === "pt" ? "pt-BR" : "en-US";
    const hm = now.toLocaleTimeString(loc, { hour: "2-digit", minute: "2-digit", hour12: false });
    const date = now.toLocaleDateString(loc, { weekday: "short", day: "numeric", month: "short" });
    const t = document.getElementById("lc-time");
    const d = document.getElementById("lc-date");
    const tb = document.getElementById("tb-clock");
    if (t) t.textContent = hm;
    if (d) d.textContent = date.charAt(0).toUpperCase() + date.slice(1);
    if (tb) tb.textContent = hm;
  }
  setInterval(tick, 1000);

  // ------------------------------------------------------------ live CPU graph
  const canvas = document.getElementById("lcpu");
  const valEl = document.getElementById("lcpu-val");
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const hist = [];
  let cpu = 18;
  for (let i = 0; i < 40; i++) { cpu = next(cpu); hist.push(cpu); }
  function next(v) {
    const spike = Math.random() < 0.08 ? 35 + Math.random() * 40 : 0;
    return Math.max(3, Math.min(97, v * 0.75 + (8 + Math.random() * 22) * 0.25 + spike * 0.4));
  }
  function drawCpu() {
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    const w = canvas.width, h = canvas.height;
    ctx.clearRect(0, 0, w, h);
    const step = w / (hist.length - 1);
    const hot = hist[hist.length - 1] > 80;
    const color = hot ? "#FF453A" : "#5AC8FA";
    ctx.beginPath();
    hist.forEach((v, i) => { const x = i * step, y = h - (v / 100) * h; i ? ctx.lineTo(x, y) : ctx.moveTo(x, y); });
    ctx.lineTo(w, h); ctx.lineTo(0, h); ctx.closePath();
    ctx.fillStyle = hot ? "rgba(255,69,58,.22)" : "rgba(90,200,250,.22)";
    ctx.fill();
    ctx.beginPath();
    hist.forEach((v, i) => { const x = i * step, y = h - (v / 100) * h; i ? ctx.lineTo(x, y) : ctx.moveTo(x, y); });
    ctx.strokeStyle = color; ctx.lineWidth = 2.5; ctx.lineJoin = "round"; ctx.stroke();
    if (valEl) valEl.textContent = Math.round(hist[hist.length - 1]) + "%";
  }
  drawCpu();
  if (!reduce) setInterval(() => { cpu = next(cpu); hist.push(cpu); hist.shift(); drawCpu(); }, 1000);

  // ------------------------------------------------------------ playground
  const pg = {
    label: document.getElementById("pg-label"),
    glass: document.getElementById("pg-glass"),
    ring: document.getElementById("pg-ring"),
    hot: document.getElementById("pg-hot"),
    widget: document.getElementById("pg-widget"),
    title: document.getElementById("pg-title"),
    value: document.getElementById("pg-value"),
    arc: document.getElementById("pg-arc"),
    ringSvg: document.getElementById("pg-ringsvg"),
    code: document.getElementById("pg-code"),
    load: document.getElementById("pg-load"),
    loadOut: document.getElementById("pg-load-out"),
  };
  let accent = "#5AC8FA";
  let demo = 42;
  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

  function renderPlayground() {
    if (!pg.widget) return;
    const label = (pg.label.value || "CPU").replace(/["\\{}]/g, "");
    const shown = pg.hot.checked && demo > 80 ? "#FF453A" : accent;
    pg.widget.style.setProperty("--accent", shown);
    pg.widget.classList.toggle("glass", pg.glass.checked);
    pg.title.textContent = label;
    pg.value.textContent = Math.round(demo) + "%";
    if (pg.load && document.activeElement !== pg.load) pg.load.value = String(Math.round(demo));
    if (pg.loadOut) pg.loadOut.textContent = Math.round(demo) + "%";
    pg.ringSvg.style.display = pg.ring.checked ? "" : "none";
    pg.arc.style.strokeDashoffset = String(163.4 * (1 - demo / 100));

    const color = pg.hot.checked ? `"cpu > 80 ? '#FF453A' : '${accent}'"` : `"${accent}"`;
    const L = [];
    const c = (s) => `<span class="c">${s}</span>`, k = (s) => `<span class="k">${s}</span>`;
    const st = (s) => `<span class="s">${esc(s)}</span>`, n = (s) => `<span class="n">${s}</span>`;
    const t = (s) => `[<span class="t">${s}</span>]`, tt = (s) => `[[<span class="t">${s}</span>]]`;
    L.push(c(lang === "pt" ? "# widgets/meu-widget.toml" : "# widgets/my-widget.toml"));
    L.push(`${k("name")} = ${st('"my-widget"')}`);
    L.push(`${k("width")} = ${n(pg.ring.checked ? 270 : 200)}`, `${k("height")} = ${n(90)}`, "");
    L.push(t("background"));
    L.push(`${k("color")} = ${st(pg.glass.checked ? '"#0E0F1470"' : '"#0E0F14"')}`);
    L.push(`${k("radius")} = ${n(18)}`);
    if (pg.glass.checked) L.push(`${k("blur")} = ${st('"acrylic"')}`);
    if (pg.ring.checked) {
      L.push("", tt("element"), `${k("type")} = ${st('"ring"')}`, `${k("x")} = ${n(48)}`, `${k("y")} = ${n(45)}`,
        `${k("radius")} = ${n(26)}`, `${k("value")} = ${st('"cpu"')}`, `${k("color")} = ${st(color)}`, `${k("animate")} = ${n(400)}`);
    }
    const x = pg.ring.checked ? 96 : 24;
    L.push("", tt("element"), `${k("type")} = ${st('"text"')}`, `${k("x")} = ${n(x)}`, `${k("y")} = ${n(12)}`,
      `${k("text")} = ${st(`"${label}"`)}`);
    L.push("", tt("element"), `${k("type")} = ${st('"text"')}`, `${k("x")} = ${n(x)}`, `${k("y")} = ${n(34)}`,
      `${k("size")} = ${n(34)}`, `${k("text")} = ${st('"{cpu}%"')}`, `${k("color")} = ${st(color)}`);
    pg.code.innerHTML = L.join("\n");
  }

  if (pg.widget) {
    [pg.label, pg.glass, pg.ring, pg.hot].forEach((el) => el.addEventListener("input", renderPlayground));
    document.querySelectorAll("#pg-swatches button").forEach((b) => b.addEventListener("click", () => {
      accent = b.dataset.c;
      document.querySelectorAll("#pg-swatches button").forEach((x) => x.classList.toggle("on", x === b));
      renderPlayground();
    }));
    // The value moves like a real CPU (sometimes above 80%) until the visitor
    // takes over with the slider.
    let phase = 0;
    let manual = false;
    pg.load.addEventListener("input", () => { manual = true; demo = Number(pg.load.value); renderPlayground(); });
    setInterval(() => {
      if (manual) return;
      phase++;
      demo = phase % 6 === 5 ? 86 + Math.random() * 8 : 25 + Math.random() * 40;
      renderPlayground();
    }, reduce ? 4000 : 1800);
  }

  // ------------------------------------------------------------ nav + reveal
  const nav = document.getElementById("nav");
  const onScroll = () => nav && nav.classList.toggle("scrolled", window.scrollY > 8);
  window.addEventListener("scroll", onScroll, { passive: true });
  onScroll();

  if ("IntersectionObserver" in window && !reduce) {
    const items = document.querySelectorAll(".reason, .showcase .row, .steps li, .playground, .final-card, .stats");
    items.forEach((el) => el.classList.add("reveal"));
    const io = new IntersectionObserver((entries) => {
      entries.forEach((e) => { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
    }, { threshold: 0.12 });
    items.forEach((el) => io.observe(el));
  }

  // ------------------------------------------------------------ start
  let saved = null;
  try { saved = localStorage.getItem("breezedesk-lang"); } catch (e) { /* storage unavailable */ }
  const browserPt = (navigator.language || "").toLowerCase().startsWith("pt");
  setLang(saved || (browserPt ? "pt" : "en"));
})();
