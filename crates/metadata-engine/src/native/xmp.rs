//! Minimal XMP packet reader: flattens rdf:Description attributes and simple property elements.
use quick_xml::events::Event;
use quick_xml::Reader;

pub fn parse(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut r = Reader::from_str(xml);
    let mut stack: Vec<String> = Vec::new();
    let mut text = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "rdf:Description" {
                    for a in e.attributes().flatten() {
                        let k = String::from_utf8_lossy(a.key.as_ref()).to_string();
                        if k.starts_with("xmlns") || k.starts_with("rdf:") || k.starts_with("x:") { continue; }
                        if let Ok(v) = a.unescape_value() { out.push((k, v.to_string())); }
                    }
                }
                stack.push(name);
                text.clear();
            }
            Ok(Event::Text(t)) => {
                if let Ok(s) = t.decode() { text.push_str(&s); }
            }
            Ok(Event::End(_)) => {
                if let Some(name) = stack.pop() {
                    let v = text.trim();
                    let structural = ["rdf:", "x:", "xmpmeta"].iter().any(|p| name.starts_with(p));
                    if !v.is_empty() && !structural && name.contains(':') {
                        // list items (rdf:li) carry values for their parent property
                        out.push((name, v.to_string()));
                    } else if !v.is_empty() && name == "rdf:li" {
                        if let Some(parent) = stack.iter().rev().find(|s| s.contains(':') && !s.starts_with("rdf:")) {
                            out.push((parent.clone(), v.to_string()));
                        }
                    }
                }
                text.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    // Merge repeated properties (list items) into one comma-separated value.
    let mut merged: Vec<(String, String)> = Vec::new();
    for (k, v) in out {
        if let Some(e) = merged.iter_mut().find(|(mk, _)| *mk == k) {
            if !e.1.split(", ").any(|x| x == v) { e.1.push_str(", "); e.1.push_str(&v); }
        } else {
            merged.push((k, v));
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    #[test]
    fn attrs_and_lists() {
        let x = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r"><rdf:Description xmlns:xmp="x" xmp:CreatorTool="Tool 1" xmp:CreateDate="2023-01-01T00:00:00Z"><dc:creator><rdf:Seq><rdf:li>Ann</rdf:li><rdf:li>Bob</rdf:li></rdf:Seq></dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let m = super::parse(x);
        assert!(m.contains(&("xmp:CreatorTool".into(), "Tool 1".into())));
        assert!(m.contains(&("dc:creator".into(), "Ann, Bob".into())));
    }
}
