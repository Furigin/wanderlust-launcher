// Все файлы лаунчера живут в одной папке %APPDATA%/<PROJECT_DIR_NAME>/,
// чтобы при отладке всё можно было снести одним удалением каталога.
use anyhow::{Context, Result};
use std::path::PathBuf;

// TODO: заменить на реальное имя проекта перед раздачей игрокам.
pub const PROJECT_DIR_NAME: &str = "ProjectLauncher";

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
    pub fn new() -> Result<Self> {
        let appdata = std::env::var_os("APPDATA")
            .context("Переменная окружения APPDATA не найдена — это не Windows?")?;
        let root = PathBuf::from(appdata).join(PROJECT_DIR_NAME);
        let runtime_dir = root.join("runtime");
        Ok(Self {
            jre_dir: runtime_dir.join("jre-25"),
            runtime_dir,
            game_dir: root.join("game"),
            tools_dir: root.join("tools"),
            launcher_json: root.join("launcher.json"),
            log_file: root.join("launcher.log"),
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
