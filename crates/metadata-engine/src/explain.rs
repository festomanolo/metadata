//! Human explanations of fields, including reliability caveats (shown in TUI details and docs).
pub fn explain(group: &str, name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    let g = group.to_ascii_lowercase();
    Some(match (g.as_str(), n.as_str()) {
        (_, "datetimeoriginal") => "Camera-recorded capture time. Stored as local time; the timezone is only known when OffsetTimeOriginal exists. Editable with common tools.",
        (_, "datetimedigitized") => "When the image was digitized (scanner/camera). Often equals DateTimeOriginal.",
        ("exif", "datetime") => "Last modification time written by the editing software or camera; not the capture time.",
        (_, "creationtime") | (_, "creation_time") => "Container timestamp written by the recorder/encoder (often UTC). It is not proof of when the scene was recorded.",
        ("ooxml-core", "created") | ("odf-meta", "creationdate") => "Document creation time stored in the file. Copying a template or 'Save As' can carry an older value; the clock of the creating machine is trusted blindly.",
        ("ooxml-core", "modified") | ("odf-meta", "date") => "Last save time stored in the file, from the saving machine's clock.",
        ("ooxml-core", "revision") => "Number of saves recorded by the application. Not an editing duration and not comparable across applications.",
        ("ooxml-app", "totaltime") | ("odf-meta", "editingduration") => "Editing time accumulated by the application (minutes/ISO duration). Application-reported; can be reset or inflated by leaving a document open.",
        ("ooxml-core", "creator") | ("pdf", "author") => "Author string stored in the document. Reflects the application's configured user name at save time, not verified identity.",
        ("ooxml-core", "lastmodifiedby") => "User name configured in the last-saving application. Not verified identity.",
        ("pdf", "producer") | ("pdf", "creator") => "Software that produced/converted the PDF. Re-exporting replaces this.",
        ("pdf", "revisions") => "Count of %%EOF markers: more than one indicates incremental updates appended after the original save.",
        ("filesystem", _) | ("file", "filemodifydate") => "Filesystem timestamps change when files are copied, downloaded, extracted, synced or edited; they are weak evidence on their own.",
        ("gps", _) => "Location metadata. Personal data; redacted from reports by default.",
        ("archive", "entry") => "Timestamp inside the archive (DOS/Unix time, usually no timezone). Set by the archiving tool from the source file's mtime.",
        _ => return None,
    })
}
