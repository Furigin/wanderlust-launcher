// Сборка финальной команды запуска игры из смёрдженного version JSON.
// Плейсхолдеры и их источники подтверждены разбором реального
// game/versions/*/*.json (см. version.rs) и живым тестовым запуском клиента.
use crate::auth::offline_auth;
use crate::libraries::build_classpath;
use crate::paths::AppPaths;
use crate::progress::ProgressReporter;
use crate::version::{expand_args, MergedVersion};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

const LAUNCHER_NAME: &str = "Wanderlust";
const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn launch_game(
    paths: &AppPaths,
    java_exe: &Path,
    version: &MergedVersion,
    nick: &str,
    ram_mb: u32,
    reporter: &ProgressReporter,
) -> Result<std::process::Child> {
    reporter.report("launch", "Запуск игры", 0, 1);
    let auth = offline_auth(nick);
    let classpath = build_classpath(paths, version);
    let classpath_str = classpath
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(";"); // ';' — разделитель classpath именно на Windows

    let natives_dir = paths.game_dir.join("versions").join(&version.id).join("natives");
    std::fs::create_dir_all(&natives_dir)
        .with_context(|| format!("Не удалось создать {}", natives_dir.display()))?;

    let library_dir = paths.game_dir.join("libraries");
    let assets_dir = paths.game_dir.join("assets");

    let mut placeholders: HashMap<&str, String> = HashMap::new();
    placeholders.insert("auth_player_name", nick.to_string());
    placeholders.insert("version_name", version.id.clone());
    placeholders.insert("game_directory", paths.game_dir.display().to_string());
    placeholders.insert("assets_root", assets_dir.display().to_string());
    placeholders.insert("assets_index_name", version.asset_index.id.clone());
    placeholders.insert("auth_uuid", auth.uuid.to_string());
    placeholders.insert("auth_access_token", auth.access_token.clone());
    placeholders.insert("user_type", auth.user_type.clone());
    placeholders.insert("clientid", "0".to_string());
    placeholders.insert("auth_xuid", "0".to_string());
    placeholders.insert("version_type", "release".to_string());
    placeholders.insert("classpath", classpath_str);
    placeholders.insert("natives_directory", natives_dir.display().to_string());
    placeholders.insert("library_directory", library_dir.display().to_string());
    placeholders.insert("launcher_name", LAUNCHER_NAME.to_string());
    placeholders.insert("launcher_version", LAUNCHER_VERSION.to_string());
    // classpath_separator нужен новым версиям FML для разбора multi-release
    // classpath на разных ОС; на Windows это всегда ';'.
    placeholders.insert("classpath_separator", ";".to_string());

    let jvm_args: Vec<String> = expand_args(&version.jvm_args)
        .iter()
        .map(|a| substitute(a, &placeholders))
        .collect();
    let game_args: Vec<String> = expand_args(&version.game_args)
        .iter()
        .map(|a| substitute(a, &placeholders))
        .collect();

    // Размер кучи в version JSON не задаётся вообще — без явного -Xmx игра
    // стартует с дефолтом JVM (обычно ¼ ОЗУ), чего сборке из сотни модов
    // не хватает и она падает по OutOfMemory. Ставим лимит первым аргументом,
    // чтобы его нельзя было перебить чем-то из version JSON.
    // -Xms держим небольшим: куча вырастет сама, а резервировать всё сразу
    // на машинах с 8 ГБ вредно.
    let heap_args = vec![format!("-Xmx{ram_mb}M"), "-Xms512M".to_string()];

    // jvm_args уже содержит "-cp" "${classpath}" (это часть arguments.jvm
    // в самом version JSON) — substitute() выше уже подставил туда путь,
    // добавлять classpath отдельно не нужно.
    let mut command = std::process::Command::new(java_exe);
    command
        .args(&heap_args)
        .args(&jvm_args)
        .arg(&version.main_class)
        .args(&game_args)
        .current_dir(&paths.game_dir);

    let child = command
        .spawn()
        .context("Не удалось запустить процесс игры (java повреждена или classpath некорректен)")?;
    reporter.report("launch", "Игра запущена", 1, 1);
    Ok(child)
}

fn substitute(template: &str, values: &HashMap<&str, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in values {
        result = result.replace(&format!("${{{key}}}"), value);
    }
    result
}
