//! String-level element editing for small property XML parts (OOXML core/app, ODF meta).
//! Deliberately conservative: refuses to insert elements whose namespace prefix is undeclared.
use metadata_core::{MetaError, Result};

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn find_element(xml: &str, q: &str) -> Option<(usize, usize)> {
    let open = format!("<{q}");
    let mut from = 0;
    while let Some(p) = xml[from..].find(&open) {
        let s = from + p;
        let after = xml[s + open.len()..].chars().next()?;
        if !matches!(after, '>' | ' ' | '/' | '\t' | '\n' | '\r') {
            from = s + open.len();
            continue;
        }
        let gt = xml[s..].find('>')? + s;
        if xml[..gt].ends_with('/') {
            return Some((s, gt + 1));
        }
        let close = format!("</{q}>");
        let e = xml[gt..].find(&close)? + gt + close.len();
        return Some((s, e));
    }
    None
}

pub fn set_element(xml: &str, q: &str, value: &str, attrs: &str) -> Result<String> {
    let el = format!("<{q}{attrs}>{}</{q}>", escape(value));
    if let Some((s, e)) = find_element(xml, q) {
        return Ok(format!("{}{el}{}", &xml[..s], &xml[e..]));
    }
    if let Some((prefix, _)) = q.split_once(':') {
        if !xml.contains(&format!("xmlns:{prefix}=")) {
            return Err(MetaError::Unsupported(format!(
                "namespace prefix '{prefix}' is not declared in this part; cannot add <{q}> safely"
            )));
        }
    }
    if attrs.contains("xsi:") && !xml.contains("xmlns:xsi=") {
        return Err(MetaError::Unsupported(
            "xsi namespace not declared; cannot add a typed date element safely".into(),
        ));
    }
    let close = xml
        .rfind("</")
        .ok_or_else(|| MetaError::Other("malformed property XML".into()))?;
    Ok(format!("{}{el}{}", &xml[..close], &xml[close..]))
}

pub fn remove_element(xml: &str, q: &str) -> (String, bool) {
    let mut out = xml.to_string();
    let mut any = false;
    while let Some((s, e)) = find_element(&out, q) {
        out.replace_range(s..e, "");
        any = true;
    }
    (out, any)
}

#[cfg(test)]
mod tests {
    use super::*;
    const X: &str = r#"<cp:coreProperties xmlns:cp="c" xmlns:dc="d"><dc:title>Old</dc:title><dc:titleX>keep</dc:titleX><cp:revision/></cp:coreProperties>"#;
    #[test]
    fn replace_insert_remove() {
        let r = set_element(X, "dc:title", "A & B", "").unwrap();
        assert!(r.contains("<dc:title>A &amp; B</dc:title>") && r.contains("<dc:titleX>keep"));
        let r = set_element(X, "dc:creator", "Zed", "").unwrap();
        assert!(r.ends_with("<dc:creator>Zed</dc:creator></cp:coreProperties>"));
        assert!(set_element(X, "dcterms:created", "x", "").is_err());
        let (r, hit) = remove_element(X, "cp:revision");
        assert!(hit && !r.contains("revision"));
        let (_, hit) = remove_element(X, "dc:nothing");
        assert!(!hit);
    }
}
