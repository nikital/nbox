use std::{fs, path::PathBuf};

use eyre::OptionExt;

pub(crate) fn list(path: Option<PathBuf>) -> eyre::Result<()> {
    if let Some(path) = path {
        let root = fs::canonicalize(&path)?;
        let (_, proj) = crate::state::get_project_above(&root)?.ok_or_eyre("no project found")?;

        println!("{}", serde_json::to_string_pretty(&proj)?);
    } else {
        for (root, _) in crate::state::get_projects()? {
            println!("{}", root.display());
        }
    }
    Ok(())
}
