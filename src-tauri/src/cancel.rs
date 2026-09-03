// Отмена запуска.
//
// Первая установка — это гигабайт с лишним и минуты ожидания. Раньше выйти
// из неё можно было только закрыв окно, то есть убив процесс на полуслове.
// Здесь — обычный флаг, который пайплайн проверяет между шагами и внутри
// длинных циклов, плюс убийство запущенного packwiz-installer: сам он
// качает моды десятками минут и на флаг посмотреть не может.
//
// Флаг один на весь процесс, и это правильно: одновременно идёт максимум
// один запуск (кнопка «Играть» на время установки прячется).
use std::sync::atomic::{AtomicBool, Ordering};

static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Текст ошибки отмены. Фронт узнаёт её по этой строке и показывает не
/// «ошибку», а обычный экран — отмена не сбой.
pub const CANCELLED_MESSAGE: &str = "Запуск отменён";

pub fn request() {
    CANCELLED.store(true, Ordering::SeqCst);
}

/// Сбрасывается в начале каждого запуска: иначе одна отмена запретила бы
/// все последующие попытки до перезапуска лаунчера.
pub fn reset() {
    CANCELLED.store(false, Ordering::SeqCst);
}

pub fn requested() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

/// Прерывает пайплайн, если отмену уже попросили. Ставится между шагами и
/// внутри циклов скачивания.
pub fn check() -> anyhow::Result<()> {
    if requested() {
        anyhow::bail!("{CANCELLED_MESSAGE}");
    }
    Ok(())
}

/// Отмена ли это. Нужно, чтобы не писать в лог как ошибку то, что игрок
/// сделал сознательно.
pub fn is_cancel(err: &anyhow::Error) -> bool {
    format!("{err:#}").contains(CANCELLED_MESSAGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_round_trip() {
        reset();
        assert!(!requested());
        assert!(check().is_ok());

        request();
        assert!(requested());
        let err = check().expect_err("после request() проверка должна прерывать");
        assert!(is_cancel(&err), "отмена должна опознаваться по тексту");

        // Следующий запуск обязан начинаться с чистого листа.
        reset();
        assert!(check().is_ok());
    }

    #[test]
    fn other_errors_are_not_cancel() {
        let err = anyhow::anyhow!("Не удалось скачать библиотеку foo");
        assert!(!is_cancel(&err));
    }
}
