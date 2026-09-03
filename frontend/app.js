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
  // Сведения о железе (ОЗУ) — для подсказок в настройках.
  system: null,
  // Полный список опциональных модов текущей сборки (включая скрытые
  // библиотеки) — нужен, чтобы разрешать зависимости при переключении.
  optionalMods: [],
  // Состояние экрана модов: строка поиска и фильтр («все» / «включённые»).
  modsQuery: "",
  modsFilter: "all",
  modsSort: "name",
  // Выбранная версия (объект из manifest.versions) или null на экране выбора.
  selected: null,
  // Был ли сервер недоступен на прошлой проверке — чтобы заметить, что он
  // поднялся, и сказать об этом ровно один раз.
  serverWasOffline: false,
};

const el = {
  homeScreen: document.getElementById("screen-home"),
  playScreen: document.getElementById("screen-play"),
  versionGrid: document.getElementById("version-grid"),
  btnHome: document.getElementById("btn-home"),
  titlebarTitle: document.getElementById("titlebar-title"),
  nickInput: document.getElementById("nick-input"),
  nickField: document.getElementById("nick-field"),
  actionRow: document.getElementById("action-row"),
  playBtn: document.getElementById("btn-play"),
  progressArea: document.getElementById("progress-area"),
  progressFill: document.getElementById("progress-fill"),
  progressLabel: document.getElementById("progress-label"),
  errorArea: document.getElementById("error-area"),
  errorText: document.getElementById("error-text"),
  errorDetail: document.getElementById("error-detail"),
  btnErrorDetail: document.getElementById("btn-error-detail"),
  newsText: document.getElementById("news-text"),
  footerLinks: document.getElementById("footer-links"),
  screenUpdate: document.getElementById("screen-update"),
  updateTitle: document.getElementById("update-title"),
  updateSub: document.getElementById("update-sub"),
  updateFill: document.getElementById("update-fill"),
  updateSize: document.getElementById("update-size"),
  updatePercent: document.getElementById("update-percent"),
  settingsBtn: document.getElementById("btn-settings"),
  screenOptional: document.getElementById("screen-optional"),
  optionalList: document.getElementById("optional-list"),
  screenMods: document.getElementById("screen-mods"),
  btnMods: document.getElementById("btn-mods"),
  modsBadge: document.getElementById("mods-badge"),
  modsSearch: document.getElementById("mods-search-input"),
  modsSummary: document.getElementById("mods-summary"),
  modDetails: document.getElementById("mod-details"),
  modDetailsIcon: document.getElementById("mod-details-icon"),
  modDetailsName: document.getElementById("mod-details-name"),
  modDetailsSub: document.getElementById("mod-details-sub"),
  modDetailsBody: document.getElementById("mod-details-body"),
  modDetailsSwitch: document.getElementById("mod-details-switch"),
  modDetailsSwitchLabel: document.getElementById("mod-details-switch-label"),
  ramSlider: document.getElementById("ram-slider"),
  ramValue: document.getElementById("ram-value"),
  ramHint: document.getElementById("ram-hint"),
  ramWarn: document.getElementById("ram-warn"),
  playtime: document.getElementById("playtime"),
  progressBar: document.getElementById("progress-bar"),
  progressPercent: document.getElementById("progress-percent"),
  progressTip: document.getElementById("progress-tip"),
  tipText: document.getElementById("tip-text"),
  tipTitle: document.getElementById("tip-title"),
  serverStatus: document.getElementById("server-status"),
  serverDot: document.getElementById("server-dot"),
  serverText: document.getElementById("server-text"),
  serverPing: document.getElementById("server-ping"),
  serverPlayersTip: document.getElementById("server-players-tip"),
  btnGameFolder: document.getElementById("btn-game-folder"),
  btnCopyIp: document.getElementById("btn-copy-ip"),
  copyIpLabel: document.getElementById("copy-ip-label"),
  newsPanel: document.getElementById("news-panel"),
  newsList: document.getElementById("news-list"),
  btnReinstall: document.getElementById("btn-reinstall"),
  btnCode: document.getElementById("btn-code"),
  codeModal: document.getElementById("code-modal"),
  codeInput: document.getElementById("code-input"),
  codeStatus: document.getElementById("code-status"),
  codeSubmit: document.getElementById("code-submit"),
  toasts: document.getElementById("toasts"),
  progressMeta: document.getElementById("progress-meta"),
  optSound: document.getElementById("opt-sound"),
  optEffects: document.getElementById("opt-effects"),
  totalPlaytime: document.getElementById("total-playtime"),
  launcherVersion: document.getElementById("launcher-version"),
  modsSort: document.getElementById("mods-sort"),
  nickHint: document.getElementById("nick-hint"),
  installSize: document.getElementById("install-size"),
  btnCheckUpdate: document.getElementById("btn-check-update"),
  modsClear: document.getElementById("btn-mods-clear"),
};

// ---------- Всплывающие уведомления ----------

/// Короткое сообщение в углу. Раньше каждая кнопка подменяла свою подпись —
/// «Скопировано» было видно, только если смотреть ровно на неё.
function toast(text, kind = "ok") {
  if (!el.toasts) return;
  const t = document.createElement("div");
  t.className = `toast toast-${kind}`;
  t.textContent = text;
  el.toasts.appendChild(t);
  // Уходит сам; класс снимаем заранее, чтобы отработало затухание.
  setTimeout(() => t.classList.add("toast-out"), 2400);
  setTimeout(() => t.remove(), 2800);
}

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
    // Кнопки окна — не место для праздника: искры из крестика выглядят
    // издевательством над тем, кто хочет закрыть лаунчер.
    if (target.closest(".titlebar")) return;
    if (state.settings && state.settings.effects_enabled === false) return;
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
    // Сервер перезапускали, и он вернулся, пока лаунчер открыт — об этом
    // стоит сказать: иначе игрок сидит и жмёт «Играть» наугад.
    if (state.serverWasOffline && s.online) toast("Сервер снова онлайн");
    state.serverWasOffline = !s.online;
    el.serverDot.classList.toggle("online", s.online);
    el.serverDot.classList.toggle("offline", !s.online);
    if (s.online) {
      const word = pluralPlayers(s.players_online);
      // Потолок сервера показываем, если он известен: «7 игроков» и
      // «7 из 40» читаются по-разному, когда решаешь, заходить ли сейчас.
      const count = s.players_max ? `${s.players_online} из ${s.players_max}` : `${s.players_online} ${word}`;
      el.serverText.textContent = `Сервер онлайн · ${count}`;
      el.serverPing.textContent = `${s.ping_ms} мс`;
    } else {
      el.serverText.textContent = "Сервер недоступен";
      el.serverPing.textContent = "";
    }
    buildPlayersTip(s);
  } catch (e) {
    flog("warn", `get_server_status: ${e}`);
    state.serverWasOffline = true;
    el.serverDot.classList.add("offline");
    el.serverText.textContent = "Сервер недоступен";
    el.serverPing.textContent = "";
    buildPlayersTip({ online: false, players_online: 0, players_sample: [] });
  }
}

// Список ников, всплывающий при наведении на плашку статуса. Тултип
// показывается только когда кто-то онлайн (класс has-players управляет
// этим из CSS). Сервер отдаёт лишь выборку имён — если онлайна больше,
// дописываем «и ещё N».
function buildPlayersTip(s) {
  const hasPlayers = s.online && s.players_online > 0;
  el.serverStatus.classList.toggle("has-players", hasPlayers);
  if (!hasPlayers) {
    el.serverPlayersTip.innerHTML = "";
    return;
  }

  const names = s.players_sample || [];
  if (names.length === 0) {
    // Онлайн есть, но сервер не прислал имён (частая настройка).
    el.serverPlayersTip.innerHTML =
      '<div class="tip-title">Сейчас играют</div><div class="tip-note">Сервер не показывает список ников</div>';
    return;
  }

  const rows = names.map((n) => `<div class="tip-player">${escapeHtml(n)}</div>`).join("");
  const more = s.players_online - names.length;
  const moreRow = more > 0 ? `<div class="tip-note">и ещё ${more}</div>` : "";
  el.serverPlayersTip.innerHTML = `<div class="tip-title">Сейчас играют</div>${rows}${moreRow}`;
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
    toast(`Адрес скопирован: ${addr.display}`);
    setTimeout(() => (el.copyIpLabel.textContent = "Адрес сервера"), 1600);
  } catch (e) {
    flog("error", `clipboard: ${e}`);
  }
});

// ---------- Подсказки на время установки ----------

// Первая установка — это несколько минут и больше гигабайта загрузки.
// Крутим подсказки, чтобы ожидание не было пустым экраном с полоской.
// Третий элемент — id сборки, для которой подсказка имеет смысл. Без него
// подсказка общая. Игроку закрытой сборки советы про гаечный ключ Create
// не говорят ничего, а место занимают.
const TIPS = [
  ["Управление", "Клавиша <b>R</b> над предметом в JEI покажет, как его скрафтить, а <b>U</b> — что из него делают."],
  ["Совет", "Не выделяйте лаунчеру больше половины оперативной памяти компьютера — остальное нужно системе, иначе игра начнёт тормозить."],
  ["Совет", "Понадобился мод из списка дополнительных? Включите его в настройках — он докачается при следующем запуске."],
  ["Производительность", "Если игра идёт рывками, убавьте дальность прорисовки до 8–10 чанков — на модовых сборках это помогает сильнее всего."],
  ["Знаете ли вы", "Все моды сборки обновляются сами: при каждом запуске лаунчер сверяет их с сервером и докачивает изменения."],
  ["Совет", "Ваши миры, скриншоты и настройки хранятся отдельно от модов, поэтому обновление сборки их не тронет."],
  ["Знаете ли вы", "Кнопка «Играть» не качает всё заново: сравниваются контрольные суммы, и скачивается только то, что изменилось."],
  ["Совет", "Свои моды можно просто положить в папку сборки — лаунчер их не тронет ни при обновлении, ни при переустановке."],
  ["Управление", "<b>Ctrl+M</b> открывает список дополнительных модов, <b>Ctrl+,</b> — настройки."],

  ["Create", "Гаечный ключ поворачивает механизмы, а с <b>Shift</b> — разбирает их и возвращает в инвентарь.", "wanderlust-create"],
  ["Create", "Не гонитесь за скоростью: чем быстрее вращается механизм, тем больше он жрёт мощности. Иногда выгоднее поставить второй.", "wanderlust-create"],
  ["Create", "Кликните по механизму гаечным ключом, удерживая <b>Ctrl</b>, — увидите схему передачи вращения.", "wanderlust-create"],
  ["Create", "Инженерный чертёж и Схематическая пушка позволяют копировать постройки. Очень выручает при строительстве заводов.", "wanderlust-create"],
  ["Create", "Механический пресс, миксер и печь можно объединить в одну линию с воронками — так завод займёт меньше места.", "wanderlust-create"],

  ["Шейдеры", "Включите Iris в списке модов — вместе с ним подтянется Sodium, и появится поддержка шейдерпаков.", "stray-souls"],
  ["Производительность", "Sodium можно включить и без Iris: он поднимает FPS сам по себе, а шейдеры видеокарту наоборот нагружают.", "stray-souls"],
];

let tipTimer = null;
let tipIndex = 0;

/// Подсказки, подходящие текущей сборке: общие плюс её собственные.
function tipsForCurrentVersion() {
  const id = state.selected && state.selected.id;
  return TIPS.filter((t) => !t[2] || t[2] === id);
}

function showNextTip() {
  const pool = tipsForCurrentVersion();
  const [title, text] = pool[tipIndex % pool.length];
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
  // Не всегда с одной и той же.
  tipIndex = Math.floor(Math.random() * tipsForCurrentVersion().length);
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

/// Показывает подсказку под ползунком и, если надо, предупреждение.
/// Выше безопасного потолка Java заберёт память, которой физически нет, и
/// система уйдёт в своп: игра не падает, а просто дико тормозит — и виноватым
/// в глазах игрока оказывается сервер.
function updateRamHint(mb) {
  const sys = state.system;
  if (!sys || !sys.total_ram_mb) return;

  el.ramHint.textContent =
    `На этом компьютере ${formatRam(sys.total_ram_mb)} памяти, ` +
    `рекомендуем ${formatRam(sys.recommended_ram_mb)}.`;

  if (mb > sys.safe_max_ram_mb) {
    el.ramWarn.textContent =
      `Слишком много: системе не останется памяти, игра будет тормозить. ` +
      `Максимум для этого компьютера — ${formatRam(sys.safe_max_ram_mb)}.`;
    el.ramWarn.hidden = false;
  } else {
    el.ramWarn.hidden = true;
  }
}

function initRamControl() {
  const sys = state.system;

  // Ползунок не должен предлагать заведомо невозможные значения.
  if (sys && sys.total_ram_mb) {
    el.ramSlider.max = Math.max(sys.safe_max_ram_mb, 2048);
  }

  // Если игрок ещё ничего не выбирал, ставим подобранное по железу, а не
  // фиксированные 4 ГБ: на 8 ГБ это впритык, на 32 ГБ — необоснованно мало.
  let mb = state.settings.ram_mb;
  if (!mb && sys && sys.recommended_ram_mb) {
    mb = sys.recommended_ram_mb;
    state.settings.ram_mb = mb;
    invoke("save_settings", { settings: state.settings }).catch(() => {});
  }
  mb = mb || 4096;

  el.ramSlider.value = mb;
  el.ramValue.textContent = formatRam(mb);
  updateRamHint(mb);

  // input — только рисуем подпись (событий много), change — сохраняем один раз
  el.ramSlider.addEventListener("input", () => {
    const v = Number(el.ramSlider.value);
    el.ramValue.textContent = formatRam(v);
    updateRamHint(v);
  });
  el.ramSlider.addEventListener("change", () => {
    state.settings.ram_mb = Number(el.ramSlider.value);
    invoke("save_settings", { settings: state.settings }).catch((e) => flog("error", `save_settings: ${e}`));
  });
}

/// «12 ч 30 мин» — часы отдельно, потому что «750 мин» читается плохо.
function formatPlaytime(seconds) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h === 0) return `${m} мин`;
  if (m === 0) return `${h} ч`;
  return `${h} ч ${m} мин`;
}

/// Наиграно в выбранной сборке. Пока меньше минуты — не показываем вовсе,
/// чтобы у нового игрока не висело «0 мин».
async function refreshPlaytime(versionId) {
  if (!el.playtime) return;
  try {
    const pt = await invoke("get_playtime", { versionId });
    if (!pt || !pt.version_seconds) {
      el.playtime.hidden = true;
      return;
    }
    el.playtime.textContent = `Наиграно ${formatPlaytime(pt.version_seconds)}`;
    el.playtime.hidden = false;
  } catch (e) {
    flog("warn", `get_playtime: ${e}`);
    el.playtime.hidden = true;
  }
}

// ---------- Титульная строка ----------

document.getElementById("btn-minimize").addEventListener("click", () => appWindow.minimize());

// Закрытие во время установки требует подтверждения. Оборванная на середине
// установка оставляет сборку в половинчатом состоянии, и следующий запуск
// начинается с докачки — а игрок к тому моменту уже уверен, что «лаунчер
// сломался». Один лишний клик дешевле такого разговора.
let closeArmed = false;
let closeTimer = null;
document.getElementById("btn-close").addEventListener("click", () => {
  const installing = !el.progressArea.classList.contains("hidden");
  if (!installing || closeArmed) {
    appWindow.close();
    return;
  }
  closeArmed = true;
  toast("Идёт установка. Нажмите крестик ещё раз, чтобы всё-таки закрыть", "err");
  clearTimeout(closeTimer);
  closeTimer = setTimeout(() => (closeArmed = false), 4000);
});
el.btnHome.addEventListener("click", goHome);

// ---------- Ник ----------

function validateNick() {
  const value = el.nickInput.value;
  const valid = NICK_RE.test(value);
  el.nickInput.classList.toggle("valid", valid);
  el.nickInput.classList.toggle("invalid-shown", value.length > 0 && !valid);
  el.nickHint.textContent = nickHintFor(value, valid);
  el.nickHint.classList.toggle("nick-hint-bad", value.length > 0 && !valid);
  updatePlayAvailability();
  return valid;
}

/// Раньше под полем всегда висело «3–16 символов», и игрок с ником «Вася»
/// видел красную рамку без единого намёка, что не так именно с ним.
function nickHintFor(value, valid) {
  if (value.length === 0) return "3–16 символов";
  if (valid) return "Всё в порядке";
  if (value.length < 3) return "Слишком короткий — нужно хотя бы 3 символа";

  const bad = [...new Set(value.split("").filter((c) => !/[A-Za-z0-9_]/.test(c)))];
  if (bad.length) {
    const cyrillic = bad.some((c) => /[А-Яа-яЁё]/.test(c));
    return cyrillic
      ? "Только латиница — Minecraft не пускает с русскими буквами"
      : `Нельзя использовать: ${bad.join(" ")}`;
  }
  return "3–16 символов";
}

el.nickInput.addEventListener("input", validateNick);

// Ник сохраняем сразу, как только он стал корректным. Раньше он записывался
// только при удачном запуске: игрок мог ввести ник, полистать моды, закрыть
// лаунчер — и в следующий раз вводить заново.
let nickSaveTimer = null;
el.nickInput.addEventListener("input", () => {
  if (!NICK_RE.test(el.nickInput.value) || !state.settings) return;
  clearTimeout(nickSaveTimer);
  nickSaveTimer = setTimeout(() => {
    if (state.settings.nickname === el.nickInput.value) return;
    state.settings.nickname = el.nickInput.value;
    invoke("save_settings", { settings: state.settings }).catch(() => {});
  }, 600);
});

// Ввёл ник — нажал Enter. Тянуться мышкой к кнопке ради этого не нужно.
el.nickInput.addEventListener("keydown", (ev) => {
  if (ev.key === "Enter" && !el.playBtn.disabled) el.playBtn.click();
});

function updatePlayAvailability() {
  el.playBtn.disabled = !NICK_RE.test(el.nickInput.value);
}

// ---------- Иконки для ссылок (без внешних файлов) ----------

const ICONS = {
  donate: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.6l-1-1a5.5 5.5 0 0 0-7.8 7.8l1 1L12 21l7.8-7.6 1-1a5.5 5.5 0 0 0 0-7.8z"></path></svg>',
  discord: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M20.3 5.4A18 18 0 0 0 15.6 4l-.3.6a13 13 0 0 1 3.9 1.6 15.6 15.6 0 0 0-13.9 0 13 13 0 0 1 3.9-1.6L8.9 4a18 18 0 0 0-4.7 1.4C1.8 9 1.1 12.5 1.4 16a18 18 0 0 0 5.4 2.7l.7-1.1a11 11 0 0 1-1.8-.9l.4-.3a13 13 0 0 0 11.6 0l.4.3a11 11 0 0 1-1.8.9l.7 1.1A18 18 0 0 0 22.1 16c.4-4-.6-7.5-1.8-10.6zM8.7 14c-.7 0-1.3-.7-1.3-1.5S8 11 8.7 11s1.3.7 1.3 1.5S9.4 14 8.7 14zm6.1 0c-.7 0-1.3-.7-1.3-1.5S14.1 11 14.8 11s1.3.7 1.3 1.5-.6 1.5-1.3 1.5z"></path></svg>',
  telegram: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 2 11 13"></path><path d="M22 2 15 22l-4-9-9-4 20-7z"></path></svg>',
  card: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="5" width="20" height="14" rx="2.5"></rect><path d="M2 10h20"></path></svg>',
};

/// Футер — ряд одиночных иконок без подписей: ссылки не должны спорить за
/// внимание с кнопкой «Играть». Что это за иконка, подсказывает всплывающий
/// заголовок при наведении.
function renderFooterLinks(links) {
  el.footerLinks.innerHTML = "";

  const entries = [
    ["discord", links.discord, "Discord"],
    ["telegram", links.telegram, "Telegram"],
    ["donate", links.donate, "Поддержать проект"],
  ];
  for (const [key, url, hint] of entries) {
    if (!url) continue;
    const b = document.createElement("button");
    b.className = `footer-link footer-link-${key}`;
    b.title = hint;
    b.innerHTML = ICONS[key] || "";
    b.addEventListener("click", () => invoke("open_url", { url }).catch((e) => flog("error", `open_url: ${e}`)));
    el.footerLinks.appendChild(b);
  }

  // Карта — такая же иконка в общем ряду. Клик копирует номер в буфер:
  // отдельный блок с 16 цифрами занимал место и лез в глаза.
  if (links.card) {
    const digits = links.card.replace(/\s+/g, "");
    const pretty = digits.replace(/(.{4})/g, "$1 ").trim();
    const b = document.createElement("button");
    b.className = "footer-link footer-link-card";
    b.title = `${links.card_note || "Перевод на карту без комиссии"}: ${pretty}\nНажмите, чтобы скопировать`;
    b.innerHTML = ICONS.card;
    b.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(digits);
        b.classList.add("copied");
        b.title = `Скопировано: ${pretty}`;
        setTimeout(() => {
          b.classList.remove("copied");
          b.title = `${links.card_note || "Перевод на карту без комиссии"}: ${pretty}\nНажмите, чтобы скопировать`;
        }, 1600);
      } catch (e) {
        flog("error", `clipboard card: ${e}`);
      }
    });
    el.footerLinks.appendChild(b);
  }
}

// ---------- Человеческий текст ошибок ----------

// Технический текст полезен нам в логе, но игроку он не говорит ничего.
// «error sending request for url ... dns error» и «os error 10060» — это
// для него одно и то же: «не работает». Переводим на понятное и, главное,
// подсказываем, что делать.
const ERROR_HINTS = [
  [/dns error|tcp connect|connection refused|ConnectFailure|отверг запрос|Connect\)/i,
   "Нет связи с сервером. Проверьте интернет и попробуйте ещё раз."],
  [/timed out|os error 10060|не получен нужный отклик|timeout/i,
   "Сервер долго не отвечает. Обычно помогает повторить попытку или включить VPN."],
  [/принудительно разорвал|10054|Обрыв соединения/i,
   "Соединение оборвалось на середине. Нажмите ещё раз — докачается только недостающее."],
  [/Контрольная сумма|hash|повреждён при скачивании/i,
   "Файл скачался повреждённым. Повторите запуск — лаунчер перекачает его заново."],
  [/нет места|os error 112|ENOSPC/i,
   "На диске закончилось место. Освободите пару гигабайт и попробуйте снова."],
  [/Invalid mod file hash/i,
   "Список модов на сервере только что обновился. Нажмите «Играть» ещё раз."],
  [/403|Forbidden/i,
   "Сервер отказал в доступе. Если пользуетесь VPN — попробуйте выключить его."],
  [/404|Not Found/i,
   "Файл не найден на сервере. Скорее всего, идёт обновление сборки — попробуйте через пару минут."],
  [/OutOfMemory|Java heap/i,
   "Игре не хватило памяти. Увеличьте её в настройках лаунчера."],
];

/// Короткое понятное объяснение. Полный текст остаётся доступен кнопкой
/// «Скопировать» — он нужен, когда игрок приходит за помощью.
function humanError(err) {
  const raw = String(err && err.message ? err.message : err);
  for (const [re, text] of ERROR_HINTS) {
    if (re.test(raw)) return text;
  }
  // Не нашли знакомого — показываем как есть, но без служебного «Error:».
  return raw.replace(/^Error:\s*/i, "");
}

/// Блок «пусто/не получилось» с кнопкой повтора. Возвращает элемент, чтобы
/// вызывающий сам решил, куда его положить.
function stateBox({ icon, title, text, actionLabel, onAction }) {
  const box = document.createElement("div");
  box.className = "state-box";
  box.innerHTML = `
    <div class="state-icon">${icon}</div>
    <div class="state-title">${escapeHtml(title)}</div>
    ${text ? `<div class="state-text">${escapeHtml(text)}</div>` : ""}
  `;
  if (actionLabel && onAction) {
    const b = document.createElement("button");
    b.className = "secondary-btn state-action";
    b.textContent = actionLabel;
    b.addEventListener("click", onAction);
    box.appendChild(b);
  }
  return box;
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

    // Карточки появляются волной, а не все разом.
    card.style.animationDelay = `${el.versionGrid.children.length * 70}ms`;

    const last = state.settings && state.settings.last_version === v.id;
    const badge = ready
      ? `<span class="vc-badge vc-badge-client">Клиент</span>${
          last ? '<span class="vc-badge vc-badge-last">Вы играли</span>' : ""
        }`
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
      // Подсветка едет за курсором. Считаем координаты относительно самой
      // карточки, чтобы пятно не «прилипало» к краю при быстрых движениях.
      card.addEventListener("pointermove", (ev) => {
        if (state.settings && state.settings.effects_enabled === false) return;
        const r = card.getBoundingClientRect();
        card.style.setProperty("--mx", `${ev.clientX - r.left}px`);
        card.style.setProperty("--my", `${ev.clientY - r.top}px`);
      });
    }
    el.versionGrid.appendChild(card);
  }

  if (versions.length === 0) {
    el.versionGrid.appendChild(
      stateBox({
        icon: "◎",
        title: "Пока нет ни одной сборки",
        text: "Похоже, идёт обновление. Загляните чуть позже.",
        actionLabel: "Обновить",
        onAction: loadManifest,
      })
    );
  }
}

function selectVersion(v) {
  state.selected = v;
  // Список модов кэшируется на сборку — при переходе в другую его надо
  // сбросить, иначе покажутся моды предыдущей.
  state.optionalMods = [];
  state.modsQuery = "";
  state.modsFilter = "all";
  state.serverWasOffline = false;
  if (el.modsSearch) el.modsSearch.value = "";
  if (el.modsBadge) el.modsBadge.classList.add("hidden");
  preloadOptionalMods(v);
  refreshPlaytime(v.id);
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

  // Новому игроку сразу ставим курсор в поле ника: это единственное, что от
  // него требуется, и промахнуться мимо мышкой уже нельзя.
  if (!NICK_RE.test(el.nickInput.value)) {
    setTimeout(() => el.nickInput.focus(), 120);
  }
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
  el.screenMods.classList.add("hidden");
  el.modDetails.classList.add("hidden");
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
  el.nickField.classList.remove("hidden");
  stopTips();
  setBackAvailable(true);
}

function showProgress() {
  el.actionRow.classList.add("hidden");
  el.progressArea.classList.remove("hidden");
  el.errorArea.classList.add("hidden");
  // Ник во время установки менять уже поздно — на его месте показываем
  // подсказки, чтобы ожидание не было пустым.
  el.nickField.classList.add("hidden");
  el.progressTip.classList.remove("hidden");
  startTips();
  setBackAvailable(false);
}

let lastErrorText = "";

function showError(message) {
  lastErrorText = String(message);
  el.actionRow.classList.add("hidden");
  el.progressArea.classList.add("hidden");
  el.errorArea.classList.remove("hidden");
  el.progressTip.classList.add("hidden");
  el.nickField.classList.remove("hidden");
  stopTips();

  // Игроку — понятное объяснение, что делать. Технические подробности
  // рядом, но свёрнуты: раньше на него вываливался стек с «dns error», и
  // единственной реакцией было «у меня всё сломалось».
  const human = humanError(message);
  el.errorText.textContent = human;
  el.errorDetail.textContent = lastErrorText;
  el.errorDetail.classList.add("hidden");
  const sameText = human === lastErrorText;
  el.btnErrorDetail.classList.toggle("hidden", sameText);
  el.btnErrorDetail.textContent = "Подробности";
  setBackAvailable(true);
}

document.getElementById("btn-error-back").addEventListener("click", showIdle);
document.getElementById("btn-error-detail").addEventListener("click", () => {
  const hidden = el.errorDetail.classList.toggle("hidden");
  el.btnErrorDetail.textContent = hidden ? "Подробности" : "Свернуть";
});
for (const id of ["btn-open-logs", "btn-logs-settings"]) {
  const b = document.getElementById(id);
  if (b) b.addEventListener("click", () => {
    invoke("open_logs_folder").catch((e) => flog("error", `open_logs_folder: ${e}`));
  });
}
document.getElementById("btn-copy-log").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText(lastErrorText);
    toast("Текст ошибки скопирован");
  } catch (e) {
    flog("error", `clipboard: ${e}`);
    toast("Не удалось скопировать", "err");
  }
});

// Скорость и остаток времени считаем на фронте по тем же событиям
// прогресса: бэкенд их и так шлёт с накопленным объёмом, отдельный канал
// ради этого не нужен.
const speed = { stage: null, startedAt: 0, startedAt0: 0, lastText: "" };

/// «12,4 МБ/с · осталось ~1 мин». Пустая строка, пока считать не по чему.
function speedSuffix(stage, current, total, unit) {
  if (unit !== "bytes" || total <= 1) return "";

  // Каждая стадия считается заново: скорость распаковки и скорость
  // скачивания — разные величины, усреднять их вместе бессмысленно.
  if (speed.stage !== stage || current < speed.startedAt0) {
    speed.stage = stage;
    speed.startedAt = performance.now();
    speed.startedAt0 = current;
    return "";
  }

  const seconds = (performance.now() - speed.startedAt) / 1000;
  const done = current - speed.startedAt0;
  // Первые мгновения дают дикие выбросы — ждём, пока наберётся статистика.
  if (seconds < 1.5 || done <= 0) return speed.lastText;

  const bps = done / seconds;
  const left = Math.max(0, total - current);
  const etaSec = bps > 0 ? left / bps : 0;
  speed.lastText = `${formatBytes(bps)}/с · осталось ${formatEta(etaSec)}`;
  return speed.lastText;
}

/// Округляем нарочно грубо: точность «осталось 3 мин 47 с» здесь ложная,
/// а дёрганый счётчик секунд только нервирует.
function formatEta(sec) {
  if (sec < 10) return "меньше 10 с";
  if (sec < 60) return `${Math.round(sec / 5) * 5} с`;
  const min = Math.round(sec / 60);
  if (min < 60) return `~${min} мин`;
  return `~${Math.round(min / 60)} ч`;
}

/// Порядок этапов установки — в нём же они идут в пайплайне запуска.
const STAGES = ["java", "install", "assets", "sync", "launch"];

/// Подсвечивает текущий этап и помечает пройденные.
function markStage(stage) {
  const idx = STAGES.indexOf(stage);
  if (idx < 0) return;
  for (const node of document.querySelectorAll(".pstep")) {
    const at = STAGES.indexOf(node.dataset.stage);
    node.classList.toggle("pstep-done", at < idx);
    node.classList.toggle("pstep-now", at === idx);
  }
}

listen("progress", (event) => {
  const { stage, label, current, total, unit } = event.payload;
  markStage(stage);

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
    el.progressMeta.textContent = speedSuffix(stage || label, current, total, unit);
  } else {
    el.progressFill.style.width = "100%";
    el.progressPercent.textContent = "";
    el.progressLabel.textContent = label;
    el.progressMeta.textContent = "";
    speed.stage = null;
    speed.lastText = "";
  }
});

// ---------- Самообновление лаунчера ----------

listen("update-started", (event) => {
  const version = event.payload || "";
  flog("info", `самообновление до ${version}`);
  el.screenUpdate.classList.remove("hidden");
  el.updateTitle.textContent = "Обновление лаунчера";
  el.updateSub.textContent = version ? `Загружаем версию ${version}...` : "Скачиваем новую версию...";
});

listen("update-progress", (event) => {
  const [done, total] = event.payload || [0, 0];
  if (total > 0) {
    const pct = Math.min(100, Math.round((done / total) * 100));
    el.updateFill.style.width = `${pct}%`;
    el.updatePercent.textContent = `${pct}%`;
    el.updateSize.textContent = `${formatBytes(done)} / ${formatBytes(total)}`;
  } else {
    // Сервер не отдал Content-Length — показываем хотя бы объём.
    el.updateSize.textContent = formatBytes(done);
  }
});

listen("update-ready", () => {
  el.updateFill.style.width = "100%";
  el.updatePercent.textContent = "100%";
  el.updateTitle.textContent = "Готово";
  el.updateSub.textContent = "Перезапускаем лаунчер...";
});

listen("update-failed", (event) => {
  // Не блокируем игру: обновление не критично, можно играть на текущей версии.
  flog("warn", `самообновление не удалось: ${event.payload}`);
  el.screenUpdate.classList.add("hidden");
});

listen("game-exited", (event) => {
  const code = event.payload;
  flog("info", `game-exited code=${code}`);
  // Сессия уже записана бэкендом — перечитываем, чтобы счётчик на экране
  // сразу показал новое время, а не старое до перезапуска лаунчера.
  if (state.selected) refreshPlaytime(state.selected.id);
  // Игра закрылась — возвращаем окно (на время игры оно было полностью
  // спрятано, а не свёрнуто, чтобы не занимать место в панели задач).
  appWindow.show();
  appWindow.unminimize();
  appWindow.setFocus();
  if (state.selected) startServerPolling();
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
  el.progressMeta.textContent = "";
  // Начинаем с чистого списка этапов: предыдущая попытка могла оборваться
  // на середине и оставить половину отмеченной.
  for (const node of document.querySelectorAll(".pstep")) {
    node.classList.remove("pstep-done", "pstep-now");
  }

  try {
    await invoke("launch", { versionId: state.selected.id, nick: el.nickInput.value });
    // Запомнили, во что играли последний раз — на главном экране эта
    // карточка будет помечена.
    if (state.settings.last_version !== state.selected.id) {
      state.settings.last_version = state.selected.id;
      invoke("save_settings", { settings: state.settings }).catch(() => {});
    }
    // Пока идёт игра, окно спрятано — опрашивать сервер каждые 30 секунд
    // некому и незачем.
    stopServerPolling();
    // Пайплайн установки завершился и игра реально запущена (spawn прошёл).
    // Прячем окно целиком (hide, а не minimize): свёрнутый лаунчер занимал
    // место в панели задач и мешал. Процесс при этом жив и ждёт выхода из
    // игры — событие game-exited вернёт окно обратно.
    await appWindow.hide();
  } catch (e) {
    flog("error", `launch failed: ${e}`);
    showError(String(e));
  }
});

// ---------- Экран опциональных модов ----------

document.getElementById("btn-settings").addEventListener("click", openOptionalScreen);
el.btnCode.addEventListener("click", openCodeModal);
document.getElementById("code-close").addEventListener("click", closeCodeModal);
el.codeSubmit.addEventListener("click", submitCode);
el.codeInput.addEventListener("keydown", (ev) => { if (ev.key === "Enter") submitCode(); });
el.codeModal.addEventListener("click", (ev) => { if (ev.target === el.codeModal) closeCodeModal(); });
document.getElementById("btn-mods").addEventListener("click", openModsScreen);
document.getElementById("btn-back-mods").addEventListener("click", () => {
  el.screenMods.classList.add("hidden");
});

el.modsSearch.addEventListener("input", () => {
  state.modsQuery = el.modsSearch.value;
  renderOptionalMods(state.optionalMods);
});

el.modsSort.addEventListener("change", () => {
  state.modsSort = el.modsSort.value;
  renderOptionalMods(state.optionalMods);
});

// Снять всё сразу. Тридцать переключателей по одному — это не выбор,
// а работа; кнопка появляется, только когда снимать есть что.
el.modsClear.addEventListener("click", () => {
  const versionId = state.selected && state.selected.id;
  if (!versionId) return;
  const chosen = (state.optionalMods || []).filter((m) => isModEnabled(m, versionId));
  if (chosen.length === 0) return;
  for (const m of chosen) {
    if (!state.settings.optional_mods[versionId]) state.settings.optional_mods[versionId] = {};
    state.settings.optional_mods[versionId][m.id] = false;
  }
  invoke("save_settings", { settings: state.settings }).catch((e) => flog("error", `save_settings: ${e}`));
  renderOptionalMods(state.optionalMods);
  toast(`Снято ${chosen.filter((m) => !m.hidden).length}`);
});

for (const btn of document.querySelectorAll(".mods-filter")) {
  btn.addEventListener("click", () => {
    state.modsFilter = btn.dataset.filter;
    for (const b of document.querySelectorAll(".mods-filter")) {
      b.classList.toggle("mods-filter-on", b === btn);
    }
    renderOptionalMods(state.optionalMods);
  });
}
document.getElementById("btn-back-optional").addEventListener("click", () => {
  el.screenOptional.classList.add("hidden");
});

document.getElementById("mod-details-close").addEventListener("click", closeModDetails);
// Клик по затемнению вокруг карточки тоже закрывает — привычное поведение.
el.modDetails.addEventListener("click", (ev) => {
  if (ev.target === el.modDetails) closeModDetails();
});
// Ctrl+M — моды, Ctrl+, — настройки: те же сочетания, что в большинстве
// приложений, и не надо тянуться мышью через весь экран.
document.addEventListener("keydown", (ev) => {
  if (!ev.ctrlKey || ev.altKey || !state.selected) return;
  if (ev.key === "m" || ev.key === "ь") {
    ev.preventDefault();
    openModsScreen();
  } else if (ev.key === "," || ev.key === "б") {
    ev.preventDefault();
    openOptionalScreen();
  }
});

// Escape закрывает то, что открыто последним. Раньше он работал только в
// окне описания мода: из настроек и списка модов выйти с клавиатуры было
// нельзя, и это читалось как «окно зависло».
document.addEventListener("keydown", (ev) => {
  if (ev.key !== "Escape") return;
  const layers = [
    [el.modDetails, closeModDetails],
    [el.codeModal, closeCodeModal],
    [el.screenMods, () => el.screenMods.classList.add("hidden")],
    [el.screenOptional, () => el.screenOptional.classList.add("hidden")],
  ];
  for (const [node, close] of layers) {
    if (node && !node.classList.contains("hidden")) {
      close();
      return;
    }
  }
});

function openOptionalScreen() {
  if (!state.selected) return;
  el.screenOptional.classList.remove("hidden");
  refreshTotals();
}

/// Переключатели звука и эффектов. Значение сохраняем сразу: настройка,
/// которая не пережила перезапуск, воспринимается как сломанная.
function initPreferences() {
  const bind = (input, key, label) => {
    if (!input) return;
    input.checked = state.settings[key] !== false;
    input.addEventListener("change", () => {
      state.settings[key] = input.checked;
      invoke("save_settings", { settings: state.settings }).catch((e) =>
        flog("error", `save_settings: ${e}`)
      );
      toast(`${label}: ${input.checked ? "включено" : "выключено"}`);
    });
  };
  bind(el.optSound, "sound_enabled", "Звук");
  bind(el.optEffects, "effects_enabled", "Эффекты");

  // Класс на body гасит всё, что движется само по себе; CSS решает, что
  // именно останавливать (см. body.fx-off в стилях).
  const applyFx = () => document.body.classList.toggle("fx-off", state.settings.effects_enabled === false);
  applyFx();
  if (el.optEffects) el.optEffects.addEventListener("change", applyFx);
}

/// Итоги для экрана настроек: сколько наиграно всего и какая версия
/// лаунчера установлена.
async function refreshTotals() {
  if (el.totalPlaytime && state.selected) {
    try {
      const pt = await invoke("get_playtime", { versionId: state.selected.id });
      el.totalPlaytime.textContent = pt && pt.total_seconds
        ? formatPlaytime(pt.total_seconds)
        : "ещё ни разу";
    } catch (_) {
      el.totalPlaytime.textContent = "—";
    }
  }
  if (el.launcherVersion && el.launcherVersion.textContent === "—") {
    try {
      el.launcherVersion.textContent = await invoke("launcher_version");
    } catch (_) {}
  }
  if (el.installSize && state.selected) {
    el.installSize.textContent = "считаем…";
    try {
      const bytes = await invoke("get_install_size", { versionId: state.selected.id });
      el.installSize.textContent = bytes ? formatBytes(bytes) : "ещё не установлена";
    } catch (_) {
      el.installSize.textContent = "—";
    }
  }
}

/// Ручная проверка обновления. Лаунчер проверяет и сам при запуске, но
/// когда что-то не работает, первое, что хочется сделать, — убедиться,
/// что версия свежая, и не гадать.
function initUpdateCheck() {
  if (!el.btnCheckUpdate) return;
  el.btnCheckUpdate.addEventListener("click", async () => {
    el.btnCheckUpdate.disabled = true;
    const label = el.btnCheckUpdate.textContent;
    el.btnCheckUpdate.textContent = "Проверяем…";
    try {
      const found = await invoke("check_for_update");
      // Если обновление есть, лаунчер уже показывает свой экран загрузки
      // и вот-вот перезапустится — говорить что-то ещё незачем.
      if (!found) toast("У вас последняя версия");
    } catch (e) {
      flog("warn", `check_for_update: ${e}`);
      toast(humanError(e), "err");
    } finally {
      el.btnCheckUpdate.disabled = false;
      el.btnCheckUpdate.textContent = label;
    }
  });
}

/// Экран модов. Список грузится один раз на сборку и дальше живёт в state,
/// чтобы повторный вход открывался мгновенно.
async function openModsScreen() {
  if (!state.selected) return;

  el.screenMods.classList.remove("hidden");
  if (state.optionalMods.length === 0) {
    // Скелет вместо строчки текста: место под карточки видно сразу, и
    // список не «прыгает», когда данные приходят.
    el.optionalList.innerHTML = Array.from({ length: 6 })
      .map(
        (_, i) =>
          `<div class="mod-card mod-card-skeleton" style="animation-delay:${i * 60}ms">
             <div class="sk sk-icon"></div>
             <div class="mod-card-body"><div class="sk sk-line sk-line-title"></div>
               <div class="sk sk-line"></div><div class="sk sk-line sk-line-short"></div></div>
           </div>`
      )
      .join("");
    try {
      const mods = await invoke("get_optional_mods", { packwizUrl: state.selected.pack.packwiz_url });
      state.optionalMods = mods;
    } catch (e) {
      flog("error", `get_optional_mods: ${e}`);
      el.optionalList.innerHTML = "";
      el.optionalList.appendChild(
        stateBox({
          icon: "⚠",
          title: "Не удалось загрузить список модов",
          text: humanError(e),
          actionLabel: "Повторить",
          onAction: () => {
            state.optionalMods = [];
            openModsScreen();
          },
        })
      );
      return;
    }
  }
  renderOptionalMods(state.optionalMods);
}

/// Тихо подтягивает список модов при входе в сборку — только чтобы показать
/// значок с числом выбранных на кнопке. Ошибки игнорируем: если не вышло,
/// список всё равно загрузится при открытии экрана.
async function preloadOptionalMods(version) {
  if (!version.pack || !version.pack.packwiz_url) return;
  try {
    const mods = await invoke("get_optional_mods", { packwizUrl: version.pack.packwiz_url });
    if (state.selected && state.selected.id === version.id) {
      state.optionalMods = mods;
      updateModsSummary();
    }
  } catch (_) {}
}

/// Сколько модов выбрано и сколько они весят — показываем и в шапке экрана,
/// и значком на кнопке «Моды», чтобы выбор был виден не заходя внутрь.
function updateModsSummary() {
  const versionId = state.selected && state.selected.id;
  if (!versionId) return;
  const all = state.optionalMods || [];
  const chosen = all.filter((m) => !m.hidden && isModEnabled(m, versionId));

  // Размер считаем со скрытыми библиотеками: скачается-то и они.
  const withDeps = all.filter((m) => isModEnabled(m, versionId));
  const bytes = withDeps.reduce((sum, m) => sum + (m.size_bytes || 0), 0);

  if (el.modsBadge) {
    el.modsBadge.textContent = String(chosen.length);
    el.modsBadge.classList.toggle("hidden", chosen.length === 0);
  }
  if (el.modsSummary) {
    const visible = all.filter((m) => !m.hidden).length;
    const base = chosen.length
      ? `Выбрано ${chosen.length} из ${visible} · ${(bytes / 1024 / 1024).toFixed(1)} МБ`
      : `Ничего не выбрано · доступно ${visible}`;
    // При активном поиске важнее знать, сколько нашлось прямо сейчас.
    const q = (state.modsQuery || "").trim();
    el.modsSummary.textContent = q
      ? `${base} · найдено ${countMatches(all, q)}`
      : base;
  }
  if (el.modsClear) el.modsClear.classList.toggle("hidden", chosen.length === 0);
}

function renderOptionalMods(mods) {
  el.optionalList.innerHTML = "";
  state.optionalMods = mods;
  const versionIdForFilter = state.selected.id;

  // Библиотеки-зависимости в списке не показываем: игроку они сами по себе
  // не нужны, лаунчер включит их вместе с модом, которому они требуются.
  let visible = mods.filter((m) => !m.hidden);

  const q = (state.modsQuery || "").trim().toLowerCase();
  if (q) {
    visible = visible.filter((m) =>
      `${m.name} ${m.description} ${m.description_full}`.toLowerCase().includes(q)
    );
  }
  if (state.modsFilter === "on") {
    visible = visible.filter((m) => isModEnabled(m, versionIdForFilter));
  }

  const bySize = (a, b) => (a.size_bytes || 0) - (b.size_bytes || 0);
  const byName = (a, b) => (a.name || "").localeCompare(b.name || "", "ru");
  if (state.modsSort === "size") visible.sort(bySize);
  else if (state.modsSort === "size-desc") visible.sort((a, b) => bySize(b, a));
  else visible.sort(byName);

  updateModsSummary();

  if (mods.filter((m) => !m.hidden).length === 0) {
    el.optionalList.innerHTML = '<div class="mods-empty">У этой сборки нет дополнительных модов.</div>';
    return;
  }
  if (visible.length === 0) {
    el.optionalList.innerHTML = "";
    el.optionalList.appendChild(
      stateBox({
        icon: "⌕",
        title: "Ничего не найдено",
        text: state.modsFilter === "on" && !q
          ? "Ни один дополнительный мод пока не включён."
          : "Попробуйте другой запрос или снимите фильтр.",
        actionLabel: q || state.modsFilter !== "all" ? "Сбросить поиск" : null,
        onAction: () => {
          state.modsQuery = "";
          state.modsFilter = "all";
          el.modsSearch.value = "";
          for (const b of document.querySelectorAll(".mods-filter")) {
            b.classList.toggle("mods-filter-on", b.dataset.filter === "all");
          }
          renderOptionalMods(state.optionalMods);
        },
      })
    );
    return;
  }

  const versionId = state.selected.id;
  if (!state.settings.optional_mods) state.settings.optional_mods = {};

  for (const mod of visible) {
    const card = document.createElement("div");
    card.className = "mod-card";
    card.dataset.id = mod.id;

    const letter = escapeHtml((mod.name || "?").slice(0, 1));
    const icon = mod.icon_url
      ? `<img class="mod-card-icon" src="${escapeHtml(mod.icon_url)}" alt="" loading="lazy" />`
      : `<div class="mod-card-icon mod-card-icon-empty">${letter}</div>`;

    const size = mod.size_bytes ? `${(mod.size_bytes / 1024 / 1024).toFixed(1)} МБ` : "";
    // Подпись про зависимости — игрок должен понимать, что включится ещё один мод.
    const reqNames = (mod.requires || [])
      .map((id) => (mods.find((x) => x.id === id) || {}).name)
      .filter(Boolean);
    const reqNote = reqNames.length
      ? `<div class="mod-card-req">+ ${escapeHtml(reqNames.join(", "))}</div>`
      : "";

    card.innerHTML = `
      ${icon}
      <div class="mod-card-body">
        <div class="mod-card-name">${escapeHtml(mod.name)}</div>
        <div class="mod-card-desc">${escapeHtml(mod.description || "")}</div>
        ${reqNote}
      </div>
      <div class="mod-card-foot">
        <span class="mod-card-size">${size}</span>
        <label class="switch" title="Включить мод">
          <input type="checkbox" />
          <span class="switch-slider"></span>
        </label>
      </div>
    `;

    // Иконка тянется с раздачи, и при слабой связи вместо неё оставался бы
    // значок битой картинки. Подменяем на букву — как у модов без иконки.
    const img = card.querySelector("img.mod-card-icon");
    if (img) {
      img.addEventListener(
        "error",
        () => {
          const ph = document.createElement("div");
          ph.className = "mod-card-icon mod-card-icon-empty";
          ph.textContent = (mod.name || "?").slice(0, 1);
          img.replaceWith(ph);
        },
        { once: true }
      );
    }

    const box = card.querySelector("input");
    box.checked = isModEnabled(mod, versionId);
    syncCardState(card, box.checked);

    // Клик по карточке открывает описание, но переключатель должен
    // переключать, а не открывать окно.
    card.addEventListener("click", (ev) => {
      if (ev.target.closest("label.switch")) return;
      openModDetails(mod);
    });
    box.addEventListener("change", () => {
      setModEnabled(mod, box.checked);
      syncAllCards();
      updateModsSummary();
      // В режиме «Включённые» выключенный мод должен сразу исчезнуть из списка.
      // Каскад мог погасить и соседей, поэтому перерисовываем список целиком.
      if (state.modsFilter === "on" && !box.checked) renderOptionalMods(state.optionalMods);
    });

    el.optionalList.appendChild(card);
  }
}

/// Сколько модов подходит под запрос — по тем же полям, что и сам поиск.
function countMatches(all, query) {
  const q = query.toLowerCase();
  return all.filter(
    (m) => !m.hidden && `${m.name} ${m.description} ${m.description_full}`.toLowerCase().includes(q)
  ).length;
}

/// Включён ли мод: сохранённый выбор игрока, иначе значение по умолчанию.
function isModEnabled(mod, versionId) {
  const saved = (state.settings.optional_mods || {})[versionId] || {};
  return Object.prototype.hasOwnProperty.call(saved, mod.id) ? saved[mod.id] : mod.default_value;
}

function syncCardState(card, on) {
  card.classList.toggle("mod-card-on", on);
}

/// Приводит все карточки к тому, что реально записано в настройках.
///
/// Нужно из-за каскада: снятие Sodium выключает и Iris, но его карточка об
/// этом не знала и продолжала показывать «включено». Следующий клик по ней
/// уходил в обратную сторону — игрок жал «включить», а мод выключался.
function syncAllCards() {
  const versionId = state.selected && state.selected.id;
  if (!versionId) return;
  for (const card of el.optionalList.querySelectorAll(".mod-card[data-id]")) {
    const mod = (state.optionalMods || []).find((m) => m.id === card.dataset.id);
    if (!mod) continue;
    const box = card.querySelector("input");
    const on = isModEnabled(mod, versionId);
    if (box) box.checked = on;
    syncCardState(card, on);
  }
}

/// Сохраняет выбор и тянет за собой зависимости.
///
/// Включаем мод — включаются все, кого он требует. Выключаем — выключаем и
/// его библиотеки, но только если они больше никому из включённых не нужны:
/// иначе, сняв Better Clouds, можно было бы случайно сломать другой мод,
/// которому та же библиотека ещё нужна.
function setModEnabled(mod, on) {
  const versionId = state.selected.id;
  const all = state.optionalMods || [];
  if (!state.settings.optional_mods) state.settings.optional_mods = {};
  if (!state.settings.optional_mods[versionId]) state.settings.optional_mods[versionId] = {};
  const sel = state.settings.optional_mods[versionId];

  sel[mod.id] = on;

  const byId = (id) => all.find((x) => x.id === id);

  if (on) {
    // Включили — подтягиваем всё, что мод требует, и зависимости зависимостей.
    const queue = [...(mod.requires || [])];
    while (queue.length) {
      const dep = byId(queue.shift());
      if (!dep || sel[dep.id] === true) continue;
      sel[dep.id] = true;
      queue.push(...(dep.requires || []));
    }
  } else {
    // Выключили — вместе с модом уходят те, кому он нужен. Раньше это было
    // не нужно: зависимости всегда были скрытыми библиотеками и снять их
    // руками было нельзя. Sodium показывается отдельно, и без него Iris
    // просто не загрузится.
    const parents = [...(mod.needed_by || [])];
    while (parents.length) {
      const parent = byId(parents.shift());
      if (!parent || !isModEnabled(parent, versionId)) continue;
      sel[parent.id] = false;
      parents.push(...(parent.needed_by || []));
    }

    // ...и скрытые библиотеки, которые больше никому из включённых не нужны.
    // Видимые моды не трогаем: их игрок выбирал сам, и снятие Iris не повод
    // отбирать у него Sodium.
    const deps = [...(mod.requires || [])];
    while (deps.length) {
      const dep = byId(deps.shift());
      if (!dep || !dep.hidden || !isModEnabled(dep, versionId)) continue;
      const stillNeeded = (dep.needed_by || []).some((otherId) => {
        const other = byId(otherId);
        return other && isModEnabled(other, versionId);
      });
      if (!stillNeeded) {
        sel[dep.id] = false;
        deps.push(...(dep.requires || []));
      }
    }
  }

  invoke("save_settings", { settings: state.settings }).catch((e) => flog("error", `save_settings: ${e}`));
}

// ---------- Окно с описанием мода ----------

function openModDetails(mod) {
  const versionId = state.selected.id;
  el.modDetailsName.textContent = mod.name;

  const nameOf = (id) => (state.optionalMods.find((x) => x.id === id) || {}).name;
  const reqNames = (mod.requires || []).map(nameOf).filter(Boolean);
  // Кому нужен сам этот мод. Раньше зависимости были только скрытыми
  // библиотеками и вопрос не стоял; теперь Sodium виден отдельной карточкой,
  // и, снимая его, игрок должен знать, что вместе с ним уйдёт Iris.
  const neededByNames = (mod.needed_by || [])
    .map((id) => state.optionalMods.find((x) => x.id === id))
    .filter((m) => m && isModEnabled(m, versionId))
    .map((m) => m.name);

  const parts = [];
  if (mod.size_bytes) parts.push(`${(mod.size_bytes / 1024 / 1024).toFixed(1)} МБ`);
  if (reqNames.length) parts.push(`Требует: ${reqNames.join(", ")}`);
  if (neededByNames.length) parts.push(`Нужен для: ${neededByNames.join(", ")}`);
  el.modDetailsSub.textContent = parts.join(" · ");

  if (mod.icon_url) {
    el.modDetailsIcon.src = mod.icon_url;
    el.modDetailsIcon.classList.remove("hidden");
    // Не догрузилась — лучше без иконки, чем со значком битой картинки.
    el.modDetailsIcon.onerror = () => el.modDetailsIcon.classList.add("hidden");
  } else {
    el.modDetailsIcon.classList.add("hidden");
  }

  // Показываем и русскую строку из пака, и полное описание от автора мода.
  const full = [mod.description, mod.description_full].filter(Boolean).join("\n\n");
  el.modDetailsBody.textContent = full || "Описание не указано.";

  const box = el.modDetailsSwitch;
  box.checked = isModEnabled(mod, versionId);
  el.modDetailsSwitchLabel.textContent = box.checked ? "Включён" : "Включить";
  box.onchange = () => {
    setModEnabled(mod, box.checked);
    el.modDetailsSwitchLabel.textContent = box.checked ? "Включён" : "Включить";
    // Список под окном должен показать то же состояние — и у этого мода,
    // и у всех, кого задел каскад зависимостей.
    syncAllCards();
    updateModsSummary();
  };

  el.modDetails.classList.remove("hidden");
}

function closeModDetails() {
  el.modDetails.classList.add("hidden");
}


// ---------- Закрытые сборки ----------

/// Проверяет код и, если он подошёл, дописывает закрытые сборки к списку.
///
/// Публичный манифест о них не знает вообще — они лежат по адресу, который
/// считается из кода (см. private_access.rs). Поэтому «открыть» их можно
/// только зная код: подобрать адрес нельзя.
async function applyPrivateCode(code, { silent = false } = {}) {
  if (!code || !code.trim()) return false;
  try {
    const versions = await invoke("unlock_private", { code });
    if (!versions || versions.length === 0) {
      if (!silent) setCodeStatus("Код не подошёл", true);
      return false;
    }

    // Дописываем к уже показанным, не задваивая.
    const known = new Set((state.manifest.versions || []).map((v) => v.id));
    const added = versions.filter((v) => !known.has(v.id));
    state.manifest.versions = [...(state.manifest.versions || []), ...added];
    renderVersionGrid(state.manifest.versions);

    // Код запоминаем: иначе при каждом запуске пришлось бы вводить заново,
    // а он нужен ещё и при запуске самой сборки (её нет в публичном манифесте).
    if (state.settings.private_code !== code) {
      state.settings.private_code = code;
      invoke("save_settings", { settings: state.settings }).catch(() => {});
    }
    return true;
  } catch (e) {
    // Сетевая ошибка — это не «неверный код», и говорить надо разное.
    flog("warn", `unlock_private: ${e}`);
    if (!silent) setCodeStatus("Нет связи с сервером, попробуйте позже", true);
    return false;
  }
}

function setCodeStatus(text, isError) {
  el.codeStatus.textContent = text;
  el.codeStatus.classList.toggle("code-status-error", !!isError);
}

function openCodeModal() {
  el.codeInput.value = "";
  setCodeStatus("", false);
  el.codeModal.classList.remove("hidden");
  setTimeout(() => el.codeInput.focus(), 50);
}

function closeCodeModal() {
  el.codeModal.classList.add("hidden");
}

async function submitCode() {
  const code = el.codeInput.value.trim();
  if (!code) return;
  setCodeStatus("Проверяем…", false);
  el.codeSubmit.disabled = true;
  const ok = await applyPrivateCode(code);
  el.codeSubmit.disabled = false;
  if (ok) {
    setCodeStatus("Готово", false);
    setTimeout(closeCodeModal, 500);
  }
}

/// Загружает манифест и рисует главный экран. Вынесено отдельно, чтобы
/// кнопка «Повторить» перезапускала ровно это, а не весь лаунчер: у игрока
/// интернет мог просто моргнуть, и заставлять его перезапускаться — грубо.
async function loadManifest() {
  el.versionGrid.innerHTML = "";
  el.versionGrid.appendChild(
    stateBox({ icon: "◍", title: "Загружаем список сборок…", text: "" })
  );

  try {
    state.manifest = await invoke("get_manifest");
  } catch (e) {
    flog("error", `get_manifest: ${e}`);
    el.versionGrid.innerHTML = "";
    el.versionGrid.appendChild(
      stateBox({
        icon: "⚠",
        title: "Не удалось загрузить список сборок",
        text: humanError(e),
        actionLabel: "Повторить",
        onAction: loadManifest,
      })
    );
    return;
  }

  renderVersionGrid(state.manifest.versions || []);
  renderNews(state.manifest.news_feed || []);

  // Код уже вводили — открываем закрытые сборки молча, без окна.
  if (state.settings.private_code) {
    applyPrivateCode(state.settings.private_code, { silent: true });
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

  // Железо нужно до инициализации ползунка: от него зависят и значение по
  // умолчанию, и верхняя граница.
  try {
    state.system = await invoke("get_system_info");
  } catch (e) {
    flog("warn", `get_system_info: ${e}`);
    state.system = null;
  }

  initRamControl();
  initPreferences();
  initUpdateCheck();
  initClickSound();
  initReinstall();

  await loadManifest();

  flog("info", "frontend initialised");

  // Проверка обновлений лаунчера — не блокирует показ окна (оно уже
  // нарисовано к этому моменту), делается тихо в фоне.
  invoke("check_for_update").catch((e) => flog("warn", `check_for_update: ${e}`));
}

init();
