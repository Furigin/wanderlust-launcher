// Обёртка над официальным NeoForge installer. Ваниль installer тянет сам —
// отдельно скачивать vanilla version manifest на этом шаге не нужно.
//
// Важная деталь, не описанная ни в одной документации и найденная только
// прогоном реального installer.jar: он отказывается ставить клиент, если
// в целевой папке нет launcher_profiles.json (эмулирует поведение
// ванильного лаунчера — "there is no minecraft launcher profile"). Поэтому
// перед установкой мы сами кладём туда минимальный стаб.
use crate::downloader::{download_and_verify, HashAlgo};
use crate::paths::AppPaths;
use anyhow::{bail, Context, Result};

const MAVEN_BASE: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";

/// Проверяет, установлен ли уже нужный NeoForge, и если нет — ставит его.
pub async fn ensure_neoforge_installed(
    paths: &AppPaths,
    java_exe: &std::path::Path,
    neoforge_version: &str,
) -> Result<()> {
    let version_json = paths
        .game_dir
        .join("versions")
        .join(format!("neoforge-{neoforge_version}"))
        .join(format!("neoforge-{neoforge_version}.json"));

    if version_json.is_file() {
        return Ok(());
    }

    std::fs::create_dir_all(&paths.game_dir)
        .with_context(|| format!("Не удалось создать папку {}", paths.game_dir.display()))?;

    ensure_launcher_profile_stub(paths)?;

    let client = reqwest::Client::new();
    let installer_jar_name = format!("neoforge-{neoforge_version}-installer.jar");
    let installer_url = format!("{MAVEN_BASE}/{neoforge_version}/{installer_jar_name}");
    let sha1_url = format!("{installer_url}.sha1");

    let expected_sha1 = client
        .get(&sha1_url)
        .send()
        .await
        .context("Не удалось получить контрольную сумму установщика NeoForge")?
        .error_for_status()
        .context("Сервер maven.neoforged.net вернул ошибку при запросе sha1")?
        .text()
        .await
        .context("Не удалось прочитать sha1 установщика NeoForge")?
        .trim()
        .to_string();

    let installer_path = paths.tools_dir.join(&installer_jar_name);
    download_and_verify(
        &client,
        &installer_url,
        HashAlgo::Sha1,
        &expected_sha1,
        &installer_path,
    )
    .await
    .context("Не удалось скачать установщик NeoForge")?;

    run_installer_with_retry(java_exe, &installer_path, &paths.game_dir).await?;

    if !version_json.is_file() {
        bail!(
            "Установщик NeoForge отработал без ошибок, но файл {} не появился",
            version_json.display()
        );
    }

    Ok(())
}

fn ensure_launcher_profile_stub(paths: &AppPaths) -> Result<()> {
    let profile_path = paths.game_dir.join("launcher_profiles.json");
    if profile_path.is_file() {
        return Ok(());
    }
    std::fs::write(&profile_path, r#"{"profiles":{},"settings":{},"version":3}"#)
        .with_context(|| format!("Не удалось создать {}", profile_path.display()))
}

const INSTALLER_MAX_ATTEMPTS: u32 = 3;

/// Сам installer.jar внутри себя качает ваниль и библиотеки без единой
/// попытки на ретрай — обрыв DNS или сети на секунду валит всю установку.
/// Поэтому ретраим здесь: повторный запуск безопасен — installer сам
/// пропускает уже скачанные файлы с корректной контрольной суммой.
async fn run_installer_with_retry(
    java_exe: &std::path::Path,
    installer_path: &std::path::Path,
    game_dir: &std::path::Path,
) -> Result<()> {
    let java_exe = java_exe.to_path_buf();
    let installer_path = installer_path.to_path_buf();
    let game_dir = game_dir.to_path_buf();

    let mut last_err = None;
    for attempt in 1..=INSTALLER_MAX_ATTEMPTS {
        let java_exe = java_exe.clone();
        let installer_path = installer_path.clone();
        let game_dir = game_dir.clone();

        // Процесс установщика идёт ~10-30 секунд и блокирует поток — уводим
        // его в spawn_blocking, чтобы не морозить остальной async-рантайм
        // (в Этапе 3 там же крутится обработка GUI-событий).
        let result = tokio::task::spawn_blocking(move || {
            run_installer_once(&java_exe, &installer_path, &game_dir)
        })
        .await
        .context("Поток установщика NeoForge аварийно завершился")?;

        match result {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt < INSTALLER_MAX_ATTEMPTS {
                    let delay_secs = 2u64.pow(attempt - 1);
                    tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                }
            }
        }
    }

    Err(last_err.unwrap()).context(format!(
        "Установка NeoForge не удалась за {INSTALLER_MAX_ATTEMPTS} попыток — проверьте интернет-соединение"
    ))
}

fn run_installer_once(
    java_exe: &std::path::Path,
    installer_path: &std::path::Path,
    game_dir: &std::path::Path,
) -> Result<()> {
    // game_dir уже абсолютный (собран из %APPDATA%), поэтому canonicalize()
    // не нужен — он добавил бы Windows-префикс \\?\, с которым не все
    // сторонние Java-инструменты работают одинаково хорошо.
    //
    // current_dir явно ставим на папку рядом с installer.jar: сам установщик
    // пишет свой installer.jar.log в текущую рабочую директорию процесса,
    // а не в место назначения — без этого лог оседал бы где попало (там,
    // откуда пользователь запустил launcher.exe).
    let installer_dir = installer_path
        .parent()
        .context("У пути к installer.jar нет родительской директории")?;

    let output = std::process::Command::new(java_exe)
        .arg("-jar")
        .arg(installer_path)
        .arg("--install-client")
        .arg(game_dir)
        .current_dir(installer_dir)
        .output()
        .context("Не удалось запустить установщик NeoForge (скачанная Java повреждена?)")?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Установщик NeoForge завершился с ошибкой:\n{stdout}\n{stderr}");
    }

    Ok(())
}
