//! Extension tag records come from declarations beside engine readers, not a shared list.
use mod_surface::ExtensionTag;
use std::{collections::BTreeSet, fs, path::Path};

pub fn read(root: &Path) -> Result<Vec<ExtensionTag>, String> {
    fn visit(dir: &Path, tags: &mut Vec<ExtensionTag>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                visit(&path, tags)?;
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".extension-tags.json"))
            {
                let text =
                    fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let rows: Vec<ExtensionTag> =
                    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
                tags.extend(rows);
            }
        }
        Ok(())
    }
    let mut tags = Vec::new();
    visit(&root.join("crates"), &mut tags)?;
    let mut seen = BTreeSet::new();
    for tag in &tags {
        if !tag.id.starts_with("sunrise:")
            || tag.id.starts_with("sunrise:class/")
            || tag.registry.is_empty()
            || tag.doc.is_empty()
            || !seen.insert((&tag.registry, &tag.id))
        {
            return Err(format!(
                "invalid or duplicate extension tag {}/{}",
                tag.registry, tag.id
            ));
        }
    }
    tags.sort_by(|a, b| (&a.registry, &a.id).cmp(&(&b.registry, &b.id)));
    Ok(tags)
}
