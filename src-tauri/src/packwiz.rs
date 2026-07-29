// Обёртка над packwiz-installer. Флаги и позиционный аргумент (URL/путь к
// pack.toml) подтверждены чтением реального исходника packwiz-installer
// (Main.kt, addNonBootstrapOptions), а весь конвейер синка проверен живым
// прогоном на боевом паке.
//
// Сам packwiz-installer решает, что скачать/обновить/удалить в mods/ —
// лаунчер лишь передаёт ему URL и рабочую директорию, никакой логики
// синхронизации здесь не дублируем (см. правило проекта "ничего не удалять
// за пределами того, что лаунчер сам создал").
//
// ВАЖНО: bootstrap-обёртку (packwiz-installer-bootstrap) мы намеренно НЕ
// используем. Она при каждом запуске ходит в api.github.com за информацией о
// последнем релизе, и это оказалось источником массовых отказов у игроков:
//   * у GitHub API лимит ~60 запросов в час на IP — игроки упирались в него
//     и получали "403 for URL: https://api.github.com/.../releases/latest";
//   * оборванная на середине докачка оставляла битый packwiz-installer.jar,
//     после чего КАЖДЫЙ следующий запуск падал с ClassNotFoundException,
//     даже когда сеть уже работала.
// Поэтому версию installer'а мы пиним сами, качаем по прямой ссылке на
// релизный файл (без API) и проверяем sha256. Jar запускается через -cp:
// его Main-Class — заглушка RequiresBootstrap, а рабочая точка входа лежит
// в классе link.infra.packwiz.installer.Main.
use crate::downloader::{download_and_verify, HashAlgo};
use crate::paths::AppPaths;
use crate::progress::ProgressReporter;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

const INSTALLER_URL: &str =
    "https://github.com/packwiz/packwiz-installer/releases/download/v0.5.14/packwiz-installer.jar";
const INSTALLER_SHA256: &str = "c9f646908d340d84773948a9a7d98bc1dae250d35e1016dc6e2b8459760b5598";
/// Точка входа внутри jar. Main-Class в манифесте — заглушка, требующая
/// bootstrap, поэтому вызываем рабочий класс напрямую.
const INSTALLER_MAIN_CLASS: &str = "link.infra.packwiz.installer.Main";

/// Отдаёт путь к проверенному packwiz-installer.jar, докачивая его при
/// необходимости. Файл с несовпавшим размером/хешем удаляется и качается
/// заново: именно «застрявший» битый jar ломал запуск у игроков навсегда.
async fn ensure_installer_jar(paths: &AppPaths) -> Result<std::path::PathBuf> {
    let jar_path = paths.tools_dir.join("packwiz-installer.jar");

    if jar_path.is_file() {
        match jar_is_valid(&jar_path) {
            Ok(true) => return Ok(jar_path),
            Ok(false) => {
                log::warn!("packwiz-installer.jar повреждён — качаем заново");
                let _ = std::fs::remove_file(&jar_path);
            }
            Err(e) => {
                log::warn!("Не удалось проверить packwiz-installer.jar ({e:#}) — качаем заново");
                let _ = std::fs::remove_file(&jar_path);
            }
        }
    }

    let client = reqwest::Client::new();
    download_and_verify(&client, INSTALLER_URL, HashAlgo::Sha256, INSTALLER_SHA256, &jar_path)
        .await
        .context("Не удалось скачать packwiz-installer")?;
    Ok(jar_path)
}

/// Быстрая проверка целостности: jar — это zip, и в нём обязан быть класс
/// точки входа. Оборванная закачка даёт битый zip и отсекается здесь.
fn jar_is_valid(jar_path: &std::path::Path) -> Result<bool> {
    let file = std::fs::File::open(jar_path).context("Не удалось открыть packwiz-installer.jar")?;
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(z) => z,
        Err(_) => return Ok(false), // не zip => обрыв закачки
    };
    // Результат кладём в переменную: иначе временный ZipFile переживёт
    // архив и borrow checker справедливо ругается.
    let has_main = zip.by_name("link/infra/packwiz/installer/Main.class").is_ok();
    Ok(has_main)
}

/// Синхронизирует моды клиентской стороны из `packwiz_url` в `game_dir`.
pub async fn sync_modpack(
    paths: &AppPaths,
    java_exe: &std::path::Path,
    packwiz_url: &str,
    reporter: &ProgressReporter,
) -> Result<()> {
    reporter.report("sync", "Синхронизация модов", 0, 1);
    let installer_jar = ensure_installer_jar(paths).await?;
    let java_exe = java_exe.to_path_buf();
    let packwiz_url = packwiz_url.to_string();
    let game_dir = paths.game_dir.clone();
    let tools_dir = paths.tools_dir.clone();

    let reporter_for_task = reporter.clone();
    tokio::task::spawn_blocking(move || {
        run_installer(
            &java_exe,
            &installer_jar,
            &tools_dir,
            &game_dir,
            &packwiz_url,
            &reporter_for_task,
        )
    })
    .await
    .context("Поток packwiz-installer аварийно завершился")??;

    reporter.report("sync", "Моды синхронизированы", 1, 1);
    Ok(())
}

fn run_installer(
    java_exe: &std::path::Path,
    installer_jar: &std::path::Path,
    tools_dir: &std::path::Path,
    game_dir: &std::path::Path,
    packwiz_url: &str,
    reporter: &ProgressReporter,
) -> Result<()> {
    use std::io::{BufRead, BufReader};
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;

    // Запуск через -cp, а не -jar: Main-Class в манифесте — заглушка
    // RequiresBootstrap, которая просто печатает «используйте bootstrap»
    // и выходит. Рабочая точка входа — отдельный класс.
    // --pack-folder указывает, куда класть моды; current_dir на tools_dir
    // держит служебные файлы installer'а рядом с ним, а не в папке игры.
    //
    // Читаем stdout построчно, а не через .output(): скачивание сотен модов
    // занимает минуты, и без живого прогресса игрок видит замерший на нуле
    // индикатор и решает, что лаунчер завис. packwiz-installer печатает
    // строки вида "(45/491) Downloaded foo.jar" — из них и берём счётчик.
    let mut child = std::process::Command::new(java_exe)
        .arg("-cp")
        .arg(installer_jar)
        .arg(INSTALLER_MAIN_CLASS)
        .arg("--no-gui")
        .arg("--side")
        .arg("client")
        .arg("--pack-folder")
        .arg(game_dir)
        .arg(packwiz_url)
        .current_dir(tools_dir)
        .creation_flags(crate::CREATE_NO_WINDOW) // без мелькающего окна консоли при синке
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Не удалось запустить packwiz-installer")?;

    let stdout = child.stdout.take().expect("stdout запрошен как piped");
    let stderr = child.stderr.take().expect("stderr запрошен как piped");

    // stderr читаем отдельным потоком: если его не вычитывать, буфер трубы
    // переполнится и процесс намертво встанет на записи в него.
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            buf.push_str(&line);
            buf.push('\n');
        }
        buf
    });

    let mut tail = Vec::new();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some((current, total)) = parse_progress_line(&line) {
            reporter.report("sync", "Скачивание модов", current, total);
        }
        // Держим только хвост — на случай ошибки его покажем игроку,
        // полный лог из сотен строк в сообщение об ошибке не нужен.
        tail.push(line);
        if tail.len() > 40 {
            tail.remove(0);
        }
    }

    let status = child.wait().context("Не удалось дождаться packwiz-installer")?;
    let stderr_text = stderr_handle.join().unwrap_or_default();

    if !status.success() {
        let stdout_text = tail.join("\n");
        bail!("Синхронизация модов завершилась с ошибкой:\n{stdout_text}\n{stderr_text}");
    }

    Ok(())
}

/// Вытаскивает счётчик из строки вида "(45/491) Downloaded foo.jar".
/// Возвращает None для всех прочих строк вывода.
fn parse_progress_line(line: &str) -> Option<(u64, u64)> {
    let rest = line.strip_prefix('(')?;
    let (numbers, _) = rest.split_once(')')?;
    let (current, total) = numbers.split_once('/')?;
    Some((current.trim().parse().ok()?, total.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::parse_progress_line;

    #[test]
    fn parses_packwiz_counter() {
        assert_eq!(parse_progress_line("(45/491) Downloaded foo.jar"), Some((45, 491)));
        assert_eq!(parse_progress_line("(1/1) Modpack files are already up to date!"), Some((1, 1)));
        // Строки без счётчика игнорируем, а не падаем на них.
        assert_eq!(parse_progress_line("Loading manifest file..."), None);
        assert_eq!(parse_progress_line("(abc/def) мусор"), None);
        assert_eq!(parse_progress_line(""), None);
    }
}

/// CLI-режим packwiz-installer НЕ умеет спрашивать про опциональные моды —
/// showOptions() в CLIHandler.kt безусловно ставит optionValue = true для
/// всех опций с комментарием "option choosing is not implemented in the
/// CLI" (подтверждено чтением исходника). Поэтому свой выбор мы применяем
/// отдельным проходом: патчим packwiz.json (тот самый файл, который
/// packwiz-installer сам ведёт и уважает при следующем запуске — см.
/// UpdateManager.kt: если у файла isOptional и optionValue == false, его
/// наличие на диске вообще не проверяется и он не перекачивается).
///
/// Возвращает true, если после патча нужен повторный sync_modpack() —
/// то есть игрок включил мод, которого ещё нет на диске.
pub async fn reconcile_optional_mods(
    paths: &AppPaths,
    desired: &HashMap<String, bool>,
) -> Result<bool> {
    let manifest_path = paths.game_dir.join("packwiz.json");
    if !manifest_path.is_file() || desired.is_empty() {
        return Ok(false);
    }

    let text = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Не удалось прочитать {}", manifest_path.display()))?;
    let mut manifest: serde_json::Value =
        serde_json::from_str(&text).context("packwiz.json повреждён — удалите его и синхронизируйте заново")?;

    let mut needs_resync = false;

    let Some(cached_files) = manifest
        .get_mut("cachedFiles")
        .and_then(|v| v.as_object_mut())
    else {
        return Ok(false);
    };

    for (mod_id, &wanted) in desired {
        let Some(entry) = cached_files.get_mut(mod_id) else {
            // Ещё ни разу не синкался (первая установка) — реагировать не на что,
            // выбор применится сам после того как packwiz о нём узнает.
            continue;
        };
        let is_optional = entry.get("isOptional").and_then(|v| v.as_bool()).unwrap_or(false);
        if !is_optional {
            continue;
        }
        let current = entry.get("optionValue").and_then(|v| v.as_bool()).unwrap_or(true);
        if current == wanted {
            continue;
        }

        if !wanted {
            // Выключили — удаляем физический файл сами: packwiz, единожды
            // решив "optionValue = false", больше не проверяет и не трогает
            // этот файл (см. комментарий выше), поэтому мусор за собой не уберёт.
            if let Some(location) = entry.get("cachedLocation").and_then(|v| v.as_str()) {
                let file_path = paths.game_dir.join(location);
                if file_path.is_file() {
                    let _ = std::fs::remove_file(&file_path);
                }
            }
        } else {
            // Включили — файла ещё нет на диске, следующий sync_modpack()
            // его докачает (isOptional && optionValue==true && cachedLocation
            // не существует физически => packwiz помечает файл invalidated).
            needs_resync = true;
        }

        entry["optionValue"] = serde_json::Value::Bool(wanted);
    }

    let updated = serde_json::to_string_pretty(&manifest).context("Не удалось сериализовать packwiz.json")?;
    std::fs::write(&manifest_path, updated)
        .with_context(|| format!("Не удалось сохранить {}", manifest_path.display()))?;

    Ok(needs_resync)
}
