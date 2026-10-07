use std::{fs, path::Path};

use eyre::OptionExt;

pub(crate) fn stop(path: Option<&Path>) -> eyre::Result<()> {
    let (root, mut proj) = if let Some(path) = path {
        crate::state::get_project_above(&fs::canonicalize(&path)?)?
            .ok_or_eyre("no project found")?
    } else {
        let projects = crate::state::get_projects()?;
        crate::pick_interactive(
            "Project to stop:",
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

    let running = crate::sh::output([
        "podman",
        "inspect",
        "--format",
        "{{.State.Running}}",
        &proj.container_name,
    ])?
    .trim_ascii()
        == "true";
    if running {
        crate::sh::run(["podman", "stop", &proj.container_name])?;
    }

    println!("Stopped {} -> {}", root.display(), proj.container_name);

    proj.mount_cache = None;

    crate::state::update_project(&root, proj)?;
    Ok(())
}
