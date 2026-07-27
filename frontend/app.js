// Чистый JS без сборщика — Tauri v2 при withGlobalTauri:true кладёт API
// в window.__TAURI__. Экраны "Прогресс" и "Ошибка" — это не отдельные
// страницы, а состояния action-area на экране запуска (кнопка ИГРАТЬ
// подменяется полосой прогресса на том же месте, см. ТЗ Этапа 4).
//
// Поверх этого — экран выбора версии (#screen-home): карточки сборок из
// manifest.versions. Клик по играбельной ("ready") карточке выбирает версию
// и открывает экран запуска уже в её контексте; "soon" — витрина «Скоро».

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const appWindow = getCurrentWindow();

const NICK_RE = /^[A-Za-z0-9_]{3,16}$/;

// Единственный канал видимости для отладки в этой сессии — форвардим
// ошибки в Rust-лог (см. lib.rs::frontend_log), т.к. напрямую посмотреть
// в окно лаунчера здесь нет возможности.
function flog(level, message) {
  invoke("frontend_log", { level, message }).catch(() => {});
}
window.addEventListener("error", (e) => flog("error", `window.onerror: ${e.message}`));
window.addEventListener("unhandledrejection", (e) => flog("error", `unhandledrejection: ${e.reason}`));

const state = {
  manifest: null,
  settings: null,
  // Выбранная версия (объект из manifest.versions) или null на экране выбора.
  selected: null,
};

const el = {
  homeScreen: document.getElementById("screen-home"),
  playScreen: document.getElementById("screen-play"),
  versionGrid: document.getElementById("version-grid"),
  btnHome: document.getElementById("btn-home"),
  titlebarTitle: document.getElementById("titlebar-title"),
  nickInput: document.getElementById("nick-input"),
  actionRow: document.getElementById("action-row"),
  playBtn: document.getElementById("btn-play"),
  progressArea: document.getElementById("progress-area"),
  progressFill: document.getElementById("progress-fill"),
  progressLabel: document.getElementById("progress-label"),
  errorArea: document.getElementById("error-area"),
  errorText: document.getElementById("error-text"),
  newsText: document.getElementById("news-text"),
  footerLinks: document.getElementById("footer-links"),
  settingsBtn: document.getElementById("btn-settings"),
  screenOptional: document.getElementById("screen-optional"),
  optionalList: document.getElementById("optional-list"),
  ramSlider: document.getElementById("ram-slider"),
  ramValue: document.getElementById("ram-value"),
  progressBar: document.getElementById("progress-bar"),
  progressPercent: document.getElementById("progress-percent"),
  progressTip: document.getElementById("progress-tip"),
  tipText: document.getElementById("tip-text"),
  tipTitle: document.getElementById("tip-title"),
  serverStatus: document.getElementById("server-status"),
  serverDot: document.getElementById("server-dot"),
  serverText: document.getElementById("server-text"),
  serverPing: document.getElementById("server-ping"),
  btnGameFolder: document.getElementById("btn-game-folder"),
  btnCopyIp: document.getElementById("btn-copy-ip"),
  copyIpLabel: document.getElementById("copy-ip-label"),
  newsPanel: document.getElementById("news-panel"),
  newsList: document.getElementById("news-list"),
  btnReinstall: document.getElementById("btn-reinstall"),
};

// ---------- Звук и эффекты кликов ----------

// Пул экземпляров: один <audio> не умеет играть сам поверх себя, и при
// быстрых кликах звук бы просто обрывался на середине.
const CLICK_POOL_SIZE = 4;
let clickPool = [];
let clickPoolIndex = 0;

function initClickSound() {
  for (let i = 0; i < CLICK_POOL_SIZE; i++) {
    const a = new Audio("assets/click.wav");
    a.preload = "auto";
    a.volume = 0.35;
    clickPool.push(a);
  }
}

function playClick() {
  if (state.settings && state.settings.sound_enabled === false) return;
  const a = clickPool[clickPoolIndex];
  clickPoolIndex = (clickPoolIndex + 1) % CLICK_POOL_SIZE;
  if (!a) return;
  try {
    a.currentTime = 0;
    a.play().catch(() => {}); // автоплей может быть заблокирован до первого жеста
  } catch (_) {}
}

const SPARKLE_COLORS = ["#ffb46b", "#ff8a3d", "#ffd9a0", "#7c5cff", "#fff2dd"];

/** Разлёт блёсток из точки клика. Чистый DOM: элементы живут ~700 мс. */
function burstSparkles(x, y) {
  const count = 14;
  for (let i = 0; i < count; i++) {
    const p = document.createElement("span");
    p.className = "sparkle";
    const angle = (Math.PI * 2 * i) / count + Math.random() * 0.5;
    const distance = 26 + Math.random() * 34;
    const size = 4 + Math.random() * 5;
    p.style.left = `${x}px`;
    p.style.top = `${y}px`;
    p.style.width = `${size}px`;
    p.style.height = `${size}px`;
    p.style.background = SPARKLE_COLORS[(Math.random() * SPARKLE_COLORS.length) | 0];
    p.style.setProperty("--dx", `${Math.cos(angle) * distance}px`);
    p.style.setProperty("--dy", `${Math.sin(angle) * distance}px`);
    p.style.animationDelay = `${Math.random() * 60}ms`;
    document.body.appendChild(p);
    setTimeout(() => p.remove(), 800);
  }
}

// Один делегированный обработчик на всё окно: не нужно вешать звук на
// каждую кнопку отдельно, включая те, что создаются динамически.
document.addEventListener(
  "click",
  (e) => {
    const target = e.target.closest("button, .version-card, .optional-item, .footer-link");
    if (!target || target.disabled) return;
    playClick();
    burstSparkles(e.clientX, e.clientY);
  },
  true
);

// ---------- Новости ----------

function renderNews(items) {
  if (!items || items.length === 0) {
    el.newsPanel.classList.add("hidden");
    return;
  }
  el.newsPanel.classList.remove("hidden");
  el.newsList.innerHTML = "";

  for (const item of items) {
    const card = document.createElement(item.url ? "button" : "div");
    card.className = "news-item";
    card.innerHTML = `
      ${item.date ? `<div class="news-date">${escapeHtml(item.date)}</div>` : ""}
      <div class="news-title">${escapeHtml(item.title || "")}</div>
      ${item.text ? `<div class="news-text">${escapeHtml(item.text)}</div>` : ""}
      ${item.url ? '<div class="news-more">Подробнее →</div>' : ""}
    `;
    if (item.url) {
      card.addEventListener("click", () =>
        invoke("open_url", { url: item.url }).catch((e) => flog("error", `open_url: ${e}`))
      );
    }
    el.newsList.appendChild(card);
  }
}

// ---------- Переустановка сборки ----------

let reinstallArmed = false;
let reinstallTimer = null;

function initReinstall() {
  el.btnReinstall.addEventListener("click", async () => {
    if (!state.selected) return;

    // Двухшаговое подтверждение вместо confirm(): в окне без рамки
    // системный диалог выглядит чужеродно, а операция необратимая.
    if (!reinstallArmed) {
      reinstallArmed = true;
      el.btnReinstall.textContent = "Точно? Нажмите ещё раз";
      el.btnReinstall.classList.add("armed");
      clearTimeout(reinstallTimer);
      reinstallTimer = setTimeout(resetReinstallButton, 4000);
      return;
    }

    clearTimeout(reinstallTimer);
    el.btnReinstall.disabled = true;
    el.btnReinstall.textContent = "Удаляем...";
    try {
      await invoke("reinstall_version", { versionId: state.selected.id });
      el.btnReinstall.textContent = "Готово — нажмите «Играть»";
    } catch (e) {
      flog("error", `reinstall_version: ${e}`);
      el.btnReinstall.textContent = "Ошибка";
    } finally {
      setTimeout(() => {
        el.btnReinstall.disabled = false;
        resetReinstallButton();
      }, 2500);
    }
  });
}

function resetReinstallButton() {
  reinstallArmed = false;
  el.btnReinstall.textContent = "Переустановить";
  el.btnReinstall.classList.remove("armed");
}

// ---------- Статус игрового сервера ----------

let serverTimer = null;

function serverAddress(ver) {
  const host = ver && ver.server && ver.server.host;
  if (!host) return null;
  const port = ver.server.port || 25565;
  return { host, port, display: port === 25565 ? host : `${host}:${port}` };
}

async function refreshServerStatus() {
  const addr = serverAddress(state.selected);
  if (!addr) {
    el.serverStatus.classList.add("hidden");
    el.btnCopyIp.classList.add("hidden");
    return;
  }
  el.serverStatus.classList.remove("hidden");
  el.btnCopyIp.classList.remove("hidden");

  try {
    const s = await invoke("get_server_status", { host: addr.host, port: addr.port });
    el.serverDot.classList.toggle("online", s.online);
    el.serverDot.classList.toggle("offline", !s.online);
    if (s.online) {
      const word = pluralPlayers(s.players_online);
      el.serverText.textContent = `Сервер онлайн · ${s.players_online} ${word}`;
      el.serverPing.textContent = `${s.ping_ms} мс`;
    } else {
      el.serverText.textContent = "Сервер недоступен";
      el.serverPing.textContent = "";
    }
  } catch (e) {
    flog("warn", `get_server_status: ${e}`);
    el.serverDot.classList.add("offline");
    el.serverText.textContent = "Сервер недоступен";
    el.serverPing.textContent = "";
  }
}

function pluralPlayers(n) {
  const mod10 = n % 10;
  const mod100 = n % 100;
  if (mod10 === 1 && mod100 !== 11) return "игрок";
  if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return "игрока";
  return "игроков";
}

function startServerPolling() {
  refreshServerStatus();
  clearInterval(serverTimer);
  serverTimer = setInterval(refreshServerStatus, 30000);
}

function stopServerPolling() {
  clearInterval(serverTimer);
  serverTimer = null;
}

// ---------- Быстрые действия ----------

el.btnGameFolder.addEventListener("click", () => {
  if (!state.selected) return;
  invoke("open_game_folder", { versionId: state.selected.id }).catch((e) =>
    flog("error", `open_game_folder: ${e}`)
  );
});

el.btnCopyIp.addEventListener("click", async () => {
  const addr = serverAddress(state.selected);
  if (!addr) return;
  try {
    await navigator.clipboard.writeText(addr.display);
    el.copyIpLabel.textContent = "Скопировано";
    setTimeout(() => (el.copyIpLabel.textContent = "Адрес сервера"), 1600);
  } catch (e) {
    flog("error", `clipboard: ${e}`);
  }
});

// ---------- Подсказки на время установки ----------

// Первая установка — это несколько минут и больше гигабайта загрузки.
// Крутим подсказки, чтобы ожидание не было пустым экраном с полоской.
const TIPS = [
  ["Управление", "Клавиша <b>R</b> над предметом в JEI покажет, как его скрафтить, а <b>U</b> — что из него делают."],
  ["Create", "Гаечный ключ поворачивает механизмы, а с <b>Shift</b> — разбирает их и возвращает в инвентарь."],
  ["Create", "Не гонитесь за скоростью: чем быстрее вращается механизм, тем больше он жрёт мощности. Иногда выгоднее поставить второй."],
  ["Create", "Кликните по механизму гаечным ключом, удерживая <b>Ctrl</b>, — увидите схему передачи вращения."],
  ["Совет", "Не выделяйте лаунчеру больше половины оперативной памяти компьютера — остальное нужно системе, иначе игра начнёт тормозить."],
  ["Совет", "Понадобился мод из списка дополнительных? Включите его в настройках — он докачается при следующем запуске."],
  ["Create", "Инженерный чертёж и Схематическая пушка позволяют копировать постройки. Очень выручает при строительстве заводов."],
  ["Производительность", "Если игра идёт рывками, убавьте дальность прорисовки до 8–10 чанков — на модовых сборках это помогает сильнее всего."],
  ["Знаете ли вы", "Все моды сборки обновляются сами: при каждом запуске лаунчер сверяет их с сервером и докачивает изменения."],
  ["Совет", "Ваши миры, скриншоты и настройки хранятся отдельно от модов, поэтому обновление сборки их не тронет."],
  ["Create", "Механический пресс, миксер и печь можно объединить в одну линию с воронками — так завод займёт меньше места."],
  ["Знаете ли вы", "Кнопка «Играть» не качает всё заново: сравниваются контрольные суммы, и скачивается только то, что изменилось."],
];

let tipTimer = null;
let tipIndex = 0;

function showNextTip() {
  const [title, text] = TIPS[tipIndex % TIPS.length];
  tipIndex += 1;
  el.progressTip.classList.remove("tip-visible");
  // Небольшая пауза — чтобы отработало затухание перед сменой текста
  setTimeout(() => {
    el.tipTitle.textContent = title;
    el.tipText.innerHTML = text;
    el.progressTip.classList.add("tip-visible");
  }, 220);
}

function startTips() {
  tipIndex = Math.floor(Math.random() * TIPS.length); // не всегда с одной и той же
  showNextTip();
  clearInterval(tipTimer);
  tipTimer = setInterval(showNextTip, 9000);
}

function stopTips() {
  clearInterval(tipTimer);
  tipTimer = null;
}

// ---------- Форматирование прогресса ----------

function formatBytes(n) {
  if (n >= 1024 * 1024 * 1024) return `${(n / 1024 / 1024 / 1024).toFixed(1)} ГБ`;
  if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} МБ`;
  if (n >= 1024) return `${Math.round(n / 1024)} КБ`;
  return `${n} Б`;
}

// ---------- Оперативная память ----------

function formatRam(mb) {
  return mb >= 1024 && mb % 1024 === 0 ? `${mb / 1024} ГБ` : `${mb} МБ`;
}

function initRamControl() {
  const mb = state.settings.ram_mb || 4096;
  el.ramSlider.value = mb;
  el.ramValue.textContent = formatRam(mb);

  // input — только рисуем подпись (событий много), change — сохраняем один раз
  el.ramSlider.addEventListener("input", () => {
    el.ramValue.textContent = formatRam(Number(el.ramSlider.value));
  });
  el.ramSlider.addEventListener("change", () => {
    state.settings.ram_mb = Number(el.ramSlider.value);
    invoke("save_settings", { settings: state.settings }).catch((e) => flog("error", `save_settings: ${e}`));
  });
}

// ---------- Титульная строка ----------

document.getElementById("btn-minimize").addEventListener("click", () => appWindow.minimize());
document.getElementById("btn-close").addEventListener("click", () => appWindow.close());
el.btnHome.addEventListener("click", goHome);

// ---------- Ник ----------

function validateNick() {
  const valid = NICK_RE.test(el.nickInput.value);
  el.nickInput.classList.toggle("valid", valid);
  el.nickInput.classList.toggle("invalid-shown", el.nickInput.value.length > 0 && !valid);
  updatePlayAvailability();
  return valid;
}

el.nickInput.addEventListener("input", validateNick);

function updatePlayAvailability() {
  el.playBtn.disabled = !NICK_RE.test(el.nickInput.value);
}

// ---------- Иконки для ссылок (без внешних файлов) ----------

const ICONS = {
  donate: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.6l-1-1a5.5 5.5 0 0 0-7.8 7.8l1 1L12 21l7.8-7.6 1-1a5.5 5.5 0 0 0 0-7.8z"></path></svg>',
  discord: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M20.3 5.4A18 18 0 0 0 15.6 4l-.3.6a13 13 0 0 1 3.9 1.6 15.6 15.6 0 0 0-13.9 0 13 13 0 0 1 3.9-1.6L8.9 4a18 18 0 0 0-4.7 1.4C1.8 9 1.1 12.5 1.4 16a18 18 0 0 0 5.4 2.7l.7-1.1a11 11 0 0 1-1.8-.9l.4-.3a13 13 0 0 0 11.6 0l.4.3a11 11 0 0 1-1.8.9l.7 1.1A18 18 0 0 0 22.1 16c.4-4-.6-7.5-1.8-10.6zM8.7 14c-.7 0-1.3-.7-1.3-1.5S8 11 8.7 11s1.3.7 1.3 1.5S9.4 14 8.7 14zm6.1 0c-.7 0-1.3-.7-1.3-1.5S14.1 11 14.8 11s1.3.7 1.3 1.5-.6 1.5-1.3 1.5z"></path></svg>',
  telegram: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 2 11 13"></path><path d="M22 2 15 22l-4-9-9-4 20-7z"></path></svg>',
};

function renderFooterLinks(links) {
  el.footerLinks.innerHTML = "";
  const entries = [
    ["donate", links.donate],
    ["discord", links.discord],
    ["telegram", links.telegram],
  ];
  for (const [key, url] of entries) {
    if (!url) continue;
    const a = document.createElement("button");
    a.className = "footer-link";
    a.title = key;
    a.innerHTML = ICONS[key] || "";
    a.addEventListener("click", () => invoke("open_url", { url }).catch((e) => flog("error", `open_url: ${e}`)));
    el.footerLinks.appendChild(a);
  }
}

function escapeHtml(s) {
  const div = document.createElement("div");
  div.textContent = s;
  return div.innerHTML;
}

// ---------- Экран выбора версии ----------

const CARD_ARROW =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"></path></svg>';

function renderVersionGrid(versions) {
  el.versionGrid.innerHTML = "";
  const themes = new Set(["orange", "purple", "default"]);

  for (const v of versions) {
    const ready = v.status === "ready";
    const theme = themes.has(v.theme) ? v.theme : "default";

    const card = document.createElement("button");
    card.type = "button";
    card.className = `version-card theme-${theme}${ready ? "" : " is-soon"}`;
    card.dataset.id = v.id;
    if (!ready) card.disabled = true;

    const badge = ready
      ? '<span class="vc-badge vc-badge-client">Клиент</span>'
      : '<span class="vc-badge vc-badge-soon">Скоро</span>';

    card.innerHTML = `
      <div class="version-card-media"></div>
      <div class="version-card-badges">${badge}</div>
      <div class="version-card-body">
        <div class="version-card-title">${escapeHtml(v.title)}</div>
        <div class="version-card-sub">${escapeHtml(v.subtitle || "")}</div>
      </div>
      <div class="version-card-arrow">${ready ? CARD_ARROW : ""}</div>
    `;

    if (ready) {
      card.addEventListener("click", () => selectVersion(v));
    }
    el.versionGrid.appendChild(card);
  }

  if (versions.length === 0) {
    el.versionGrid.innerHTML = '<div class="optional-empty">В манифесте нет ни одной версии.</div>';
  }
}

function selectVersion(v) {
  state.selected = v;
  el.titlebarTitle.textContent = v.title;
  el.btnHome.classList.remove("hidden");

  // Фон сборки включаем только внутри неё: экран выбора остаётся
  // нейтральным и не окрашен под одну из версий.
  applyVersionBackground(v.id);

  el.homeScreen.classList.add("hidden");
  el.playScreen.classList.remove("hidden");

  el.newsText.textContent = v.news || "";
  renderFooterLinks(state.manifest.links || {});
  showIdle();
  validateNick();
  startServerPolling();
}

/// Ставит на body класс с фоном выбранной сборки, снимая предыдущий.
/// `null` — вернуться к нейтральному фону главного экрана.
function applyVersionBackground(versionId) {
  for (const cls of Array.from(document.body.classList)) {
    if (cls.startsWith("bg-")) document.body.classList.remove(cls);
  }
  if (versionId) {
    document.body.classList.add(`bg-${versionId}`, "version-bg-on");
  } else {
    document.body.classList.remove("version-bg-on");
  }
}

function goHome() {
  state.selected = null;
  applyVersionBackground(null);
  stopServerPolling();
  el.screenOptional.classList.add("hidden");
  el.playScreen.classList.add("hidden");
  el.homeScreen.classList.remove("hidden");
  el.btnHome.classList.add("hidden");
  el.titlebarTitle.textContent = "Wanderlust";
}

// ---------- Прогресс / ошибка (состояния action-area) ----------

function setBackAvailable(available) {
  // Во время установки уходить с версии нельзя — прячем кнопку "назад".
  el.btnHome.classList.toggle("hidden", !available || !state.selected);
}

function showIdle() {
  el.actionRow.classList.remove("hidden");
  el.progressArea.classList.add("hidden");
  el.errorArea.classList.add("hidden");
  el.progressTip.classList.add("hidden");
  el.nickInput.classList.remove("hidden");
  stopTips();
  setBackAvailable(true);
}

function showProgress() {
  el.actionRow.classList.add("hidden");
  el.progressArea.classList.remove("hidden");
  el.errorArea.classList.add("hidden");
  // Ник во время установки менять уже поздно — на его месте показываем
  // подсказки, чтобы ожидание не было пустым.
  el.nickInput.classList.add("hidden");
  el.progressTip.classList.remove("hidden");
  startTips();
  setBackAvailable(false);
}

let lastErrorText = "";

function showError(message) {
  lastErrorText = message;
  el.actionRow.classList.add("hidden");
  el.progressArea.classList.add("hidden");
  el.errorArea.classList.remove("hidden");
  el.progressTip.classList.add("hidden");
  el.nickInput.classList.remove("hidden");
  stopTips();
  el.errorText.textContent = message;
  setBackAvailable(true);
}

document.getElementById("btn-error-back").addEventListener("click", showIdle);
document.getElementById("btn-open-logs").addEventListener("click", () => {
  invoke("open_logs_folder").catch((e) => flog("error", `open_logs_folder: ${e}`));
});
document.getElementById("btn-copy-log").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText(lastErrorText);
  } catch (e) {
    flog("error", `clipboard: ${e}`);
  }
});

listen("progress", (event) => {
  const { label, current, total, unit } = event.payload;

  // total = 0 или 1 — объём работы неизвестен (распаковка, установка
  // NeoForge). Показываем бегущую заливку вместо застывшего нуля.
  const determinate = total > 1;
  el.progressBar.classList.toggle("indeterminate", !determinate);

  if (determinate) {
    const pct = Math.min(100, Math.round((current / total) * 100));
    el.progressFill.style.width = `${pct}%`;
    el.progressPercent.textContent = `${pct}%`;
    const detail =
      unit === "bytes"
        ? `${formatBytes(current)} / ${formatBytes(total)}`
        : `${current} / ${total}`;
    el.progressLabel.textContent = `${label} · ${detail}`;
  } else {
    el.progressFill.style.width = "100%";
    el.progressPercent.textContent = "";
    el.progressLabel.textContent = label;
  }
});

listen("game-exited", (event) => {
  const code = event.payload;
  flog("info", `game-exited code=${code}`);
  appWindow.unminimize();
  appWindow.setFocus();
  if (code !== 0) {
    showError(
      `Игра завершилась с ошибкой (код ${code}). Нажмите «Открыть логи» — ` +
        `нужен файл game.log, в нём причина.`
    );
  } else {
    showIdle();
  }
});

// ---------- Кнопка ИГРАТЬ ----------

el.playBtn.addEventListener("click", async () => {
  if (el.playBtn.disabled || !state.selected) return;
  showProgress();
  el.progressFill.style.width = "0%";
  el.progressLabel.textContent = "Подготовка...";

  try {
    await invoke("launch", { versionId: state.selected.id, nick: el.nickInput.value });
    // Пайплайн установки завершился и игра реально запущена (spawn прошёл) —
    // сворачиваем лаунчер, не закрываем. Возврат/ошибку игры отследит
    // слушатель события game-exited выше.
    await appWindow.minimize();
  } catch (e) {
    flog("error", `launch failed: ${e}`);
    showError(String(e));
  }
});

// ---------- Экран опциональных модов ----------

document.getElementById("btn-settings").addEventListener("click", openOptionalScreen);
document.getElementById("btn-back-optional").addEventListener("click", () => {
  el.screenOptional.classList.add("hidden");
});

async function openOptionalScreen() {
  if (!state.selected) return;

  el.screenOptional.classList.remove("hidden");
  el.optionalList.innerHTML = '<div class="optional-empty">Загрузка списка модов...</div>';

  try {
    const mods = await invoke("get_optional_mods", { packwizUrl: state.selected.pack.packwiz_url });
    renderOptionalMods(mods);
  } catch (e) {
    flog("error", `get_optional_mods: ${e}`);
    el.optionalList.innerHTML = `<div class="optional-empty">Не удалось загрузить список: ${escapeHtml(String(e))}</div>`;
  }
}

function renderOptionalMods(mods) {
  el.optionalList.innerHTML = "";
  if (mods.length === 0) {
    el.optionalList.innerHTML = '<div class="optional-empty">У этой сборки нет опциональных модов.</div>';
    return;
  }

  // Выбор опциональных модов хранится раздельно по версиям (см. settings.rs).
  const versionId = state.selected.id;
  if (!state.settings.optional_mods) state.settings.optional_mods = {};
  const saved = state.settings.optional_mods[versionId] || {};

  for (const mod of mods) {
    const checked = Object.prototype.hasOwnProperty.call(saved, mod.id) ? saved[mod.id] : mod.default_value;

    const item = document.createElement("label");
    item.className = "optional-item";
    const sizeText = mod.size_bytes ? `${(mod.size_bytes / 1024 / 1024).toFixed(1)} МБ` : "";
    item.innerHTML = `
      <input type="checkbox" ${checked ? "checked" : ""} />
      <div class="optional-item-body">
        <div class="optional-item-name">${escapeHtml(mod.name)}</div>
        <div class="optional-item-desc">${escapeHtml(mod.description || "")}</div>
        ${sizeText ? `<div class="optional-item-size">${sizeText}</div>` : ""}
      </div>
    `;
    const checkbox = item.querySelector("input");
    checkbox.addEventListener("change", () => {
      if (!state.settings.optional_mods) state.settings.optional_mods = {};
      if (!state.settings.optional_mods[versionId]) state.settings.optional_mods[versionId] = {};
      state.settings.optional_mods[versionId][mod.id] = checkbox.checked;
      invoke("save_settings", { settings: state.settings }).catch((e) => flog("error", `save_settings: ${e}`));
    });
    el.optionalList.appendChild(item);
  }
}

// ---------- Инициализация ----------

async function init() {
  try {
    state.settings = await invoke("get_settings");
  } catch (e) {
    flog("error", `get_settings: ${e}`);
    state.settings = { nickname: "", ram_mb: 2048, optional_mods: {} };
  }

  el.nickInput.value = state.settings.nickname || "";
  validateNick();
  initRamControl();
  initClickSound();
  initReinstall();

  try {
    state.manifest = await invoke("get_manifest");
  } catch (e) {
    flog("error", `get_manifest: ${e}`);
    el.versionGrid.innerHTML = `<div class="optional-empty">Не удалось загрузить манифест: ${escapeHtml(String(e))}</div>`;
    return;
  }

  renderVersionGrid(state.manifest.versions || []);
  renderNews(state.manifest.news_feed || []);

  flog("info", "frontend initialised");

  // Проверка обновлений лаунчера — не блокирует показ окна (оно уже
  // нарисовано к этому моменту), делается тихо в фоне.
  invoke("check_for_update").catch((e) => flog("warn", `check_for_update: ${e}`));
}

init();
