// Поиск запрещённых модов и ресурспаков по именам файлов.
//
// Фильтр намеренно простейший — совпадение подстроки в имени. Он ловит того,
// кто скачал чит и кинул в папку как есть, и не ловит никого, кто догадался
// файл переименовать. Это осознанный выбор, а не недоделка: правила лежат в
// манифесте и пополняются без пересборки лаунчера.
//
// Наружу отсюда ничего не уходит: находка просто не пускает игрока в игру и
// показывает ему, куда писать. Ни отчётов, ни сети — весь разбор офлайн,
// глазами администратора, по скриншоту от самого игрока.
//
// Что важно про ложные срабатывания: правило "xray" сработает и на честном
// antixray-моде, и на паке вроде "no-xray-textures". Список стоит держать
// узким — цена ошибки здесь не «лишняя строчка в логе», а не запустившаяся
// игра у невиновного.
use serde::Serialize;
use std::path::Path;

/// Папки внутри game-директории, где вообще имеет смысл искать. Остальное
/// (saves, config, logs) не трогаем: там нет чужих модов, а лишний обход —
/// только время и повод для вопросов к лаунчеру.
const SCANNED_DIRS: [&str; 3] = ["mods", "resourcepacks", "shaderpacks"];

#[derive(Debug, Clone, Serialize)]
pub struct Detection {
    /// Имя файла как есть на диске — именно его игрок увидит в окне блокировки.
    pub file: String,
    /// Где нашли: mods / resourcepacks / shaderpacks.
    pub folder: String,
    /// Правило из блоклиста, которое сработало.
    pub rule: String,
}

/// Приводит имя к виду, устойчивому к косметике: «X-Ray_Ultimate v2.4.zip» и
/// «xrayultimate.zip» дают одну и ту же строку. Благодаря этому одно правило
/// "xray" покрывает и "x-ray", и "X Ray", и "x_ray" — писать их в блоклист
/// по отдельности не нужно.
fn normalize(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Обходит игровые папки и возвращает всё, что совпало с блоклистом.
/// Ошибки чтения не поднимаются наверх: отсутствующая shaderpacks/ — норма.
pub fn scan(game_dir: &Path, blocklist: &[String]) -> Vec<Detection> {
    let rules: Vec<(String, String)> = blocklist
        .iter()
        .map(|r| (normalize(r), r.clone()))
        .filter(|(norm, _)| !norm.is_empty())
        .collect();

    if rules.is_empty() {
        return Vec::new();
    }

    let mut found = Vec::new();

    for folder in SCANNED_DIRS {
        let dir = game_dir.join(folder);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let normalized = normalize(&name);

            // Первое совпавшее правило — достаточно: игроку показываем имя
            // файла, а не разбор по каким правилам оно прошло.
            if let Some((_, original)) = rules.iter().find(|(norm, _)| normalized.contains(norm.as_str())) {
                found.push(Detection {
                    file: name,
                    folder: folder.to_string(),
                    rule: original.clone(),
                });
            }
        }
    }

    found
}

/// Код в тексте ошибки. Для игрока — техническая абракадабра, для админа —
/// однозначный признак: пришли с этим кодом, значит сработал блоклист.
/// Отличать его от настоящих сбоев больше не по чему: наружу лаунчер о
/// находке не сообщает, а список файлов остаётся только в launcher.log.
const BLOCK_CODE: &str = "0x4C3";

/// Текст, который увидит игрок вместо игры.
///
/// Намеренно маскируется под рядовой сбой проверки файлов: ни слова про
/// читы, ни имён найденного. Причина не в вежливости — узнав точную причину,
/// человек просто переименует файл и зайдёт снова, а так он идёт к админу и
/// приносит скриншот сам.
///
/// Коротко тоже намеренно: блок ошибки на фронте живёт внутри контейнера
/// высотой 64px вместе с кнопками (.action-area в style.css), и длинный
/// текст там обрежется на середине.
pub fn block_message(contact: &str) -> String {
    if contact.trim().is_empty() {
        format!("Ошибка проверки целостности файлов ({BLOCK_CODE}). Обратись к администратору.")
    } else {
        format!("Ошибка проверки целостности файлов ({BLOCK_CODE}). Напиши в Telegram {contact}")
    }
}

#[cfg(test)]
mod tests {
    use super::{block_message, normalize, scan};

    #[test]
    fn normalize_strips_cosmetics() {
        assert_eq!(normalize("X-Ray_Ultimate v2.4.zip"), "xrayultimatev24zip");
        // Именно это свойство даёт одному правилу "xray" ловить все написания.
        assert!(normalize("X Ray Pack.zip").contains(&normalize("xray")));
        assert!(normalize("x_ray.jar").contains(&normalize("xray")));
    }

    #[test]
    fn finds_planted_files() {
        let dir = std::env::temp_dir().join(format!("wl-cheat-test-{}", std::process::id()));
        let mods = dir.join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("X-Ray_Ultimate.zip"), b"x").unwrap();
        std::fs::write(mods.join("create-1.21.1.jar"), b"x").unwrap();

        let hits = scan(&dir, &["xray".to_string(), "wurst".to_string()]);

        assert_eq!(hits.len(), 1, "должен сработать ровно один файл");
        assert_eq!(hits[0].file, "X-Ray_Ultimate.zip");
        assert_eq!(hits[0].folder, "mods");
        assert_eq!(hits[0].rule, "xray");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_blocklist_scans_nothing() {
        assert!(scan(std::path::Path::new("."), &[]).is_empty());
    }

    /// Главное свойство сообщения — оно НЕ должно выдавать причину. Если
    /// текст once проговорится про читы или назовёт файл, игрок переименует
    /// его и пройдёт мимо блокировки, а смысл затеи пропадёт.
    #[test]
    fn message_hides_the_real_reason() {
        let msg = block_message("@MegoCat").to_lowercase();

        for leak in ["чит", "xray", "cheat", "блокир", "запрещ", "mods", "заблок"] {
            assert!(!msg.contains(leak), "текст выдаёт причину словом «{leak}»: {msg}");
        }
        assert!(msg.contains("@megocat"), "контакт обязан остаться");

        // Без контакта в манифесте текст всё равно осмысленный.
        assert!(block_message("").to_lowercase().contains("администратору"));
    }

    /// Тот же блок ошибки показывает и обычные сбои, а он ограничен двумя
    /// строками (см. .error-text в style.css) — длинный текст обрежется.
    #[test]
    fn message_is_short_enough_for_the_error_box() {
        let msg = block_message("@MegoCat");
        assert!(msg.chars().count() <= 90, "не влезет в блок ошибки: {} симв.", msg.chars().count());
        assert!(!msg.contains('\n'), "переносы в этом блоке не отображаются");
    }
}
