//! Canonical English + optional locale display (product §1.2.5 / FR-UI-48).

use super::DomainError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocaleCopy {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub description: String,
}

pub type I18nMap = BTreeMap<String, LocaleCopy>;

fn normalize_tag_raw(raw: &str) -> Result<String, DomainError> {
    let t = raw.trim().replace('_', "-");
    if t.is_empty() {
        return Ok(String::new());
    }
    let mut parts = t.split('-').filter(|p| !p.is_empty());
    let lang = parts.next().unwrap_or("").to_ascii_lowercase();
    if lang.len() < 2 || lang.len() > 8 || !lang.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(DomainError::Invalid("locale"));
    }
    let mut out = lang;
    for p in parts.take(2) {
        if p.len() > 8 || !p.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(DomainError::Invalid("locale"));
        }
        out.push('-');
        if p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()) {
            out.push_str(&p.to_ascii_uppercase());
        } else {
            let mut chars = p.chars();
            if let Some(f) = chars.next() {
                out.push(f.to_ascii_uppercase());
                out.extend(chars.map(|c| c.to_ascii_lowercase()));
            }
        }
    }
    Ok(out)
}

/// Optional i18n key — never `en` (canonical columns are English).
pub fn normalize_locale(raw: &str) -> Result<String, DomainError> {
    let out = normalize_tag_raw(raw)?;
    if out.is_empty() {
        return Ok(out);
    }
    if out.eq_ignore_ascii_case("en") || out.to_ascii_lowercase().starts_with("en-") {
        return Err(DomainError::Invalid("locale en is canonical, not i18n"));
    }
    Ok(out)
}

/// Author draft locale — `en` allowed; empty → `en`.
pub fn normalize_source_locale(raw: &str) -> Result<String, DomainError> {
    let out = normalize_tag_raw(raw)?;
    if out.is_empty() {
        return Ok("en".into());
    }
    if out.eq_ignore_ascii_case("en") || out.to_ascii_lowercase().starts_with("en-") {
        return Ok("en".into());
    }
    Ok(out)
}

pub fn normalize_i18n(raw: I18nMap) -> Result<I18nMap, DomainError> {
    let mut out = I18nMap::new();
    for (k, v) in raw {
        let locale = normalize_locale(&k)?;
        if locale.is_empty() {
            continue;
        }
        let title = v.title.trim().to_string();
        let event = v.event.trim().to_string();
        let description = v.description.trim().to_string();
        if title.is_empty() && event.is_empty() && description.is_empty() {
            continue;
        }
        if title.chars().count() > 120 || event.chars().count() > 160 || description.chars().count() > 2000 {
            return Err(DomainError::Invalid("i18n field too long"));
        }
        out.insert(
            locale,
            LocaleCopy {
                title,
                event,
                description,
            },
        );
        if out.len() > 16 {
            return Err(DomainError::Invalid("too many locales"));
        }
    }
    Ok(out)
}

/// Parse `Accept-Language` into preferred tags, most preferred first.
pub fn accept_language_chain(header: &str) -> Vec<String> {
    let mut scored: Vec<(f32, String)> = Vec::new();
    for part in header.split(',') {
        let mut it = part.trim().split(';');
        let tag = it.next().unwrap_or("").trim();
        if tag.is_empty() || tag == "*" {
            continue;
        }
        let mut q = 1.0f32;
        for p in it {
            let p = p.trim();
            if let Some(v) = p.strip_prefix("q=").or_else(|| p.strip_prefix("Q=")) {
                if let Ok(n) = v.trim().parse::<f32>() {
                    q = n;
                }
            }
        }
        let norm = tag.replace('_', "-");
        scored.push((q, norm));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::new();
    for (_, tag) in scored {
        let lower = tag.to_ascii_lowercase();
        if !out.iter().any(|x: &String| x.eq_ignore_ascii_case(&lower)) {
            out.push(tag.clone());
        }
        // parent: zh-Hans → zh
        if let Some((parent, _)) = tag.split_once('-') {
            let p = parent.to_string();
            if !out.iter().any(|x: &String| x.eq_ignore_ascii_case(&p)) {
                out.push(p);
            }
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct DisplayListing {
    pub title: String,
    pub event: String,
    pub description: String,
    pub locale: String,
    pub is_translation: bool,
}

/// Exact tag, or parent/child (`zh` ↔ `zh-Hans`). Never `zh-Hans` ↔ `zh-Hant`.
fn locale_key_matches(key: &str, pref: &str) -> bool {
    if key.eq_ignore_ascii_case(pref) {
        return true;
    }
    let key_l = key.to_ascii_lowercase();
    let pref_l = pref.to_ascii_lowercase();
    key_l.starts_with(&format!("{pref_l}-")) || pref_l.starts_with(&format!("{key_l}-"))
}

fn row_from_i18n<'a>(i18n: &'a I18nMap, pref: &str) -> Option<(&'a String, &'a LocaleCopy)> {
    // Exact first so zh-Hant is not stolen by zh-Hans via BTree order.
    if let Some((k, v)) = i18n.iter().find(|(k, _)| k.eq_ignore_ascii_case(pref)) {
        return Some((k, v));
    }
    i18n.iter().find(|(k, _)| locale_key_matches(k, pref))
}

pub fn pick_display(
    canonical_title: &str,
    canonical_event: &str,
    canonical_description: &str,
    i18n: &I18nMap,
    preferred: &[String],
) -> DisplayListing {
    for pref in preferred {
        if pref.eq_ignore_ascii_case("en") || pref.to_ascii_lowercase().starts_with("en-") {
            break;
        }
        if let Some((matched_key, row)) = row_from_i18n(i18n, pref) {
            let title = if row.title.is_empty() {
                canonical_title.to_string()
            } else {
                row.title.clone()
            };
            let event = if row.event.is_empty() {
                canonical_event.to_string()
            } else {
                row.event.clone()
            };
            let description = if row.description.is_empty() {
                canonical_description.to_string()
            } else {
                row.description.clone()
            };
            return DisplayListing {
                title,
                event,
                description,
                locale: matched_key.clone(),
                is_translation: true,
            };
        }
    }
    DisplayListing {
        title: canonical_title.to_string(),
        event: canonical_event.to_string(),
        description: canonical_description.to_string(),
        locale: "en".into(),
        is_translation: false,
    }
}

pub fn i18n_to_json(map: &I18nMap) -> String {
    serde_json::to_string(map).unwrap_or_else(|_| "{}".into())
}

pub fn i18n_from_json(raw: &str) -> I18nMap {
    if raw.trim().is_empty() {
        return I18nMap::new();
    }
    serde_json::from_str::<I18nMap>(raw)
        .ok()
        .and_then(|m| normalize_i18n(m).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_rejects_en() {
        assert!(normalize_locale("en").is_err());
        assert!(normalize_locale("en-US").is_err());
    }

    #[test]
    fn locale_normalizes() {
        assert_eq!(normalize_locale("zh-hans").unwrap(), "zh-Hans");
        assert_eq!(normalize_locale("pt_br").unwrap(), "pt-BR");
    }

    #[test]
    fn pick_falls_back_to_english() {
        let mut map = I18nMap::new();
        map.insert(
            "zh-Hans".into(),
            LocaleCopy {
                title: "美国 CPI".into(),
                event: String::new(),
                description: "首次官方打印".into(),
            },
        );
        let d = pick_display("US CPI", "print", "desc", &map, &["ja".into()]);
        assert!(!d.is_translation);
        assert_eq!(d.title, "US CPI");
        let d2 = pick_display("US CPI", "print", "desc", &map, &["zh-Hans".into(), "zh".into()]);
        assert!(d2.is_translation);
        assert_eq!(d2.title, "美国 CPI");
        assert_eq!(d2.event, "print");
    }

    #[test]
    fn pick_prefers_exact_script_over_sibling() {
        let mut map = I18nMap::new();
        map.insert(
            "zh-Hans".into(),
            LocaleCopy {
                title: "简体".into(),
                event: String::new(),
                description: String::new(),
            },
        );
        map.insert(
            "zh-Hant".into(),
            LocaleCopy {
                title: "繁體".into(),
                event: String::new(),
                description: String::new(),
            },
        );
        let d = pick_display("EN", "e", "d", &map, &["zh-Hant".into(), "zh".into()]);
        assert_eq!(d.title, "繁體");
        assert_eq!(d.locale, "zh-Hant");
        // Parent `zh` may fall back to either child; exact script must win first.
        let d2 = pick_display("EN", "e", "d", &map, &["zh-Hans".into()]);
        assert_eq!(d2.title, "简体");
    }

    #[test]
    fn accept_language_parents() {
        let chain = accept_language_chain("zh-CN,zh;q=0.9,en;q=0.8");
        assert!(chain.iter().any(|x| x.eq_ignore_ascii_case("zh-CN") || x.eq_ignore_ascii_case("zh-cn")));
        assert!(chain.iter().any(|x| x.eq_ignore_ascii_case("zh")));
    }
}
