use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const DOCS_URL: &str = "https://opencode.ai/v2/docs/console/models";
pub const DOCS_ANCHOR_URL: &str = "https://opencode.ai/v2/docs/console/models#free-models";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FreeModel {
    pub id: String,
    pub name: String,
    pub note: String,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct DiffResult {
    pub added: Vec<FreeModel>,
    pub removed: Vec<FreeModel>,
    pub updated: Vec<FreeModel>,
}

fn decode_entities(s: &str) -> String {
    let mut out = s.to_string();
    for (k, v) in [
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&#x27;", "'"),
        ("&apos;", "'"),
        ("&nbsp;", " "),
    ] {
        out = out.replace(k, v);
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth: usize = 0;
    for ch in s.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn cell(s: &str) -> String {
    strip_tags(&decode_entities(s)).trim().to_string()
}

fn for_each_tr<F: FnMut(&str)>(html: &str, mut f: F) {
    let mut rest = html;
    while let Some(start) = rest.find("<tr") {
        let after = &rest[start..];
        let end_rel = after.find("</tr>").map(|e| e + 5).unwrap_or(after.len());
        f(&after[..end_rel]);
        rest = &rest[start + end_rel..];
    }
}

fn for_each_td(tr: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut rest = tr;
    while let Some(start) = rest.find("<td") {
        let after = &rest[start..];
        let end_rel = after.find("</td>").map(|e| e + 5).unwrap_or(after.len());
        cells.push(cell(&after[..end_rel]));
        rest = &rest[start + end_rel..];
    }
    cells
}

fn is_model_id(s: &str) -> bool {
    let b: Vec<char> = s.chars().collect();
    !b.is_empty() && b.len() <= 60 && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-' || c == '_')
}

/// Parse the docs page into name->id mapping (from the endpoints table) and free rows.
fn parse_tables(html: &str) -> (BTreeMap<String, String>, Vec<(String, String)>) {
    let mut name_to_id = BTreeMap::new();
    let mut free_rows: Vec<(String, String)> = Vec::new();
    for_each_tr(html, |tr| {
        let cells = for_each_td(tr);
        if cells.len() >= 2 {
            let is_id = is_model_id(&cells[1]);
            if is_id && !cells[0].is_empty() {
                name_to_id.insert(cells[0].clone(), cells[1].clone());
            }
        }
        if cells.len() >= 3 && cells[1] == "Free" && cells[2] == "Free" && !cells[0].is_empty() {
            free_rows.push((cells[0].clone(), cells.clone().join("|")));
        }
    });
    (name_to_id, free_rows)
}

/// Extract per-model notes from the `#free-models` bullet list.
fn parse_notes(html: &str) -> Vec<String> {
    let start = match html.find("id=\"free-models\"") {
        Some(mut i) => {
            i += 16;
            i
        }
        None => return Vec::new(),
    };
    let mut end = html.len();
    for pat in ["id=\"auto-recharge-and-budgets\"", "<h2", "<h3"] {
        if let Some(o) = html[start..].find(pat) {
            end = end.min(start + o);
        }
    }
    let section = &html[start..end];
    let mut notes = Vec::new();
    let mut rest = section;
    while let Some(start) = rest.find("<li") {
        let after = &rest[start..];
        let end_rel = after.find("</li>").map(|e| e + 5).unwrap_or(after.len());
        notes.push(strip_tags(&decode_entities(&after[..end_rel])).trim().to_string());
        rest = &rest[start + end_rel..];
    }
    notes
}

pub fn parse_free_models(html: &str) -> Option<Vec<FreeModel>> {
    let (name_to_id, free_rows) = parse_tables(html);
    if free_rows.is_empty() {
        return None; // page structure changed or fetch failed: do not treat as "everything removed"
    }
    let mut names: Vec<&String> = name_to_id.keys().collect();
    names.sort_by(|a, b| b.len().cmp(&a.len()));
    let notes = parse_notes(html);

    let mut models = Vec::new();
    for (name, _) in free_rows {
        let id = name_to_id
            .get(&name)
            .cloned()
            .unwrap_or_else(|| name.to_lowercase().replace(' ', "-"));
        let note = notes
            .iter()
            .find(|n| n.starts_with(&name))
            .cloned()
            .unwrap_or_default();
        models.push(FreeModel { id, name, note });
    }
    models.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Some(models)
}

pub fn diff_models(old: &[FreeModel], new: &[FreeModel]) -> DiffResult {
    let old_map: BTreeMap<&str, &FreeModel> = old.iter().map(|m| (m.id.as_str(), m)).collect();
    let new_map: BTreeMap<&str, &FreeModel> = new.iter().map(|m| (m.id.as_str(), m)).collect();

    let mut res = DiffResult::default();
    for m in new {
        if !old_map.contains_key(m.id.as_str()) {
            res.added.push(m.clone());
        } else {
            let o = old_map[m.id.as_str()];
            if o.note != m.note || o.name != m.name {
                res.updated.push(m.clone());
            }
        }
    }
    for m in old {
        if !new_map.contains_key(m.id.as_str()) {
            res.removed.push(m.clone());
        }
    }
    res
}

pub async fn fetch_html(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let resp = client
        .get(url)
        .header("user-agent", "opencode-free-watcher/0.1")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("network error: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.text().await.map_err(|e| format!("read error: {e}"))
}

pub async fn fetch_free_models() -> Result<Vec<FreeModel>, String> {
    let client = reqwest::Client::builder().build().map_err(|e| e.to_string())?;
    let html = fetch_html(&client, DOCS_URL).await?;
    parse_free_models(&html).ok_or_else(|| "parse failed: page structure changed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_live_docs_page() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let html = rt.block_on(async { fetch_html(&reqwest::Client::new(), DOCS_URL).await })
            .expect("fetch should succeed");
        let models = parse_free_models(&html).expect("parse should succeed");
        assert!(models.len() >= 5, "free rows found = {}", models.len());
        let big_pickle = models.iter().find(|m| m.id == "big-pickle").expect("big-pickle must be detected (no -free suffix)");
        assert_eq!(big_pickle.name, "Big Pickle");
        assert!(!big_pickle.note.is_empty(), "note should come from bullets");
        let jev_free = models.iter().find(|m| m.id == "jev-1.13-free").expect("jev-1.13-free present");
        let _ = jev_free;
        assert!(!models.iter().any(|m| m.id == "jev-1.13"), "paid-input jev must not be free");
        println!("parsed {} free models", models.len());
        for m in &models {
            println!("  {} | {} | note={}chars", m.id, m.name, m.note.len());
        }
    }

    #[test]
    fn diff_added_removed_updated() {
        let m1 = FreeModel { id: "a".into(), name: "A".into(), note: "note1".into() };
        let m2 = FreeModel { id: "b".into(), name: "B".into(), note: "note2".into() };
        let m3 = FreeModel { id: "c".into(), name: "C".into(), note: "note3".into() };
        let old = vec![m1.clone(), m3.clone()];
        let new = vec![m1.clone(), m2.clone(), FreeModel { note: "changed".into(), ..m3 }];
        let d = diff_models(&old, &new);
        assert_eq!(d.added.len(), 1); // b
        assert!(d.added.iter().all(|m| m.id == "b"));
        assert!(!d.updated.is_empty() && d.updated[0].id == "c");
        assert!(d.removed.is_empty());
    }
}
