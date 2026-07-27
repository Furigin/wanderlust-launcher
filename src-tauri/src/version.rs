// Разбор и слияние version JSON. Формат и алгоритм rules/os/features проверены
// эмпирически на реальном game/versions/*/*.json после установки, а не по
// памяти: см. запись в git-истории — mainClass NeoForge не входит ни в один
// libraries-массив, а находится динамически через -DlibraryDirectory.
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct RawVersion {
    pub id: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(rename = "inheritsFrom")]
    pub inherits_from: Option<String>,
    pub arguments: Option<RawArguments>,
    #[serde(default)]
    pub libraries: Vec<RawLibrary>,
    #[serde(rename = "assetIndex")]
    pub asset_index: Option<AssetIndexRef>,
    pub assets: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawArguments {
    #[serde(default)]
    pub game: Vec<ArgEntry>,
    #[serde(default)]
    pub jvm: Vec<ArgEntry>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum ArgEntry {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        value: StringOrList,
    },
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum StringOrList {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    pub action: String,
    pub os: Option<OsRule>,
    pub features: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawLibrary {
    pub name: String,
    pub downloads: Option<LibraryDownloads>,
    pub rules: Option<Vec<Rule>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Artifact {
    pub path: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AssetIndexRef {
    pub id: String,
    pub url: String,
    pub sha1: String,
}

/// Результат этой функции — что реально нужно для запуска: mainClass и
/// arguments берём у ребёнка (NeoForge), assetIndex/assets — у родителя
/// (ваниль их не переопределяет), libraries — объединение обоих списков.
pub struct MergedVersion {
    pub id: String,
    pub vanilla_id: String,
    pub main_class: String,
    pub game_args: Vec<ArgEntry>,
    pub jvm_args: Vec<ArgEntry>,
    pub libraries: Vec<RawLibrary>,
    pub asset_index: AssetIndexRef,
    pub assets: String,
}

pub fn load_merged_version(game_dir: &Path, neoforge_id: &str) -> Result<MergedVersion> {
    let child = read_version_json(game_dir, neoforge_id)?;
    let parent_id = child
        .inherits_from
        .clone()
        .context("У версии NeoForge нет inheritsFrom — не на что опереться для assetIndex/ванильных библиотек")?;
    let parent = read_version_json(game_dir, &parent_id)?;

    let child_args = child.arguments.unwrap_or(RawArguments { game: vec![], jvm: vec![] });
    let parent_args = parent.arguments.unwrap_or(RawArguments { game: vec![], jvm: vec![] });

    // Ваниль и NeoForge объявляют часть библиотек совместно (gson, guava,
    // commons-*). Если оставить оба вхождения, один и тот же jar попадёт в
    // classpath дважды, и BootstrapLauncher падает ещё до старта игры:
    // "IllegalStateException: Duplicate key ... gson-2.10.1.jar".
    // Правило стандартное: версия ребёнка (NeoForge) перекрывает родительскую,
    // позиция при этом сохраняется — порядок classpath значим.
    let mut libraries = parent.libraries;
    libraries.extend(child.libraries);
    libraries = dedup_libraries(libraries);

    let mut game_args = parent_args.game;
    game_args.extend(child_args.game);
    let mut jvm_args = parent_args.jvm;
    jvm_args.extend(child_args.jvm);

    Ok(MergedVersion {
        id: child.id,
        vanilla_id: parent_id,
        main_class: child.main_class,
        game_args,
        jvm_args,
        libraries,
        asset_index: parent
            .asset_index
            .context("В родительской (ванильной) версии нет assetIndex")?,
        assets: parent.assets.context("В родительской версии нет поля assets")?,
    })
}

/// Ключ совпадения библиотек — "группа:артефакт" (+ классификатор, если он
/// есть). Версия в ключ не входит: разные версии одного артефакта — это как
/// раз тот случай, ради которого дедупликация и нужна.
fn library_key(name: &str) -> String {
    let parts: Vec<&str> = name.split(':').collect();
    match parts.as_slice() {
        [group, artifact, _version, classifier, ..] => format!("{group}:{artifact}:{classifier}"),
        [group, artifact, ..] => format!("{group}:{artifact}"),
        _ => name.to_string(),
    }
}

/// Оставляет по одной библиотеке на ключ: побеждает последняя (NeoForge),
/// но встаёт на позицию первой, чтобы не менять порядок classpath.
fn dedup_libraries(libraries: Vec<RawLibrary>) -> Vec<RawLibrary> {
    let mut position: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut result: Vec<RawLibrary> = Vec::with_capacity(libraries.len());

    for lib in libraries {
        let key = library_key(&lib.name);
        match position.get(&key) {
            Some(&index) => result[index] = lib,
            None => {
                position.insert(key, result.len());
                result.push(lib);
            }
        }
    }
    result
}

fn read_version_json(game_dir: &Path, id: &str) -> Result<RawVersion> {
    let path = game_dir.join("versions").join(id).join(format!("{id}.json"));
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("Не удалось прочитать {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("Не удалось разобрать {}", path.display()))
}

/// Стандартный алгоритм Mojang: идём по правилам по порядку, каждое
/// подходящее правило перезаписывает результат; последнее совпадение решает.
/// Правила с `features` у нас никогда не совпадают — лаунчер не включает
/// демо-режим, кастомное разрешение экрана и quick-play.
pub fn rules_allow(rules: &Option<Vec<Rule>>) -> bool {
    let Some(rules) = rules else { return true };
    let mut result = false;
    for rule in rules {
        if rule_matches(rule) {
            result = rule.action == "allow";
        }
    }
    result
}

fn rule_matches(rule: &Rule) -> bool {
    if rule.features.is_some() {
        return false;
    }
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != "windows" {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            // Целимся только в x64 — 32-битные правила (arch: "x86") нам не подходят.
            if arch == "x86" {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(name: &str) -> RawLibrary {
        RawLibrary { name: name.to_string(), downloads: None, rules: None }
    }

    #[test]
    fn key_ignores_version_but_keeps_classifier() {
        assert_eq!(library_key("com.google.code.gson:gson:2.10.1"), "com.google.code.gson:gson");
        assert_eq!(library_key("org.lwjgl:lwjgl:3.3.3:natives-windows"), "org.lwjgl:lwjgl:natives-windows");
    }

    #[test]
    fn child_version_wins_at_parent_position() {
        // Порядок: ваниль объявляет gson 2.10.1, NeoForge — её же.
        let merged = dedup_libraries(vec![
            lib("com.google.code.gson:gson:2.10.1"),
            lib("org.slf4j:slf4j-api:2.0.9"),
            lib("com.google.code.gson:gson:2.11.0"),
        ]);
        let names: Vec<&str> = merged.iter().map(|l| l.name.as_str()).collect();
        // Дубликата нет, победила версия ребёнка, позиция — исходная.
        assert_eq!(names, vec!["com.google.code.gson:gson:2.11.0", "org.slf4j:slf4j-api:2.0.9"]);
    }

    #[test]
    fn different_classifiers_are_kept_apart() {
        let merged = dedup_libraries(vec![
            lib("org.lwjgl:lwjgl:3.3.3"),
            lib("org.lwjgl:lwjgl:3.3.3:natives-windows"),
        ]);
        assert_eq!(merged.len(), 2, "натив и основной jar — разные артефакты");
    }
}

/// Разворачивает StringOrList в отдельные аргументы командной строки.
pub fn expand_args(entries: &[ArgEntry]) -> Vec<String> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            ArgEntry::Plain(s) => out.push(s.clone()),
            ArgEntry::Conditional { rules, value } => {
                if rules_allow(&Some(rules.clone())) {
                    match value {
                        StringOrList::Single(s) => out.push(s.clone()),
                        StringOrList::Multiple(items) => out.extend(items.clone()),
                    }
                }
            }
        }
    }
    out
}
