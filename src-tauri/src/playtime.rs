// Учёт наигранного времени.
//
// Зачем свой счётчик, если Minecraft ведёт статистику сам: серверный
// `world/stats/<uuid>.json` считает только время на конкретном сервере и
// только для тех, кто на него заходил. Лаунчер же видит всю картину — когда
// игру запускали, сколько она была открыта, по каким сборкам это разошлось.
// Эти данные потом уезжают на сайт (см. `sessions`), поэтому пишем их
// в виде списка сессий, а не одного счётчика: из сессий можно построить
// что угодно (график по дням, активность по часам), а из числа — ничего.
//
// Файл живёт рядом с настройками. Порча файла не должна ломать запуск игры,
// поэтому все ошибки чтения дают пустую статистику, а не отказ.

use crate::paths::AppPaths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Одна сессия: игра запустилась и закрылась.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Session {
    /// Какая сборка запускалась (id из манифеста).
    pub version_id: String,
    /// Ник на момент запуска — он может меняться между сессиями.
    #[serde(default)]
    pub nickname: String,
    /// Начало сессии, unix-время в секундах (UTC).
    pub started_at: u64,
    /// Длительность в секундах.
    pub seconds: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Playtime {
    /// Все сессии, старые сверху. Чистим редко — за год активной игры это
    /// пара тысяч записей, то есть сотни килобайт.
    #[serde(default)]
    pub sessions: Vec<Session>,
    /// Что из этого уже ушло на сервер статистики: индекс первой неотправленной
    /// сессии. Так повторная отправка не задваивает время.
    #[serde(default)]
    pub synced_count: usize,
}

impl Playtime {
    /// Суммарно наиграно секунд по всем сборкам.
    pub fn total_seconds(&self) -> u64 {
        self.sessions.iter().map(|s| s.seconds).sum()
    }

    /// Наиграно секунд в конкретной сборке.
    pub fn seconds_for(&self, version_id: &str) -> u64 {
        self.sessions
            .iter()
            .filter(|s| s.version_id == version_id)
            .map(|s| s.seconds)
            .sum()
    }

    /// Сессии, ещё не отправленные на сервер статистики.
    pub fn unsynced(&self) -> &[Session] {
        let from = self.synced_count.min(self.sessions.len());
        &self.sessions[from..]
    }
}

/// Текущее unix-время в секундах.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn file_path(paths: &AppPaths) -> std::path::PathBuf {
    paths.launcher_json.with_file_name("playtime.json")
}

pub fn load(paths: &AppPaths) -> Playtime {
    let path = file_path(paths);
    if !path.is_file() {
        return Playtime::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(paths: &AppPaths, data: &Playtime) -> Result<()> {
    let path = file_path(paths);
    let text = serde_json::to_string_pretty(data).context("Не удалось сериализовать статистику")?;
    std::fs::write(&path, text)
        .with_context(|| format!("Не удалось сохранить {}", path.display()))
}

/// Дописывает завершившуюся сессию. Слишком короткие не сохраняем: если игра
/// упала на старте или игрок сразу передумал, это не «игровое время», а шум,
/// который потом испортит графики на сайте.
pub fn record_session(
    paths: &AppPaths,
    version_id: &str,
    nickname: &str,
    started_at: u64,
    seconds: u64,
) -> Result<()> {
    const MIN_SECONDS: u64 = 60;
    if seconds < MIN_SECONDS {
        log::info!("[playtime] сессия {seconds} с — короче минуты, не записываю");
        return Ok(());
    }

    let mut data = load(paths);
    data.sessions.push(Session {
        version_id: version_id.to_string(),
        nickname: nickname.to_string(),
        started_at,
        seconds,
    });
    log::info!(
        "[playtime] +{} мин, всего {} ч",
        seconds / 60,
        data.total_seconds() / 3600
    );
    save(paths, &data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(version: &str, seconds: u64) -> Session {
        Session {
            version_id: version.to_string(),
            nickname: "furigin".to_string(),
            started_at: 1_700_000_000,
            seconds,
        }
    }

    #[test]
    fn totals_split_by_version() {
        let p = Playtime {
            sessions: vec![
                session("wanderlust-create", 3600),
                session("wanderlust-create", 1800),
                session("stray-souls", 600),
            ],
            synced_count: 0,
        };
        assert_eq!(p.total_seconds(), 6000);
        assert_eq!(p.seconds_for("wanderlust-create"), 5400);
        assert_eq!(p.seconds_for("stray-souls"), 600);
        assert_eq!(p.seconds_for("нет такой"), 0);
    }

    #[test]
    fn unsynced_returns_only_new_sessions() {
        let p = Playtime {
            sessions: vec![session("a", 100), session("b", 200), session("c", 300)],
            synced_count: 2,
        };
        let left = p.unsynced();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].version_id, "c");
    }

    #[test]
    fn unsynced_survives_broken_counter() {
        // Счётчик больше числа сессий — файл могли поправить руками или
        // статистику подчистить. Не должно паниковать на срезе.
        let p = Playtime {
            sessions: vec![session("a", 100)],
            synced_count: 99,
        };
        assert!(p.unsynced().is_empty());
    }
}
