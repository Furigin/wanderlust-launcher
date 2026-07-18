// Чистый JS без сборщика — Tauri v2 при withGlobalTauri:true кладёт API
// в window.__TAURI__. Экраны "Прогресс" и "Ошибка" — это не отдельные
// страницы, а состояния action-area на главном экране (кнопка ИГРАТЬ
// подменяется полосой прогресса на том же месте, см. ТЗ Этапа 4).

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
  selectedPackId: null,
};

const el = {
  nickInput: document.getElementById("nick-input"),
  packCards: document.getElementById("pack-cards"),
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
};

// ---------- Титульная строка ----------

document.getElementById("btn-minimize").addEventListener("click", () => appWindow.minimize());
document.getElementById("btn-close").addEventListener("click", () => appWindow.close());

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
  const nickOk = NICK_RE.test(el.nickInput.value);
  el.playBtn.disabled = !(nickOk && state.selectedPackId);
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

// ---------- Карточки паков ----------

function renderPackCards(packs) {
  el.packCards.innerHTML = "";
  for (const pack of packs) {
    const card = document.createElement("button");
    card.className = "pack-card";
    card.type = "button";
    card.innerHTML = `
      <div class="pack-card-title">${escapeHtml(pack.title)}</div>
      <div class="pack-card-subtitle">${escapeHtml(pack.subtitle || "")}</div>
    `;
    card.addEventListener("click", () => selectPack(pack.id));
    card.dataset.packId = pack.id;
    el.packCards.appendChild(card);
  }
}

function selectPack(packId) {
  state.selectedPackId = packId;
  for (const card of el.packCards.children) {
    card.classList.toggle("selected", card.dataset.packId === packId);
  }
  updatePlayAvailability();
}

function escapeHtml(s) {
  const div = document.createElement("div");
  div.textContent = s;
  return div.innerHTML;
}

// ---------- Прогресс / ошибка (состояния action-area) ----------

function showIdle() {
  el.playBtn.classList.remove("hidden");
  el.progressArea.classList.add("hidden");
  el.errorArea.classList.add("hidden");
}

function showProgress() {
  el.playBtn.classList.add("hidden");
  el.progressArea.classList.remove("hidden");
  el.errorArea.classList.add("hidden");
}

let lastErrorText = "";

function showError(message) {
  lastErrorText = message;
  el.playBtn.classList.add("hidden");
  el.progressArea.classList.add("hidden");
  el.errorArea.classList.remove("hidden");
  el.errorText.textContent = message;
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
  if (el.playBtn.disabled) return;
  showProgress();
  el.progressFill.style.width = "0%";
  el.progressLabel.textContent = "Подготовка...";

  try {
    await invoke("launch", { nick: el.nickInput.value, packId: state.selectedPackId });
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
  if (!state.selectedPackId || !state.manifest) return;
  const pack = state.manifest.packs.find((p) => p.id === state.selectedPackId);
  if (!pack) return;

  el.screenOptional.classList.remove("hidden");
  el.optionalList.innerHTML = '<div class="optional-empty">Загрузка списка модов...</div>';

  try {
    const mods = await invoke("get_optional_mods", { packwizUrl: pack.packwiz_url });
    renderOptionalMods(pack.id, mods);
  } catch (e) {
    flog("error", `get_optional_mods: ${e}`);
    el.optionalList.innerHTML = `<div class="optional-empty">Не удалось загрузить список: ${escapeHtml(String(e))}</div>`;
  }
}

function renderOptionalMods(packId, mods) {
  el.optionalList.innerHTML = "";
  if (mods.length === 0) {
    el.optionalList.innerHTML = '<div class="optional-empty">У этого пака нет опциональных модов.</div>';
    return;
  }

  const saved = (state.settings.optional_mods && state.settings.optional_mods[packId]) || {};

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
      if (!state.settings.optional_mods[packId]) state.settings.optional_mods[packId] = {};
      state.settings.optional_mods[packId][mod.id] = checkbox.checked;
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
    state.settings = { nickname: "", selected_pack: "", ram_mb: 2048, optional_mods: {} };
  }

  el.nickInput.value = state.settings.nickname || "";
  validateNick();

  try {
    state.manifest = await invoke("get_manifest");
  } catch (e) {
    flog("error", `get_manifest: ${e}`);
    showError(`Не удалось загрузить манифест: ${e}`);
    return;
  }

  el.newsText.textContent = state.manifest.news || "";
  renderFooterLinks(state.manifest.links || {});
  renderPackCards(state.manifest.packs || []);

  if (state.settings.selected_pack && state.manifest.packs.some((p) => p.id === state.settings.selected_pack)) {
    selectPack(state.settings.selected_pack);
  } else if (state.manifest.packs.length > 0) {
    selectPack(state.manifest.packs[0].id);
  }

  flog("info", "frontend initialised");

  // Проверка обновлений лаунчера — не блокирует показ окна (оно уже
  // нарисовано к этому моменту), делается тихо в фоне.
  invoke("check_for_update").catch((e) => flog("warn", `check_for_update: ${e}`));
}

init();
