/* ============================================================
   Valvet website interactions
   No dependencies. Everything degrades without JavaScript.
   ============================================================ */

(function () {
  "use strict";

  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var finePointer = window.matchMedia("(pointer: fine)").matches;
  var root = document.documentElement;

  root.classList.add("js");

  function $(selector, scope) {
    return (scope || document).querySelector(selector);
  }

  function $$(selector, scope) {
    return Array.prototype.slice.call((scope || document).querySelectorAll(selector));
  }

  function clamp(value, min, max) {
    return Math.min(max, Math.max(min, value));
  }

  /* ---------- Theme ---------- */

  var themeToggle = $(".theme-toggle");
  var metaTheme = $('meta[name="theme-color"]');

  function currentTheme() {
    return root.dataset.theme === "dark" ? "dark" : "light";
  }

  function updateThemeButton() {
    if (!themeToggle) return;
    var next = currentTheme() === "dark" ? "light" : "dark";
    themeToggle.setAttribute("aria-label", "Switch to " + next + " theme");
    themeToggle.setAttribute("title", "Switch to " + next + " theme");
  }

  function setTheme(theme) {
    root.dataset.theme = theme;
    try {
      localStorage.setItem("valvet-theme", theme);
    } catch (error) {}
    if (metaTheme) metaTheme.setAttribute("content", theme === "dark" ? "#0b0714" : "#f7f3ff");
    updateThemeButton();
    document.dispatchEvent(new CustomEvent("valvet:theme", { detail: theme }));
  }

  if (themeToggle) {
    updateThemeButton();
    themeToggle.addEventListener("click", function () {
      setTheme(currentTheme() === "dark" ? "light" : "dark");
    });
  }

  /* ---------- Header and mobile menu ---------- */

  var header = $("#site-header");
  var menuToggle = $(".menu-toggle");
  var mobileMenu = $("#mobile-menu");

  function updateHeader() {
    if (!header) return;
    header.classList.toggle("is-scrolled", window.scrollY > 12);
  }

  function openMenu() {
    if (!mobileMenu || !menuToggle) return;
    mobileMenu.hidden = false;
    menuToggle.setAttribute("aria-expanded", "true");
    menuToggle.setAttribute("aria-label", "Close menu");
    document.body.classList.add("menu-open");
    var firstLink = $("a", mobileMenu);
    if (firstLink) firstLink.focus();
  }

  function closeMenu(returnFocus) {
    if (!mobileMenu || !menuToggle) return;
    var wasOpen = !mobileMenu.hidden;
    mobileMenu.hidden = true;
    menuToggle.setAttribute("aria-expanded", "false");
    menuToggle.setAttribute("aria-label", "Open menu");
    document.body.classList.remove("menu-open");
    if (wasOpen && returnFocus) menuToggle.focus();
  }

  if (menuToggle) {
    menuToggle.addEventListener("click", function () {
      if (!mobileMenu) return;
      if (mobileMenu.hidden) openMenu();
      else closeMenu(true);
    });
  }

  if (mobileMenu) {
    $$("a", mobileMenu).forEach(function (link) {
      link.addEventListener("click", closeMenu);
    });
  }

  document.addEventListener("keydown", function (event) {
    if (event.key === "Escape" && mobileMenu && !mobileMenu.hidden) closeMenu(true);
  });

  window.addEventListener("resize", function () {
    if (window.innerWidth > 980) closeMenu(false);
  });

  updateHeader();
  window.addEventListener("scroll", updateHeader, { passive: true });

  /* ---------- Back to top ---------- */

  var backToTop = $(".back-to-top");

  function updateBackToTop() {
    if (!backToTop) return;
    backToTop.classList.toggle("is-visible", window.scrollY > 620);
  }

  if (backToTop) {
    updateBackToTop();
    window.addEventListener("scroll", updateBackToTop, { passive: true });
    backToTop.addEventListener("click", function () {
      window.scrollTo({ top: 0, behavior: reduceMotion ? "auto" : "smooth" });
    });
  }

  /* ---------- Scroll reveal ---------- */

  var revealItems = $$(".reveal-up");

  revealItems.forEach(function (item) {
    if (item.style.getPropertyValue("--reveal-delay")) return;
    var siblings = item.parentElement ? $$(".reveal-up", item.parentElement) : [];
    var index = siblings.indexOf(item);
    if (index > 0) item.style.setProperty("--reveal-delay", Math.min(index, 7) * 55 + "ms");
  });

  if ("IntersectionObserver" in window && !reduceMotion) {
    var revealObserver = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (entry) {
          if (!entry.isIntersecting) return;
          entry.target.classList.add("is-visible");
          revealObserver.unobserve(entry.target);
        });
      },
      { threshold: 0.01, rootMargin: "0px 0px -2% 0px" }
    );

    revealItems.forEach(function (item) {
      revealObserver.observe(item);
    });
  } else {
    revealItems.forEach(function (item) {
      item.classList.add("is-visible");
    });
  }

  /* ---------- Toast ---------- */

  var toast = $(".toast");
  var toastTimer = 0;

  function showToast(message) {
    if (!toast) return;
    toast.textContent = message;
    toast.classList.add("is-visible");
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(function () {
      toast.classList.remove("is-visible");
    }, 1800);
  }

  /* ---------- Syntax highlighting ---------- */

  var KEYWORDS = new Set([
    "as", "break", "continue", "else", "enum", "for", "fun", "if", "impl",
    "import", "in", "let", "loop", "match", "mode", "module", "pub", "return",
    "struct", "trait", "use", "val", "var", "while", "where", "const", "true",
    "false", "bin", "exp", "imp"
  ]);

  var TYPES = new Set([
    "Bool", "CsvTable", "Dual", "DynamicFrame", "Float64", "Int32", "Int64",
    "Option", "Result", "Rng", "String", "Vec", "char", "f64", "i32", "i64",
    "usize"
  ]);

  var FUNCTIONS = new Set([
    "println", "print", "vec", "par_sum", "par_map", "seed", "len", "push",
    "pop", "get", "set", "frame_from_table", "frame_col", "frame_group_count",
    "frame_group_sum", "sql", "audit_summary"
  ]);

  var C_KEYWORDS = new Set([
    "auto", "break", "case", "char", "const", "continue", "default", "do",
    "double", "else", "enum", "extern", "float", "for", "goto", "if", "inline",
    "int", "long", "register", "restrict", "return", "short", "signed",
    "sizeof", "static", "struct", "switch", "typedef", "union", "unsigned",
    "void", "volatile", "while", "NULL", "true", "false"
  ]);

  function escapeHtml(text) {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  function span(className, text) {
    return '<span class="' + className + '">' + escapeHtml(text) + "</span>";
  }

  function highlightValvetLine(line) {
    var out = "";
    var i = 0;

    while (i < line.length) {
      var ch = line[i];

      if (ch === "/" && line[i + 1] === "/") {
        out += span("tok-comment", line.slice(i));
        break;
      }

      if (ch === "/" && line[i + 1] === "*") {
        var blockEnd = line.indexOf("*/", i + 2);
        if (blockEnd === -1) {
          out += span("tok-comment", line.slice(i));
          break;
        }
        out += span("tok-comment", line.slice(i, blockEnd + 2));
        i = blockEnd + 2;
        continue;
      }

      if (ch === '"' || ch === "'") {
        var quote = ch;
        var j = i + 1;
        while (j < line.length) {
          if (line[j] === "\\") {
            j += 2;
            continue;
          }
          if (line[j] === quote) {
            j += 1;
            break;
          }
          j += 1;
        }
        out += span("tok-string", line.slice(i, j));
        i = j;
        continue;
      }

      if (/[0-9]/.test(ch)) {
        var n = i + 1;
        while (n < line.length && /[0-9a-fA-FxXoObB_.]/.test(line[n])) n += 1;
        out += span("tok-number", line.slice(i, n));
        i = n;
        continue;
      }

      if (ch === "@") {
        var u = i + 1;
        while (u < line.length && /[A-Za-z0-9_/]/.test(line[u])) u += 1;
        out += span("tok-unit", line.slice(i, u));
        i = u;
        continue;
      }

      if (/[A-Za-z_]/.test(ch)) {
        var w = i + 1;
        while (w < line.length && /[A-Za-z0-9_]/.test(line[w])) w += 1;
        var word = line.slice(i, w);
        var cls = "";

        if (KEYWORDS.has(word)) cls = "tok-keyword";
        else if (TYPES.has(word)) cls = "tok-type";
        else if (FUNCTIONS.has(word)) cls = "tok-fn";

        if (line[w] === "!") cls = "tok-macro";
        if (/^\s*fun\s*$/.test(line.slice(0, i))) cls = "tok-fn";

        out += cls ? span(cls, word) : escapeHtml(word);
        i = w;
        continue;
      }

      if ("+-*/%=<>!&|^~".indexOf(ch) !== -1) {
        out += span("tok-operator", ch);
        i += 1;
        continue;
      }

      if ("(){}[],;:.".indexOf(ch) !== -1) {
        out += span("tok-punct", ch);
        i += 1;
        continue;
      }

      out += escapeHtml(ch);
      i += 1;
    }

    return out;
  }

  function highlightCLine(line) {
    var out = "";
    var i = 0;
    while (i < line.length) {
      var ch = line[i];

      if (ch === "/" && line[i + 1] === "/") {
        out += span("tok-comment", line.slice(i));
        break;
      }

      if (ch === '"' || ch === "'") {
        var quote = ch;
        var j = i + 1;
        while (j < line.length) {
          if (line[j] === "\\") {
            j += 2;
            continue;
          }
          if (line[j] === quote) {
            j += 1;
            break;
          }
          j += 1;
        }
        out += span("tok-string", line.slice(i, j));
        i = j;
        continue;
      }

      if (/[0-9]/.test(ch)) {
        var n = i + 1;
        while (n < line.length && /[0-9a-fA-FxX_.]/.test(line[n])) n += 1;
        out += span("tok-number", line.slice(i, n));
        i = n;
        continue;
      }

      if (/[A-Za-z_]/.test(ch)) {
        var w = i + 1;
        while (w < line.length && /[A-Za-z0-9_]/.test(line[w])) w += 1;
        var word = line.slice(i, w);
        var cls = C_KEYWORDS.has(word) ? "tok-keyword" : "";
        if (/(_t|_fn|_wrap|_work|_new|_free|_len|_push)$/.test(word)) cls = "tok-fn";
        out += cls ? span(cls, word) : escapeHtml(word);
        i = w;
        continue;
      }

      if ("+-*/%=<>!&|^~".indexOf(ch) !== -1) {
        out += span("tok-operator", ch);
        i += 1;
        continue;
      }

      if ("(){}[],;:.".indexOf(ch) !== -1) {
        out += span("tok-punct", ch);
        i += 1;
        continue;
      }

      out += escapeHtml(ch);
      i += 1;
    }
    return out;
  }

  function highlightBashLine(line) {
    var trimmed = line.trim();
    if (trimmed.indexOf("#") === 0) return span("tok-comment", line);

    var out = "";
    var i = 0;
    while (i < line.length) {
      var ch = line[i];
      if (ch === "#") {
        out += span("tok-comment", line.slice(i));
        break;
      }
      if (ch === '"' || ch === "'") {
        var quote = ch;
        var j = i + 1;
        while (j < line.length) {
          if (line[j] === "\\") {
            j += 2;
            continue;
          }
          if (line[j] === quote) {
            j += 1;
            break;
          }
          j += 1;
        }
        out += span("tok-string", line.slice(i, j));
        i = j;
        continue;
      }
      if (ch === "$" && (i === 0 || /\s/.test(line[i - 1]))) {
        out += span("tok-macro", ch);
        i += 1;
        continue;
      }
      if (ch === "-" && line[i + 1] === "-") {
        var flag = i + 2;
        while (flag < line.length && /[A-Za-z0-9-]/.test(line[flag])) flag += 1;
        out += span("tok-type", line.slice(i, flag));
        i = flag;
        continue;
      }
      out += escapeHtml(ch);
      i += 1;
    }
    return out;
  }

  function highlightText(raw, lang) {
    var lines = raw.split("\n");
    var safeLang = (lang || "text").toLowerCase();
    var noLines = safeLang === "bash" || safeLang === "text";

    var html = lines
      .map(function (line) {
        var content;
        if (safeLang === "valvet") content = highlightValvetLine(line);
        else if (safeLang === "c") content = highlightCLine(line);
        else if (safeLang === "bash") content = highlightBashLine(line);
        else content = escapeHtml(line);

        return '<span class="code-line">' + (content || "&nbsp;") + "</span>";
      })
      .join("\n");

    return '<span class="code-lines' + (noLines ? " no-lines" : "") + '">' + html + "</span>";
  }

  function languageOf(code) {
    var match = null;
    code.classList.forEach(function (name) {
      if (name.indexOf("language-") === 0) match = name.slice(9);
    });
    return match || "text";
  }

  function rawOf(code) {
    if (!code.dataset.raw) code.dataset.raw = code.textContent;
    return code.dataset.raw;
  }

  function highlightCode(code) {
    var raw = rawOf(code);
    code.innerHTML = highlightText(raw, languageOf(code));
    code.dataset.highlighted = "true";
  }

  function highlightStaticCode() {
    $$("pre code").forEach(function (code) {
      if (code.closest(".code-panel")) return;
      highlightCode(code);
    });
  }

  highlightStaticCode();

  /* ---------- Code typewriter ---------- */

  var showcase = $(".showcase");

  function typeCode(panel) {
    if (!panel) return;
    var code = $("code", panel);
    if (!code) return;

    var raw = rawOf(code);
    var token = String(Date.now()) + Math.random();
    panel.dataset.typingToken = token;

    if (reduceMotion || !showcase) {
      highlightCode(code);
      panel.dataset.typed = "true";
      return;
    }

    var total = Math.max(48, Math.min(130, Math.ceil(raw.length / 2.2)));
    var chunk = Math.max(1, Math.ceil(raw.length / total));
    var index = 0;
    panel.classList.add("is-typing");
    code.textContent = "";
    panel.dataset.typed = "true";

    function step() {
      if (panel.dataset.typingToken !== token) return;
      index = Math.min(raw.length, index + chunk);
      code.textContent = raw.slice(0, index);
      if (index < raw.length) {
        window.setTimeout(step, 16);
        return;
      }
      panel.classList.remove("is-typing");
      highlightCode(code);
    }

    window.setTimeout(step, 30);
  }

  function initCodeTabs() {
    var tablist = $(".code-tabs");
    var panels = $$("[data-code-panel]");
    if (!tablist || !panels.length) return;

    var tabs = $$("[data-code-tab]", tablist);

    function activate(id, focus) {
      tabs.forEach(function (tab) {
        var active = tab.dataset.codeTab === id;
        tab.classList.toggle("is-active", active);
        tab.setAttribute("aria-selected", active ? "true" : "false");
        tab.tabIndex = active ? 0 : -1;
        if (active && focus) tab.focus();
      });

      panels.forEach(function (panel) {
        var active = panel.dataset.codePanel === id;
        panel.hidden = !active;
        panel.classList.toggle("is-active", active);
        if (active && !panel.dataset.typed) typeCode(panel);
      });
    }

    tabs.forEach(function (tab, index) {
      tab.addEventListener("click", function () {
        activate(tab.dataset.codeTab, false);
      });

      tab.addEventListener("keydown", function (event) {
        var next = null;
        if (event.key === "ArrowRight" || event.key === "ArrowDown") next = tabs[(index + 1) % tabs.length];
        if (event.key === "ArrowLeft" || event.key === "ArrowUp") next = tabs[(index - 1 + tabs.length) % tabs.length];
        if (event.key === "Home") next = tabs[0];
        if (event.key === "End") next = tabs[tabs.length - 1];
        if (!next) return;
        event.preventDefault();
        activate(next.dataset.codeTab, true);
      });
    });

    if (!showcase) {
      typeCode($(".code-panel.is-active"));
      return;
    }

    if ("IntersectionObserver" in window && !reduceMotion) {
      var showcaseObserver = new IntersectionObserver(
        function (entries) {
          entries.forEach(function (entry) {
            if (!entry.isIntersecting) return;
            var active = $(".code-panel.is-active");
            if (active && !active.dataset.typed) typeCode(active);
            showcaseObserver.disconnect();
          });
        },
        { threshold: 0.2, rootMargin: "0px 0px -8% 0px" }
      );
      showcaseObserver.observe(showcase);
    } else {
      typeCode($(".code-panel.is-active"));
    }
  }

  initCodeTabs();

  /* ---------- Pipeline tabs ---------- */

  function initPipelineTabs() {
    var rail = $(".pipeline-rail");
    if (!rail) return;
    var buttons = $$("[data-stage]", rail);
    var panels = $$("[data-stage-panel]");

    function activate(name, focus) {
      buttons.forEach(function (button) {
        var active = button.dataset.stage === name;
        button.classList.toggle("is-active", active);
        button.setAttribute("aria-selected", active ? "true" : "false");
        button.tabIndex = active ? 0 : -1;
        if (active && focus) button.focus();
      });

      panels.forEach(function (panel) {
        var active = panel.dataset.stagePanel === name;
        panel.hidden = !active;
        panel.classList.toggle("is-active", active);
      });
    }

    buttons.forEach(function (button, index) {
      button.addEventListener("click", function () {
        activate(button.dataset.stage, false);
      });

      button.addEventListener("keydown", function (event) {
        var next = null;
        if (event.key === "ArrowRight" || event.key === "ArrowDown") next = buttons[(index + 1) % buttons.length];
        if (event.key === "ArrowLeft" || event.key === "ArrowUp") next = buttons[(index - 1 + buttons.length) % buttons.length];
        if (event.key === "Home") next = buttons[0];
        if (event.key === "End") next = buttons[buttons.length - 1];
        if (!next) return;
        event.preventDefault();
        activate(next.dataset.stage, true);
      });
    });
  }

  initPipelineTabs();

  /* ---------- Copy buttons ---------- */

  function copyText(text) {
    if (navigator.clipboard && window.isSecureContext) {
      return navigator.clipboard.writeText(text);
    }

    return new Promise(function (resolve, reject) {
      var area = document.createElement("textarea");
      area.value = text;
      area.setAttribute("readonly", "");
      area.style.position = "fixed";
      area.style.top = "-1000px";
      area.style.opacity = "0";
      document.body.appendChild(area);
      area.select();
      try {
        document.execCommand("copy");
        resolve();
      } catch (error) {
        reject(error);
      } finally {
        document.body.removeChild(area);
      }
    });
  }

  document.addEventListener("click", function (event) {
    var button = event.target.closest("[data-copy]");
    if (!button) return;

    var scope = button.closest(".code-card, .code-panel, .terminal-mini, .install-terminal") || document;
    var code = $("pre code", scope);
    if (!code) return;

    var raw = rawOf(code);
    copyText(raw)
      .then(function () {
        showToast("Copied to clipboard");
        button.classList.add("is-copied");
        var label = $("span", button);
        var previous = label ? label.textContent : "";
        if (label) label.textContent = "Copied";
        window.setTimeout(function () {
          button.classList.remove("is-copied");
          if (label) label.textContent = previous;
        }, 1600);
      })
      .catch(function () {
        showToast("Copy failed. Select the code and press Ctrl+C.");
      });
  });

  /* ---------- Tilt and cursor light ---------- */

  if (finePointer && !reduceMotion) {
    $$("[data-tilt]").forEach(function (card) {
      var max = parseFloat(card.dataset.tiltMax || "2.4");

      card.addEventListener("pointermove", function (event) {
        var rect = card.getBoundingClientRect();
        var px = (event.clientX - rect.left) / rect.width;
        var py = (event.clientY - rect.top) / rect.height;
        var rx = (0.5 - py) * max * 2;
        var ry = (px - 0.5) * max * 2;

        card.style.setProperty("--mx", px * 100 + "%");
        card.style.setProperty("--my", py * 100 + "%");
        card.style.transform =
          "perspective(900px) rotateX(" + rx.toFixed(2) + "deg) rotateY(" + ry.toFixed(2) + "deg) translateY(-2px)";
      });

      card.addEventListener("pointerleave", function () {
        card.style.transform = "";
      });
    });
  }

  /* ---------- Hero parallax ---------- */

  var hero = $(".hero");
  var heroVisual = $(".hero-visual");
  var parallaxFrame = 0;

  if (hero && heroVisual && finePointer && !reduceMotion) {
    hero.addEventListener("pointermove", function (event) {
      var rect = hero.getBoundingClientRect();
      var px = (event.clientX - rect.left) / rect.width - 0.5;
      var py = (event.clientY - rect.top) / rect.height - 0.5;

      if (parallaxFrame) return;
      parallaxFrame = window.requestAnimationFrame(function () {
        heroVisual.style.transform =
          "translate3d(" + (px * -12).toFixed(2) + "px, " + (py * -8).toFixed(2) + "px, 0)";
        parallaxFrame = 0;
      });
    });

    hero.addEventListener("pointerleave", function () {
      heroVisual.style.transform = "";
    });
  }

  /* ---------- Particle field ---------- */

  var canvas = $("#particles");

  if (canvas && hero && !reduceMotion) {
    var context = canvas.getContext("2d");
    var particles = [];
    var particleFrame = 0;
    var particleWidth = 0;
    var particleHeight = 0;
    var dpr = Math.min(window.devicePixelRatio || 1, 2);
    var pointer = { x: -1000, y: -1000 };
    var visible = true;

    function particleColour(alpha) {
      return currentTheme() === "dark"
        ? "rgba(196, 181, 253, " + alpha + ")"
        : "rgba(124, 92, 255, " + alpha + ")";
    }

    function resizeCanvas() {
      var rect = hero.getBoundingClientRect();
      particleWidth = rect.width;
      particleHeight = rect.height;
      canvas.width = Math.round(particleWidth * dpr);
      canvas.height = Math.round(particleHeight * dpr);
      canvas.style.width = particleWidth + "px";
      canvas.style.height = particleHeight + "px";
      context.setTransform(dpr, 0, 0, dpr, 0, 0);

      var count = clamp(Math.floor(particleWidth / 22), 24, 64);
      particles = [];

      for (var i = 0; i < count; i += 1) {
        particles.push({
          x: Math.random() * particleWidth,
          y: Math.random() * particleHeight,
          vx: (Math.random() - 0.5) * 0.24,
          vy: (Math.random() - 0.5) * 0.24,
          r: Math.random() * 1.4 + 0.6
        });
      }
    }

    function drawParticles() {
      if (!visible || document.hidden) {
        particleFrame = 0;
        return;
      }

      context.clearRect(0, 0, particleWidth, particleHeight);

      for (var i = 0; i < particles.length; i += 1) {
        var p = particles[i];
        p.x += p.vx;
        p.y += p.vy;

        if (p.x < -10) p.x = particleWidth + 10;
        if (p.x > particleWidth + 10) p.x = -10;
        if (p.y < -10) p.y = particleHeight + 10;
        if (p.y > particleHeight + 10) p.y = -10;

        context.beginPath();
        context.fillStyle = particleColour(0.42);
        context.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        context.fill();

        for (var j = i + 1; j < particles.length; j += 1) {
          var q = particles[j];
          var dx = p.x - q.x;
          var dy = p.y - q.y;
          var dist = Math.sqrt(dx * dx + dy * dy);
          if (dist < 118) {
            context.beginPath();
            context.strokeStyle = particleColour((1 - dist / 118) * 0.13);
            context.lineWidth = 1;
            context.moveTo(p.x, p.y);
            context.lineTo(q.x, q.y);
            context.stroke();
          }
        }

        var mdx = p.x - pointer.x;
        var mdy = p.y - pointer.y;
        var mouseDist = Math.sqrt(mdx * mdx + mdy * mdy);
        if (mouseDist < 170) {
          context.beginPath();
          context.strokeStyle = particleColour((1 - mouseDist / 170) * 0.22);
          context.lineWidth = 1;
          context.moveTo(p.x, p.y);
          context.lineTo(pointer.x, pointer.y);
          context.stroke();
        }
      }

      particleFrame = window.requestAnimationFrame(drawParticles);
    }

    function startParticles() {
      if (particleFrame) return;
      particleFrame = window.requestAnimationFrame(drawParticles);
    }

    function stopParticles() {
      if (!particleFrame) return;
      window.cancelAnimationFrame(particleFrame);
      particleFrame = 0;
    }

    resizeCanvas();
    startParticles();

    window.addEventListener("resize", resizeCanvas);

    hero.addEventListener("pointermove", function (event) {
      var rect = canvas.getBoundingClientRect();
      pointer.x = event.clientX - rect.left;
      pointer.y = event.clientY - rect.top;
    });

    hero.addEventListener("pointerleave", function () {
      pointer.x = -1000;
      pointer.y = -1000;
    });

    if ("IntersectionObserver" in window) {
      var canvasObserver = new IntersectionObserver(
        function (entries) {
          visible = entries[0].isIntersecting;
          if (visible) startParticles();
          else stopParticles();
        },
        { threshold: 0 }
      );
      canvasObserver.observe(hero);
    }

    document.addEventListener("visibilitychange", function () {
      if (document.hidden) stopParticles();
      else if (visible) startParticles();
    });

    document.addEventListener("valvet:theme", function () {
      if (!particleFrame) startParticles();
    });
  }

  /* ---------- Count up ---------- */

  var counters = $$("[data-count]");

  function setCounterFinal(counter) {
    var target = parseFloat(counter.dataset.count);
    var decimals = parseInt(counter.dataset.decimals || "0", 10);
    counter.textContent = target.toFixed(decimals);
  }

  function animateCounter(counter) {
    if (counter.dataset.counted) return;
    counter.dataset.counted = "true";

    if (reduceMotion) {
      setCounterFinal(counter);
      return;
    }

    var target = parseFloat(counter.dataset.count);
    var decimals = parseInt(counter.dataset.decimals || "0", 10);
    var start = performance.now();
    var duration = 680;

    function frame(now) {
      var progress = clamp((now - start) / duration, 0, 1);
      var eased = 1 - Math.pow(1 - progress, 3);
      counter.textContent = (target * eased).toFixed(decimals);
      if (progress < 1) window.requestAnimationFrame(frame);
      else setCounterFinal(counter);
    }

    window.requestAnimationFrame(frame);
  }

  if (counters.length) {
    if ("IntersectionObserver" in window && !reduceMotion) {
      var counterObserver = new IntersectionObserver(
        function (entries) {
          entries.forEach(function (entry) {
            if (!entry.isIntersecting) return;
            animateCounter(entry.target);
            counterObserver.unobserve(entry.target);
          });
        },
        { threshold: 0.5 }
      );
      counters.forEach(function (counter) {
        counterObserver.observe(counter);
      });
    } else {
      counters.forEach(setCounterFinal);
    }
  }

  /* ---------- Standard library filter ---------- */

  var moduleFilter = $("#module-filter");
  var moduleItems = $$("[data-module]");
  var moduleCount = $("[data-module-count]");
  var moduleGroups = $$(".module-group");

  if (moduleFilter && moduleItems.length) {
    moduleFilter.addEventListener("input", function () {
      var query = moduleFilter.value.trim().toLowerCase();
      var visible = 0;

      moduleItems.forEach(function (item) {
        var match = !query || item.textContent.toLowerCase().indexOf(query) !== -1;
        item.classList.toggle("is-hidden", !match);
        if (match) visible += 1;
      });

      moduleGroups.forEach(function (group) {
        var hasVisible = $$("[data-module]", group).some(function (item) {
          return !item.classList.contains("is-hidden");
        });
        group.hidden = !hasVisible;
      });

      if (moduleCount) moduleCount.textContent = visible + (visible === 1 ? " module" : " modules");
    });
  }

  /* ---------- Documentation page ---------- */

  var docsContent = $("#docs-content");

  if (docsContent) {
    var docsToc = $(".docs-toc");
    if (docsToc && window.matchMedia("(min-width: 981px)").matches) docsToc.open = true;

    var docSections = $$("[data-doc-section]");
    var docLinks = $$("[data-doc-link]");
    var docSearch = $("#docs-search");
    var searchStatus = $("[data-search-status]");
    var progress = $("[data-progress]");
    var tocGroups = $$(".toc-group");
    var activeSection = "";
    var docFrame = 0;

    function updateDocsActive() {
      if (!docSections.length) return;

      var fromTop = window.scrollY + 150;
      var current = docSections[0].id;

      docSections.forEach(function (section) {
        if (section.getBoundingClientRect().top + window.scrollY <= fromTop) current = section.id;
      });

      if (current === activeSection) return;
      activeSection = current;

      docLinks.forEach(function (link) {
        link.classList.toggle("is-active", link.dataset.docLink === current);
      });
    }

    function updateProgress() {
      if (!progress) return;
      var rect = docsContent.getBoundingClientRect();
      var top = rect.top + window.scrollY;
      var height = docsContent.offsetHeight;
      var viewport = window.innerHeight;
      var total = Math.max(1, height - viewport);
      var value = ((window.scrollY - top + 120) / total) * 100;
      progress.style.width = clamp(value, 0, 100) + "%";
    }

    function onDocsScroll() {
      if (docFrame) return;
      docFrame = window.requestAnimationFrame(function () {
        updateDocsActive();
        updateProgress();
        docFrame = 0;
      });
    }

    updateDocsActive();
    updateProgress();
    window.addEventListener("scroll", onDocsScroll, { passive: true });

    if (docSearch) {
      docSearch.addEventListener("input", function () {
        var query = docSearch.value.trim().toLowerCase();
        var visible = 0;

        docLinks.forEach(function (link) {
          var section = document.getElementById(link.dataset.docLink);
          var text = section ? section.textContent.toLowerCase() : "";
          var match = !query || text.indexOf(query) !== -1;
          link.hidden = !match;
          if (match) visible += 1;
        });

        tocGroups.forEach(function (group) {
          var hasVisible = $$("a", group).some(function (link) {
            return !link.hidden;
          });
          group.hidden = !hasVisible;
        });

        if (searchStatus) {
          searchStatus.textContent = query
            ? visible + (visible === 1 ? " matching section" : " matching sections")
            : docLinks.length + " sections";
        }
      });

      if (searchStatus) searchStatus.textContent = docLinks.length + " sections";
    }
  }

  /* ---------- Examples page ---------- */

  var examplesContent = $(".examples-content");

  if (examplesContent) {
    var examplesToc = $(".examples-toc");
    if (examplesToc && window.matchMedia("(min-width: 981px)").matches) examplesToc.open = true;

    var filterButtons = $$("[data-filter]");
    var exampleCards = $$("[data-example-card]");
    var filterStatus = $("[data-filter-status]");
    var emptyState = $("[data-empty-state]");
    var exampleLinks = $$("[data-example-link]");
    var exampleSections = $$("[data-example-card]");
    var exampleActive = "";
    var exampleFrame = 0;

    function applyFilter(filter) {
      var visible = 0;

      filterButtons.forEach(function (button) {
        var active = button.dataset.filter === filter;
        button.classList.toggle("is-active", active);
        button.setAttribute("aria-pressed", active ? "true" : "false");
      });

      exampleCards.forEach(function (card) {
        var match = filter === "all" || card.dataset.category === filter;
        card.hidden = !match;
        if (match) visible += 1;
      });

      exampleLinks.forEach(function (link) {
        var match = filter === "all" || link.dataset.category === filter;
        link.hidden = !match;
      });

      if (filterStatus) {
        var noun = visible === 1 ? " example" : " examples";
        filterStatus.textContent =
          filter === "all"
            ? "Showing all " + visible + noun
            : "Showing " + visible + " " + filter + noun;
      }

      if (emptyState) emptyState.hidden = visible !== 0;
    }

    filterButtons.forEach(function (button) {
      button.addEventListener("click", function () {
        applyFilter(button.dataset.filter);
      });
    });

    function updateExampleActive() {
      if (!exampleSections.length) return;
      var fromTop = window.scrollY + 150;
      var current = exampleSections[0].id;

      exampleSections.forEach(function (section) {
        if (!section.hidden && section.getBoundingClientRect().top + window.scrollY <= fromTop) {
          current = section.id;
        }
      });

      if (current === exampleActive) return;
      exampleActive = current;

      exampleLinks.forEach(function (link) {
        link.classList.toggle("is-active", link.dataset.exampleLink === current);
      });
    }

    function onExamplesScroll() {
      if (exampleFrame) return;
      exampleFrame = window.requestAnimationFrame(function () {
        updateExampleActive();
        exampleFrame = 0;
      });
    }

    updateExampleActive();
    window.addEventListener("scroll", onExamplesScroll, { passive: true });
    applyFilter("all");
  }

  /* ---------- Initial theme event ---------- */

  document.dispatchEvent(new CustomEvent("valvet:theme", { detail: currentTheme() }));
})();
