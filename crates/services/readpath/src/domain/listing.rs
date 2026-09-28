use super::DomainError;

pub const DEFAULT_CATALOG_TAGS: &[&str] = &["football", "epl", "world cup", "macro", "cpi", "price", "election", "binary"];

#[derive(Clone, Debug)]
pub struct CatalogTag {
    pub name: String,
}

impl CatalogTag {
    pub fn new(raw: impl AsRef<str>) -> Result<Self, DomainError> {
        let name = normalize_tag(raw.as_ref())?;
        if name.is_empty() {
            return Err(DomainError::Invalid("tag required"));
        }
        Ok(Self { name })
    }
}

#[derive(Clone, Debug)]
pub struct Listing {
    pub market: String,
    pub title: String,
    pub tags: Vec<String>,
    pub category: String,
    pub topic: String,
    pub tag: String,
    pub description: String,
    pub event: String,
}

pub fn normalize_tag(raw: &str) -> Result<String, DomainError> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(String::new());
    }
    if t.chars().count() > 24 {
        return Err(DomainError::Invalid("tag too long"));
    }
    Ok(t.chars()
        .map(|c| if c.is_ascii_alphabetic() { c.to_ascii_lowercase() } else { c })
        .collect())
}

pub fn normalize_tags<I, S>(raw: I) -> Result<Vec<String>, DomainError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = Vec::new();
    for s in raw {
        let t = normalize_tag(s.as_ref())?;
        if t.is_empty() {
            continue;
        }
        if !out.iter().any(|x: &String| x.eq_ignore_ascii_case(&t)) {
            out.push(t);
        }
        if out.len() > 8 {
            return Err(DomainError::Invalid("too many tags"));
        }
    }
    if out.is_empty() {
        return Err(DomainError::Invalid("at least one tag"));
    }
    Ok(out)
}

pub fn encode_tags(tags: &[String]) -> String {
    tags.join(",")
}

pub fn decode_tags(raw: &str, fallback: &str) -> Vec<String> {
    let parts: Vec<&str> = raw
        .split(|c| c == ',' || c == '，' || c == '/' || c == '|')
        .collect();
    match normalize_tags(parts) {
        Ok(v) => v,
        Err(_) => normalize_tags([fallback]).unwrap_or_default(),
    }
}

impl Listing {
    pub fn new(
        market: impl Into<String>,
        title: impl Into<String>,
        tags: Vec<String>,
        topic: impl Into<String>,
        tag: impl Into<String>,
        description: impl Into<String>,
        event: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let market = market.into();
        let title = title.into();
        let event = event.into().trim().to_string();
        let description = description.into().trim().to_string();
        if market.trim().is_empty() || title.trim().is_empty() {
            return Err(DomainError::Invalid("listing identity required"));
        }
        if event.is_empty() || description.is_empty() {
            return Err(DomainError::Invalid("listing event and description required"));
        }
        let tags = normalize_tags(tags)?;
        let category = tags[0].clone();
        Ok(Self {
            market: market.trim().to_string(),
            title: title.trim().to_string(),
            tags,
            category,
            topic: topic.into().trim().to_string(),
            tag: tag.into().trim().to_string(),
            description,
            event,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_rejects_empty_title() {
        assert!(Listing::new("m", "", vec!["宏观".into()], "", "", "d", "e").is_err());
    }

    #[test]
    fn listing_requires_event_and_description() {
        assert!(Listing::new("m", "阿森纳 vs 切尔西", vec!["足球".into()], "", "", "", "英超").is_err());
        assert!(Listing::new("m", "阿森纳 vs 切尔西", vec!["足球".into()], "", "", "说明", "").is_err());
    }

    #[test]
    fn listing_keeps_cjk_tags() {
        let row = Listing::new(
            "m",
            "阿森纳 vs 切尔西",
            vec!["足球".into(), "英超".into()],
            "",
            "",
            "英超轮次。常规时间终场比分结算。",
            "阿森纳 vs 切尔西",
        )
        .unwrap();
        assert_eq!(row.tags, vec!["足球", "英超"]);
        assert_eq!(row.category, "足球");
        assert_eq!(row.event, "阿森纳 vs 切尔西");
    }

    #[test]
    fn listing_normalizes_ascii_tags() {
        let row = Listing::new(
            "m",
            "US CPI YoY persist check",
            vec!["Macro".into()],
            "",
            "",
            "First official print. Revisions do not settle.",
            "US CPI YoY first print",
        )
        .unwrap();
        assert_eq!(row.tags, vec!["macro"]);
    }

    #[test]
    fn catalog_tag_rejects_empty() {
        assert!(CatalogTag::new("  ").is_err());
        assert_eq!(CatalogTag::new("英超").unwrap().name, "英超");
    }
}
