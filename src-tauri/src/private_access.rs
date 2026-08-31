// Доступ к закрытым сборкам по коду.
//
// Задача была не «спрятать кнопку», а сделать так, чтобы о закрытой сборке
// нельзя было узнать вообще. Проверка пароля внутри лаунчера этого не даёт:
// публичный манифест лежит открыто, и любой, кто откроет его в браузере,
// увидит и сборку, и прямые ссылки на все её моды — программу можно даже
// не запускать.
//
// Поэтому закрытая сборка вообще не упоминается в публичном манифесте.
// Её манифест, пак и jar-ы лежат по адресу, который считается из кода:
//
//     p/<sha256(SALT:код)[..40]>/manifest.json
//
// Нет кода — нет адреса. Подобрать его перебором нельзя: проверить догадку
// можно только запросом к серверу, а это не офлайн-перебор хешей.
//
// Соль лежит в коде лаунчера, и это нормально: она не секрет, а привязка
// схемы к нашему проекту. Секрет — сам код доступа.

use anyhow::Result;
use sha2::{Digest, Sha256};

/// Привязывает схему к нашему проекту: без неё путь считался бы «просто
/// хешем пароля», одинаковым у всех, кто использует такой же приём.
const SALT: &str = "wanderlust-private-v1";

/// Путь к папке закрытой сборки для данного кода.
pub fn secret_path(code: &str) -> String {
    let mut h = Sha256::new();
    h.update(format!("{SALT}:{}", code.trim()));
    format!("p/{}", &hex::encode(h.finalize())[..40])
}

/// Адрес приватного манифеста рядом с публичным.
pub fn manifest_url(public_manifest_url: &str, code: &str) -> String {
    let base = public_manifest_url
        .rsplit_once('/')
        .map(|(b, _)| b)
        .unwrap_or(public_manifest_url);
    format!("{base}/{}/manifest.json", secret_path(code))
}

/// Проверяет код и отдаёт приватный манифест.
///
/// `Ok(None)` — код не подошёл (по этому адресу ничего нет). Ошибку
/// возвращаем только на реальных проблемах сети, чтобы не путать игрока
/// «неверным кодом», когда у него просто нет интернета.
pub async fn fetch_private_manifest(
    public_manifest_url: &str,
    code: &str,
) -> Result<Option<crate::manifest::Manifest>> {
    let url = manifest_url(public_manifest_url, code);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND
        || resp.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Ok(None);
    }
    let resp = resp.error_for_status()?;
    let text = resp.text().await?;
    match serde_json::from_str::<crate::manifest::Manifest>(&text) {
        Ok(m) => Ok(Some(m)),
        // По адресу что-то есть, но это не манифест — для игрока это тот же
        // «код не подошёл», а подробности уйдут в лог.
        Err(e) => {
            log::warn!("[private] по адресу кода лежит не манифест: {e}");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_depends_on_code() {
        let a = secret_path("sosybiby");
        let b = secret_path("sosybibz");
        assert_ne!(a, b, "разные коды должны давать разные пути");
        assert!(a.starts_with("p/"));
        assert_eq!(a.len(), 2 + 40);
    }

    #[test]
    fn path_is_stable_and_trims_input() {
        // Игрок почти наверняка вставит код с пробелом или переводом строки.
        assert_eq!(secret_path("sosybiby"), secret_path("  sosybiby\n"));
    }

    #[test]
    fn manifest_url_sits_next_to_public_one() {
        let url = manifest_url("https://example.com/manifest.json", "sosybiby");
        assert!(url.starts_with("https://example.com/p/"));
        assert!(url.ends_with("/manifest.json"));
    }
}
