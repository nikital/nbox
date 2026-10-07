use std::{fs, path::PathBuf};

use eyre::OptionExt;

pub(crate) fn delete(path: Option<PathBuf>) -> eyre::Result<()> {
    let (root, proj) = if let Some(path) = path {
        crate::state::get_project_above(&fs::canonicalize(&path)?)?
            .ok_or_eyre("no project found")?
    } else {
        let projects = crate::state::get_projects()?;
        crate::pick_interactive(
            "Project to delete:",
            None,
            projects.into_iter().map(|(path, proj)| {
                (
                    format!("{} -> {}", path.display(), proj.container_name),
                    (path, proj),
                )
            }),
        )
        .ok_or_eyre("No registered projects")?
    };

    crate::sh::run(["podman", "rm", "-f", "-t", "0", &proj.container_name])?;
    crate::state::remove_project(&root)?.ok_or_eyre("no project found, again?")?;

    println!("Deleted {} -> {}", root.display(), proj.container_name);
    Ok(())
}
