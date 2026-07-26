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
};

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

  el.homeScreen.classList.add("hidden");
  el.playScreen.classList.remove("hidden");

  el.newsText.textContent = v.news || "";
  renderFooterLinks(state.manifest.links || {});
  showIdle();
  validateNick();
}

function goHome() {
  state.selected = null;
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
  setBackAvailable(true);
}

function showProgress() {
  el.actionRow.classList.add("hidden");
  el.progressArea.classList.remove("hidden");
  el.errorArea.classList.add("hidden");
  setBackAvailable(false);
}

let lastErrorText = "";

function showError(message) {
  lastErrorText = message;
  el.actionRow.classList.add("hidden");
  el.progressArea.classList.add("hidden");
  el.errorArea.classList.remove("hidden");
  el.errorText.textContent = message;
  setBackAvailable(true);
}

document.getElementById("btn-error-back").addEventListener("click", showIdle);
document.getElementById("btn-copy-log").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText(lastErrorText);
  } catch (e) {
    flog("error", `clipboard: ${e}`);
  }
});

listen("progress", (event) => {
  const { label, current, total } = event.payload;
  const pct = total > 0 ? Math.min(100, Math.round((current / total) * 100)) : 0;
  el.progressFill.style.width = `${pct}%`;
  el.progressLabel.textContent = total > 1 ? `${label} (${current}/${total})` : label;
});

listen("game-exited", (event) => {
  const code = event.payload;
  flog("info", `game-exited code=${code}`);
  appWindow.unminimize();
  appWindow.setFocus();
  if (code !== 0) {
    showError(`Игра завершилась с кодом ${code}. Подробности — в launcher.log.`);
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

  try {
    state.manifest = await invoke("get_manifest");
  } catch (e) {
    flog("error", `get_manifest: ${e}`);
    el.versionGrid.innerHTML = `<div class="optional-empty">Не удалось загрузить манифест: ${escapeHtml(String(e))}</div>`;
    return;
  }

  renderVersionGrid(state.manifest.versions || []);

  flog("info", "frontend initialised");

  // Проверка обновлений лаунчера — не блокирует показ окна (оно уже
  // нарисовано к этому моменту), делается тихо в фоне.
  invoke("check_for_update").catch((e) => flog("warn", `check_for_update: ${e}`));
}

init();
