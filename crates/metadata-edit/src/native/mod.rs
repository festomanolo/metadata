//! Native writers. Each supports a fixed, tested set of keys; anything else is reported unsupported.
pub mod jpeg;
pub mod office;
pub mod pdf;
pub mod png;
pub mod xmlprops;
pub mod zipedit;

use metadata_core::{MetaError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WProfile {
    Privacy,
    All,
    Gps,
}

/// A key already resolved to its canonical `Group:Name`; `value: None` means remove.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub key: String,
    pub value: Option<String>,
}

/// Canonical writable keys for a format (`PNG:*` accepts any valid text-chunk keyword).
pub fn keys(format: &str) -> Vec<String> {
    match format {
        "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" => {
            office::keys(format).into_iter().map(String::from).collect()
        }
        "pdf" => pdf::KEYS.iter().map(|k| format!("PDF:{k}")).collect(),
        "png" => vec!["PNG:<any text keyword>".into()],
        "jpeg" => vec!["File:Comment".into()],
        _ => vec![],
    }
}

/// Map user input (`Group:Name` or bare `Name`, any case) to a canonical key for this format.
pub fn resolve_key(format: &str, input: &str) -> Result<String> {
    if format == "png" {
        let k = input
            .strip_prefix("PNG:")
            .or_else(|| input.strip_prefix("png:"))
            .unwrap_or(input);
        return if png::key_ok(k) {
            Ok(format!("PNG:{k}"))
        } else {
            Err(MetaError::InvalidArgument(format!(
                "'{k}' is not a valid PNG text keyword (1-79 printable ASCII chars)"
            )))
        };
    }
    let all = keys(format);
    if all.is_empty() {
        return Err(MetaError::Unsupported(format!(
            "no native writer for {format}"
        )));
    }
    let lower = input.to_ascii_lowercase();
    let hits: Vec<&String> = all
        .iter()
        .filter(|k| {
            k.to_ascii_lowercase() == lower
                || k.rsplit(':')
                    .next()
                    .is_some_and(|n| n.to_ascii_lowercase() == lower)
        })
        .collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(MetaError::Unsupported(format!(
            "'{input}' is not writable for {format}; writable: {}",
            all.join(", ")
        ))),
        many => Err(MetaError::InvalidArgument(format!(
            "'{input}' is ambiguous: {}",
            many.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

pub fn supports_profile(format: &str, p: WProfile) -> bool {
    match (format, p) {
        (_, WProfile::Gps) => false,
        ("docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "pdf" | "png" | "jpeg", _) => true,
        _ => false,
    }
}

pub fn has_signature(format: &str, data: &[u8]) -> bool {
    match format {
        "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" => office::has_signature(data),
        "pdf" => {
            data.windows(10).any(|w| w == b"/ByteRange") && data.windows(4).any(|w| w == b"/Sig")
        }
        _ => false,
    }
}

pub fn write(
    format: &str,
    data: &[u8],
    ops: &[Resolved],
    profile: Option<WProfile>,
) -> Result<Vec<u8>> {
    match format {
        "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" => {
            office::apply(format, data, ops, profile)
        }
        "pdf" => pdf::apply(data, ops, profile),
        "png" => png::apply(data, ops, profile),
        "jpeg" => jpeg::apply(data, ops, profile),
        _ => Err(MetaError::Unsupported(format!(
            "no native writer for {format}"
        ))),
    }
}
