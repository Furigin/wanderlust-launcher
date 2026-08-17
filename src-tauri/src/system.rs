// Сведения о железе игрока — нужны, чтобы не заставлять его угадывать размер
// памяти для игры. Раньше всем ставилось 4 ГБ независимо от машины: на 8 ГБ
// это впритык, а на 32 ГБ — необоснованно мало для сборки с сотней модов.
//
// Отдельная беда — игроки, которые выкручивают ползунок на максимум «чтобы
// быстрее». Java резервирует эту память сразу, и если её нет физически,
// система уходит в своп: игра не падает, а просто дико тормозит, и виноватым
// оказывается сервер. Поэтому здесь же считается безопасный потолок.

/// Сколько всего оперативной памяти в машине, в мегабайтах.
/// `None` — определить не удалось (не Windows или отказал системный вызов).
pub fn total_ram_mb() -> Option<u32> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{
            GlobalMemoryStatusEx, MEMORYSTATUSEX,
        };

        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..unsafe { std::mem::zeroed() }
        };

        // SAFETY: структура заполнена по контракту WinAPI — dwLength выставлен,
        // остальное обнулено. Функция только пишет в неё.
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        if ok == 0 {
            return None;
        }
        Some((status.ullTotalPhys / 1024 / 1024) as u32)
    }

    #[cfg(not(windows))]
    {
        None
    }
}

/// Сколько памяти разумно отдать игре на этой машине.
///
/// Половина от общего объёма, но не выходя за границы:
///   * нижняя — 3 ГБ: со сборкой такого размера меньше просто не стартует;
///   * верхняя — 8 ГБ: дальше выигрыша нет, а паузы сборщика мусора растут.
///
/// Значение округляется до 512 МБ, чтобы в интерфейсе не появлялись числа
/// вроде «5734 МБ».
pub fn recommended_ram_mb(total_mb: u32) -> u32 {
    let half = total_mb / 2;
    let clamped = half.clamp(3072, 8192);
    (clamped / 512) * 512
}

/// Потолок, выше которого выделять память опасно: системе и всему остальному
/// нужно оставить хотя бы 2 ГБ, иначе машина уйдёт в своп.
pub fn safe_max_ram_mb(total_mb: u32) -> u32 {
    total_mb.saturating_sub(2048).max(2048)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_stays_within_bounds() {
        // Слабая машина: половина меньше нижней границы — берём минимум.
        assert_eq!(recommended_ram_mb(4096), 3072);
        // Типичные 8 ГБ: ровно половина.
        assert_eq!(recommended_ram_mb(8192), 4096);
        // 16 ГБ: половина, всё ещё под потолком.
        assert_eq!(recommended_ram_mb(16384), 8192);
        // 64 ГБ: упираемся в верхнюю границу, а не отдаём 32 ГБ.
        assert_eq!(recommended_ram_mb(65536), 8192);
    }

    #[test]
    fn recommendation_is_multiple_of_512() {
        for total in [6000, 7000, 12000, 13000, 20000] {
            assert_eq!(recommended_ram_mb(total) % 512, 0, "total={total}");
        }
    }

    #[test]
    fn safe_max_leaves_room_for_system() {
        assert_eq!(safe_max_ram_mb(16384), 14336);
        // На совсем слабой машине не уходим в ноль и не паникуем.
        assert_eq!(safe_max_ram_mb(2048), 2048);
        assert_eq!(safe_max_ram_mb(1024), 2048);
    }
}
