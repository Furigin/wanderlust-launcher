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
use anyhow::{bail, Context, Result};

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
pub async fn sync_modpack(paths: &AppPaths, java_exe: &std::path::Path, packwiz_url: &str) -> Result<()> {
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
