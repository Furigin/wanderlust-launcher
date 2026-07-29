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
use std::os::windows::process::CommandExt;
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

    let mut jvm_args: Vec<String> = expand_args(&version.jvm_args)
        .iter()
        .map(|a| substitute(a, &placeholders))
        .collect();
    patch_ignore_list(&mut jvm_args, &version.vanilla_id);
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
        .current_dir(&paths.game_dir)
        // Не создавать окно консоли для игрового java.exe — иначе оно висит
        // в панели задач всю сессию рядом с окном самой игры (см. lib.rs).
        .creation_flags(crate::CREATE_NO_WINDOW)
        // Перехватываем вывод игры: если она падает до инициализации своего
        // логгера (нехватка памяти, битый мод, несовместимая Java), в
        // logs/latest.log не остаётся ничего, и без этого причина краха
        // теряется полностью. Пайплайн pipe работает и с CREATE_NO_WINDOW.
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Начинаем сессию с чистого файла: иначе лог рос бы бесконечно, а
    // разбирать в нём нужно всегда последний запуск.
    let _ = std::fs::write(&paths.game_log, b"");

    let mut child = command
        .spawn()
        .context("Не удалось запустить процесс игры (java повреждена или classpath некорректен)")?;

    pipe_to_log(child.stdout.take(), &paths.game_log, "out");
    pipe_to_log(child.stderr.take(), &paths.game_log, "err");

    reporter.report("launch", "Игра запущена", 1, 1);
    Ok(child)
}

/// Сливает поток процесса в файл лога построчно, в отдельном потоке.
/// Читать обязательно: если этого не делать, буфер трубы переполнится и
/// игра намертво встанет на первой же попытке что-то напечатать.
fn pipe_to_log<R: std::io::Read + Send + 'static>(
    stream: Option<R>,
    log_path: &Path,
    tag: &'static str,
) {
    let Some(stream) = stream else { return };
    let log_path = log_path.to_path_buf();
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .ok();
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if let Some(f) = file.as_mut() {
                let _ = writeln!(f, "[{tag}] {line}");
            }
        }
    });
}

/// NeoForge передаёт `-DignoreList=client-extra,${version_name}.jar` — это
/// файлы classpath, которые BootstrapLauncher не должен превращать в модули.
/// `${version_name}` — это id запускаемой версии (neoforge-21.1.243), а
/// ванильный client.jar лежит под именем родительской (1.21.1.jar) и в список
/// не попадает. Java делает из него автоматический модуль `_1._21._1`, тот
/// экспортирует те же пакеты, что и модуль `minecraft`, и запуск падает с
/// ResolutionException ещё до появления окна. Дописываем его в список сами.
fn patch_ignore_list(args: &mut [String], vanilla_id: &str) {
    const PREFIX: &str = "-DignoreList=";
    let entry = format!("{vanilla_id}.jar");
    for arg in args.iter_mut() {
        let Some(list) = arg.strip_prefix(PREFIX) else { continue };
        if !list.split(',').any(|item| item.trim() == entry) {
            *arg = format!("{PREFIX}{list},{entry}");
        }
        return;
    }
}

fn substitute(template: &str, values: &HashMap<&str, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in values {
        result = result.replace(&format!("${{{key}}}"), value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::patch_ignore_list;

    #[test]
    fn appends_vanilla_jar_to_ignore_list() {
        let mut args = vec![
            "-p".to_string(),
            "-DignoreList=client-extra,neoforge-21.1.243.jar".to_string(),
        ];
        patch_ignore_list(&mut args, "1.21.1");
        assert_eq!(args[1], "-DignoreList=client-extra,neoforge-21.1.243.jar,1.21.1.jar");
    }

    #[test]
    fn does_not_duplicate_existing_entry() {
        let mut args = vec!["-DignoreList=client-extra,1.21.1.jar".to_string()];
        patch_ignore_list(&mut args, "1.21.1");
        assert_eq!(args[0], "-DignoreList=client-extra,1.21.1.jar");
    }

    #[test]
    fn leaves_args_alone_when_no_ignore_list() {
        let mut args = vec!["-Xmx4096M".to_string(), "-cp".to_string()];
        let before = args.clone();
        patch_ignore_list(&mut args, "1.21.1");
        assert_eq!(args, before);
    }
}
