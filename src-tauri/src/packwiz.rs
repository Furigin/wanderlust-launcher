// Обёртка над packwiz-installer-bootstrap. Флаги и позиционный аргумент
// (URL/путь к pack.toml) подтверждены чтением реального исходника
// packwiz-installer (Main.kt, addNonBootstrapOptions/addBootstrapOptions),
// а весь конвейер синка проверен живым прогоном на тестовом pack.toml.
//
// Сам packwiz-installer решает, что скачать/обновить/удалить в mods/ —
// лаунчер лишь передаёт ему URL и рабочую директорию, никакой логики
// синхронизации здесь не дублируем (см. правило проекта "ничего не удалять
// за пределами того, что лаунчер сам создал").
use crate::downloader::{download_and_verify, HashAlgo};
use crate::paths::AppPaths;
use crate::progress::ProgressReporter;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

const BOOTSTRAP_URL: &str = "https://github.com/packwiz/packwiz-installer-bootstrap/releases/download/v0.0.3/packwiz-installer-bootstrap.jar";
const BOOTSTRAP_SHA256: &str = "a8fbb24dc604278e97f4688e82d3d91a318b98efc08d5dbfcbcbcab6443d116c";

async fn ensure_bootstrap_jar(paths: &AppPaths) -> Result<std::path::PathBuf> {
    let jar_path = paths.tools_dir.join("packwiz-installer-bootstrap.jar");
    if jar_path.is_file() {
        return Ok(jar_path);
    }
    let client = reqwest::Client::new();
    download_and_verify(&client, BOOTSTRAP_URL, HashAlgo::Sha256, BOOTSTRAP_SHA256, &jar_path)
        .await
        .context("Не удалось скачать packwiz-installer-bootstrap")?;
    Ok(jar_path)
}

/// Синхронизирует моды клиентской стороны из `packwiz_url` в `game_dir`.
/// packwiz-installer-bootstrap сам держит packwiz-installer в актуальном
/// состоянии (--bootstrap-no-update здесь НЕ передаём специально — это
/// важно для игроков: обновления самого packwiz-installer должны идти
/// автоматически, как для любого другого системного инструмента лаунчера).
pub async fn sync_modpack(
    paths: &AppPaths,
    java_exe: &std::path::Path,
    packwiz_url: &str,
    reporter: &ProgressReporter,
) -> Result<()> {
    reporter.report("sync", "Синхронизация модов", 0, 1);
    let bootstrap_jar = ensure_bootstrap_jar(paths).await?;
    let bootstrap_jar = bootstrap_jar.clone();
    let java_exe = java_exe.to_path_buf();
    let packwiz_url = packwiz_url.to_string();
    let game_dir = paths.game_dir.clone();
    let tools_dir = paths.tools_dir.clone();

    tokio::task::spawn_blocking(move || {
        run_bootstrap(&java_exe, &bootstrap_jar, &tools_dir, &game_dir, &packwiz_url)
    })
    .await
    .context("Поток packwiz-installer аварийно завершился")??;

    reporter.report("sync", "Моды синхронизированы", 1, 1);
    Ok(())
}

fn run_bootstrap(
    java_exe: &std::path::Path,
    bootstrap_jar: &std::path::Path,
    tools_dir: &std::path::Path,
    game_dir: &std::path::Path,
    packwiz_url: &str,
) -> Result<()> {
    // current_dir — tools_dir: именно туда bootstrap кэширует свою
    // скачанную копию packwiz-installer.jar (проверено эмпирически — она
    // появляется рядом с CWD процесса, а не рядом с самим bootstrap.jar).
    // --pack-folder при этом отдельно указывает, куда класть сами моды.
    let output = std::process::Command::new(java_exe)
        .arg("-jar")
        .arg(bootstrap_jar)
        .arg("--no-gui")
        .arg("--side")
        .arg("client")
        .arg("--pack-folder")
        .arg(game_dir)
        .arg(packwiz_url)
        .current_dir(tools_dir)
        .output()
        .context("Не удалось запустить packwiz-installer")?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Синхронизация модов завершилась с ошибкой:\n{stdout}\n{stderr}");
    }

    Ok(())
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
