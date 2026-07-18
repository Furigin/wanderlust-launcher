// Офлайн-авторизация: сервер видит именно этот UUID как личность игрока
// (прогресс, права, whitelist), поэтому расхождение с оригинальным
// алгоритмом Java-клиента означает "игрок зашёл как другой человек".
//
// Java: UUID.nameUUIDFromBytes(("OfflinePlayer:" + nick).getBytes(UTF_8))
// Это НЕ RFC4122 v3 name-based UUID (там MD5 берётся от namespace+name) —
// у офлайн-майнкрафта нет namespace, MD5 считается от одной строки.
pub struct OfflineAuth {
    pub uuid: uuid::Uuid,
    pub access_token: String,
    pub user_type: String,
}

pub fn offline_auth(nick: &str) -> OfflineAuth {
    OfflineAuth {
        uuid: offline_uuid(nick),
        // Токен для offline-режима — любая непустая строка, сервер (в offline
        // mode) её не проверяет.
        access_token: "0".to_string(),
        user_type: "legacy".to_string(),
    }
}

pub fn offline_uuid(nick: &str) -> uuid::Uuid {
    let digest = md5::compute(format!("OfflinePlayer:{nick}").as_bytes());
    let mut bytes = *digest;
    bytes[6] = (bytes[6] & 0x0f) | 0x30; // версия 3
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // вариант RFC 4122
    uuid::Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Эталонные значения получены прогоном того же алгоритма (MD5 +
    // выставление version/variant) в Node.js на реальном движке V8 —
    // "Notch" совпадает с широко известным публичным значением его
    // офлайн-UUID, что подтверждает правильность реализации.
    #[test]
    fn known_nick_uuid_pairs() {
        let cases = [
            ("Notch", "b50ad385-829d-3141-a216-7e7d7539ba7f"),
            ("Steve", "5627dd98-e6be-3c21-b8a8-e92344183641"),
            ("TestUser123", "fc484a9b-4142-3457-8016-f639bdb119cf"),
            ("_Test_Nick_", "9a72e415-1a92-3fb6-925c-fd602d780dba"),
        ];
        for (nick, expected) in cases {
            assert_eq!(offline_uuid(nick).to_string(), expected, "nick={nick}");
        }
    }

    #[test]
    fn same_nick_is_deterministic() {
        assert_eq!(offline_uuid("Alex"), offline_uuid("Alex"));
    }

    #[test]
    fn different_nicks_differ() {
        assert_ne!(offline_uuid("Alex"), offline_uuid("Alexx"));
    }
}
