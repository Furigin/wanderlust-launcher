// Все файлы лаунчера живут в одной папке %APPDATA%/<PROJECT_DIR_NAME>/,
// чтобы при отладке всё можно было снести одним удалением каталога.
//
// С мультиверсиями каждая версия получает свою game-папку
// (instances/<version_id>) и свою JRE (runtime/jre-<major>): разные версии
// Minecraft требуют разной Java и не должны делить mods/. Общие для всех
// версий вещи (tools/, настройки, лог) остаются в корне.
use anyhow::{Context, Result};
use std::path::PathBuf;

// Имя папки в %APPDATA%. Менять после релиза нельзя: у игроков установка
// уедет на новое место и всё (JRE, NeoForge, моды) перекачается заново.
pub const PROJECT_DIR_NAME: &str = "Wanderlust";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub runtime_dir: PathBuf,
    pub jre_dir: PathBuf,
    pub game_dir: PathBuf,
    pub tools_dir: PathBuf,
    pub launcher_json: PathBuf,
    pub log_file: PathBuf,
}

impl AppPaths {
    fn root_and_runtime() -> Result<(PathBuf, PathBuf)> {
        let appdata = std::env::var_os("APPDATA")
            .context("Переменная окружения APPDATA не найдена — это не Windows?")?;
        let root = PathBuf::from(appdata).join(PROJECT_DIR_NAME);
        let runtime_dir = root.join("runtime");
        Ok((root, runtime_dir))
    }

    /// Пути без привязки к версии — для команд, которым нужен только корень
    /// (настройки, tools). `game_dir`/`jre_dir` здесь указывают на общие
    /// каталоги-родители и напрямую такими командами не используются.
    pub fn global() -> Result<Self> {
        let (root, runtime_dir) = Self::root_and_runtime()?;
        Ok(Self {
            jre_dir: runtime_dir.join("jre"),
            game_dir: root.join("instances"),
            tools_dir: root.join("tools"),
            launcher_json: root.join("launcher.json"),
            log_file: root.join("launcher.log"),
            runtime_dir,
            root,
        })
    }

    /// Пути конкретной версии: своя game-папка (instances/<version_id>) и своя
    /// JRE (runtime/jre-<major>). Настройки и tools — общие для всех версий.
    pub fn for_version(version_id: &str, java_major: u32) -> Result<Self> {
        let (root, runtime_dir) = Self::root_and_runtime()?;
        Ok(Self {
            jre_dir: runtime_dir.join(format!("jre-{java_major}")),
            game_dir: root.join("instances").join(version_id),
            tools_dir: root.join("tools"),
            launcher_json: root.join("launcher.json"),
            log_file: root.join("launcher.log"),
            runtime_dir,
            root,
        })
    }

    /// Создаёт все базовые директории, если их ещё нет. Игровые данные
    /// (mods/config/saves) создаёт сам packwiz/Minecraft — их не трогаем.
    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [&self.root, &self.runtime_dir, &self.game_dir, &self.tools_dir] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("Не удалось создать папку {}", dir.display()))?;
        }
        Ok(())
    }

    pub fn java_exe(&self) -> PathBuf {
        self.jre_dir.join("bin").join("java.exe")
    }
}
