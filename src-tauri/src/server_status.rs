// Пинг игрового сервера по Server List Ping (протокол Minecraft 1.7+) —
// тот же запрос, что делает сам клиент в списке серверов.
//
// Формат вручную, без внешних крейтов: обмен занимает два пакета, а тянуть
// ради этого зависимость с разбором всего протокола избыточно.
// Пакет = VarInt(длина) + VarInt(id) + данные.
//   1. Handshake: id 0x00, protocol VarInt, адрес String, порт u16, next_state=1
//   2. Status Request: id 0x00, пусто
//   3. Ответ: id 0x00, JSON-строка с players.online / players.max
use anyhow::{bail, Context, Result};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// -1 = "не определился". Сервер всё равно отвечает статусом на любой
/// версии протокола, а так не придётся обновлять число под каждый релиз.
const PROTOCOL_UNKNOWN: i32 = -1;
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);
/// Защита от мусорного/враждебного ответа: JSON статуса — это килобайты,
/// а не мегабайты, и читать по заявленной длине без потолка нельзя.
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub online: bool,
    pub players_online: u32,
    pub players_max: u32,
    /// Задержка до сервера в миллисекундах (время полного обмена).
    pub ping_ms: u64,
}

impl ServerStatus {
    fn offline() -> Self {
        Self { online: false, players_online: 0, players_max: 0, ping_ms: 0 }
    }
}

/// Никогда не возвращает ошибку: недоступный сервер — это нормальное
/// состояние (выключен, перезапускается), а не сбой лаунчера. Виджет в
/// таком случае просто покажет «оффлайн».
pub async fn ping(host: &str, port: u16) -> ServerStatus {
    match tokio::time::timeout(CONNECT_TIMEOUT + READ_TIMEOUT, try_ping(host, port)).await {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => {
            log::debug!("Пинг {host}:{port} не удался: {e:#}");
            ServerStatus::offline()
        }
        Err(_) => {
            log::debug!("Пинг {host}:{port} — таймаут");
            ServerStatus::offline()
        }
    }
}

async fn try_ping(host: &str, port: u16) -> Result<ServerStatus> {
    let started = std::time::Instant::now();

    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .context("Таймаут подключения")?
        .context("Не удалось подключиться к серверу")?;
    let mut stream = stream;
    stream.set_nodelay(true).ok();

    // --- Handshake ---
    let mut payload = Vec::new();
    write_varint(&mut payload, 0x00); // id пакета
    write_varint(&mut payload, PROTOCOL_UNKNOWN);
    write_string(&mut payload, host);
    payload.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut payload, 1); // next state: status
    write_framed(&mut stream, &payload).await?;

    // --- Status Request (пустой) ---
    let mut request = Vec::new();
    write_varint(&mut request, 0x00);
    write_framed(&mut stream, &request).await?;

    // --- Ответ ---
    let len = read_varint(&mut stream).await.context("Не удалось прочитать длину ответа")?;
    if len <= 0 || len as usize > MAX_RESPONSE_BYTES {
        bail!("Некорректная длина ответа: {len}");
    }
    let mut body = vec![0u8; len as usize];
    tokio::time::timeout(READ_TIMEOUT, stream.read_exact(&mut body))
        .await
        .context("Таймаут чтения ответа")?
        .context("Обрыв соединения при чтении ответа")?;

    let mut cursor = std::io::Cursor::new(body);
    let packet_id = read_varint_sync(&mut cursor)?;
    if packet_id != 0x00 {
        bail!("Неожиданный id пакета в ответе: {packet_id}");
    }
    let json_len = read_varint_sync(&mut cursor)?;
    let pos = cursor.position() as usize;
    let buf = cursor.into_inner();
    let end = pos
        .checked_add(json_len.max(0) as usize)
        .filter(|e| *e <= buf.len())
        .context("Длина JSON выходит за границы пакета")?;
    let json: serde_json::Value =
        serde_json::from_slice(&buf[pos..end]).context("Статус сервера — не JSON")?;

    let players = json.get("players");
    Ok(ServerStatus {
        online: true,
        players_online: players.and_then(|p| p.get("online")).and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        players_max: players.and_then(|p| p.get("max")).and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        ping_ms: started.elapsed().as_millis() as u64,
    })
}

/// Отправляет пакет, предваряя его VarInt-длиной.
async fn write_framed(stream: &mut TcpStream, payload: &[u8]) -> Result<()> {
    let mut framed = Vec::with_capacity(payload.len() + 5);
    write_varint(&mut framed, payload.len() as i32);
    framed.extend_from_slice(payload);
    stream.write_all(&framed).await.context("Не удалось отправить пакет")?;
    Ok(())
}

fn write_varint(buf: &mut Vec<u8>, value: i32) {
    let mut v = value as u32;
    loop {
        let mut byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if v == 0 {
            break;
        }
    }
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len() as i32);
    buf.extend_from_slice(s.as_bytes());
}

async fn read_varint(stream: &mut TcpStream) -> Result<i32> {
    let mut result: i32 = 0;
    for shift in 0..5 {
        let mut byte = [0u8; 1];
        tokio::time::timeout(READ_TIMEOUT, stream.read_exact(&mut byte))
            .await
            .context("Таймаут чтения VarInt")?
            .context("Обрыв соединения при чтении VarInt")?;
        result |= ((byte[0] & 0x7F) as i32) << (7 * shift);
        if byte[0] & 0x80 == 0 {
            return Ok(result);
        }
    }
    bail!("VarInt длиннее 5 байт — соединение рассинхронизировано")
}

fn read_varint_sync(cursor: &mut std::io::Cursor<Vec<u8>>) -> Result<i32> {
    let mut result: i32 = 0;
    for shift in 0..5 {
        let mut byte = [0u8; 1];
        // Полное имя: в модуле есть и tokio::AsyncReadExt::read_exact,
        // из-за чего короткий вызов неоднозначен.
        std::io::Read::read_exact(cursor, &mut byte).context("Пакет оборвался на VarInt")?;
        result |= ((byte[0] & 0x7F) as i32) << (7 * shift);
        if byte[0] & 0x80 == 0 {
            return Ok(result);
        }
    }
    bail!("VarInt длиннее 5 байт")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_matches_protocol_examples() {
        // Значения из описания протокола Minecraft.
        let cases: [(i32, &[u8]); 5] = [
            (0, &[0x00]),
            (1, &[0x01]),
            (127, &[0x7F]),
            (128, &[0x80, 0x01]),
            (-1, &[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]),
        ];
        for (value, expected) in cases {
            let mut buf = Vec::new();
            write_varint(&mut buf, value);
            assert_eq!(buf, expected, "write_varint({value})");
        }
    }

    #[test]
    fn varint_roundtrip() {
        for value in [0, 1, 127, 128, 255, 25565, i32::MAX, -1] {
            let mut buf = Vec::new();
            write_varint(&mut buf, value);
            let mut cursor = std::io::Cursor::new(buf);
            assert_eq!(read_varint_sync(&mut cursor).unwrap(), value);
        }
    }

    #[test]
    fn string_is_length_prefixed() {
        let mut buf = Vec::new();
        write_string(&mut buf, "play.example.com");
        assert_eq!(buf[0], 16);
        assert_eq!(&buf[1..], b"play.example.com");
    }

    /// Проверка обмена с настоящим сервером. Помечен ignore: обычный
    /// прогон тестов не должен зависеть от сети и чужого сервера.
    /// Запуск: cargo test -- --ignored pings_real_server
    #[tokio::test]
    #[ignore = "требует доступа в интернет"]
    async fn pings_real_server() {
        let status = ping("mc.hypixel.net", 25565).await;
        assert!(status.online, "сервер должен ответить статусом");
        assert!(status.players_max > 0, "должен вернуться лимит игроков");
    }

    /// Несуществующий адрес — это «оффлайн», а не паника и не ошибка.
    #[tokio::test]
    async fn unreachable_host_is_offline_not_error() {
        // 203.0.113.0/24 — зарезервированный TEST-NET-3, маршрута нет.
        let status = ping("203.0.113.1", 25565).await;
        assert!(!status.online);
        assert_eq!(status.players_online, 0);
    }
}
