use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    SimplifiedChinese,
    TraditionalChinese,
}

impl Language {
    pub fn from_preference(preference: &str) -> Self {
        match preference {
            "zh-CN" => Self::SimplifiedChinese,
            "zh-TW" => Self::TraditionalChinese,
            "en" => Self::English,
            _ => Self::from_system(),
        }
    }

    pub fn from_system() -> Self {
        Self::from_environment(|key| std::env::var(key).ok())
    }

    pub fn from_environment(mut get: impl FnMut(&str) -> Option<String>) -> Self {
        let locale = ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
            .into_iter()
            .find_map(|key| get(key).filter(|value| !value.trim().is_empty()))
            .unwrap_or_default();
        Self::from_locale(&locale)
    }

    pub fn from_locale(locale: &str) -> Self {
        let locale = locale
            .split(':')
            .next()
            .unwrap_or_default()
            .replace('-', "_")
            .to_ascii_lowercase();
        if locale.starts_with("zh_tw")
            || locale.starts_with("zh_hk")
            || locale.starts_with("zh_mo")
            || locale.contains("hant")
        {
            Self::TraditionalChinese
        } else if locale.starts_with("zh") {
            Self::SimplifiedChinese
        } else {
            Self::English
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::English => 0,
            Self::SimplifiedChinese => 1,
            Self::TraditionalChinese => 2,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::SimplifiedChinese,
            2 => Self::TraditionalChinese,
            _ => Self::English,
        }
    }
}

thread_local! {
    static CURRENT: Cell<u8> = const { Cell::new(0) };
}

pub fn set_current(language: Language) {
    CURRENT.set(language.code());
}

pub fn current() -> Language {
    Language::from_code(CURRENT.get())
}

// Only application-owned text goes through this catalog. User configuration,
// node names, URLs, and core logs are rendered unchanged.
fn catalog() -> &'static std::collections::BTreeMap<String, [String; 2]> {
    static CATALOG: std::sync::OnceLock<std::collections::BTreeMap<String, [String; 2]>> =
        std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("translations.json")).expect("valid bundled translations")
    })
}

pub fn translate(source: &str, language: Language) -> String {
    if language == Language::SimplifiedChinese {
        return source.into();
    }
    let column = usize::from(language == Language::TraditionalChinese);
    let catalog = catalog();
    if let Some(entry) = catalog.get(source) {
        return entry[column].clone();
    }
    for (template, translations) in catalog {
        if template.contains('{') {
            if let Some(values) = capture_template(template, source) {
                if template.contains("{error}")
                    || template.contains("{message}")
                    || template.contains("{e}")
                {
                    let translated: Vec<String> = values
                        .iter()
                        .map(|value| {
                            if *value == source {
                                value.to_string()
                            } else {
                                translate(value, language)
                            }
                        })
                        .collect();
                    return fill_template(
                        &translations[column],
                        &translated.iter().map(String::as_str).collect::<Vec<_>>(),
                    );
                }
                return fill_template(&translations[column], &values);
            }
        }
    }
    // Composite UI lines contain punctuation and values around trusted labels.
    // Match each original byte once, longest phrase first; never retranslate output.
    static PHRASES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    let phrases = PHRASES.get_or_init(|| {
        let mut keys: Vec<_> = catalog
            .keys()
            .map(String::as_str)
            .filter(|s| s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)))
            .collect();
        keys.sort_by_key(|key| std::cmp::Reverse(key.len()));
        keys
    });
    let mut result = String::new();
    let mut remaining = source;
    while !remaining.is_empty() {
        if let Some(key) = phrases.iter().find(|key| remaining.starts_with(**key)) {
            result.push_str(&catalog[*key][column]);
            remaining = &remaining[key.len()..];
        } else {
            let c = remaining.chars().next().unwrap();
            result.push(c);
            remaining = &remaining[c.len_utf8()..];
        }
    }
    result
}

pub fn tr(source: &str) -> String {
    translate(source, current())
}

pub fn choice_label(value: &str) -> String {
    match value {
        "auto" => match current() {
            Language::English => "Follow system",
            Language::SimplifiedChinese => "跟随系统",
            Language::TraditionalChinese => "跟隨系統",
        }
        .into(),
        "en" => "English".into(),
        "zh-CN" => "简体中文".into(),
        "zh-TW" => "繁體中文".into(),
        _ => tr(value),
    }
}

pub struct LanguageGuard(Language);
impl Drop for LanguageGuard {
    fn drop(&mut self) {
        set_current(self.0);
    }
}
pub fn use_language(language: Language) -> LanguageGuard {
    let previous = current();
    set_current(language);
    LanguageGuard(previous)
}

pub fn t(source: &'static str) -> &'static str {
    match current() {
        Language::SimplifiedChinese => source,
        language => catalog()
            .get(source)
            .map(|entry| entry[usize::from(language == Language::TraditionalChinese)].as_str())
            .unwrap_or(source),
    }
}

fn template_parts(template: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}') else {
            break;
        };
        parts.push(&rest[..start]);
        rest = &rest[start + end + 1..];
    }
    parts.push(rest);
    parts
}
fn capture_template<'a>(template: &str, source: &'a str) -> Option<Vec<&'a str>> {
    let parts = template_parts(template);
    if parts.len() < 2 {
        return None;
    }
    let mut rest = source.strip_prefix(parts[0])?;
    let mut values = Vec::new();
    for (i, suffix) in parts.iter().enumerate().skip(1) {
        if i == parts.len() - 1 {
            values.push(rest.strip_suffix(suffix)?);
            return Some(values);
        }
        if suffix.is_empty() {
            return None;
        }
        let index = rest.find(suffix)?;
        values.push(&rest[..index]);
        rest = &rest[index + suffix.len()..];
    }
    None
}
fn fill_template(template: &str, values: &[&str]) -> String {
    let parts = template_parts(template);
    let mut result = parts[0].to_string();
    for (value, suffix) in values.iter().zip(parts.iter().skip(1)) {
        result.push_str(value);
        result.push_str(suffix);
    }
    result
}

pub fn format(source: &str, values: &[String]) -> String {
    let template = if current() == Language::SimplifiedChinese {
        source
    } else {
        catalog()
            .get(source)
            .map(|entry| entry[usize::from(current() == Language::TraditionalChinese)].as_str())
            .unwrap_or(source)
    };
    fill_template(
        template,
        &values.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}
