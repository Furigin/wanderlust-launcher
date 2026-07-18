// Единый канал отчёта о прогрессе для всех долгих операций. Не завязан на
// Tauri напрямую (jre.rs/assets.rs и т.д. остаются тестируемыми без GUI) —
// конкретную отправку события в окно делает замыкание, которое передаёт
// lib.rs при вызове команды `launch`.
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProgressEvent {
    pub stage: String,
    pub label: String,
    pub current: u64,
    pub total: u64,
}

#[derive(Clone)]
pub struct ProgressReporter(Arc<dyn Fn(ProgressEvent) + Send + Sync>);

impl ProgressReporter {
    pub fn new(f: impl Fn(ProgressEvent) + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    /// Для консольного режима / тестов, где событие некуда отправлять.
    pub fn noop() -> Self {
        Self::new(|_| {})
    }

    pub fn report(&self, stage: &str, label: &str, current: u64, total: u64) {
        (self.0)(ProgressEvent {
            stage: stage.to_string(),
            label: label.to_string(),
            current,
            total,
        });
    }
}
