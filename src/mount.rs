use std::{fs, path::PathBuf};

use eyre::{Context, OptionExt, eyre};

use crate::state::{ExtraMount, Project};

fn resolve(proj_path: PathBuf) -> eyre::Result<(PathBuf, Project)> {
    let root = fs::canonicalize(&proj_path)?;
    crate::state::get_project_above(&root)?.ok_or_eyre("no project found")
}

pub(crate) fn list(proj_path: PathBuf) -> eyre::Result<()> {
    let (_, proj) = resolve(proj_path)?;
    for (dest, mount) in &proj.extra_mounts {
        let ro = if mount.ro { " (ro)" } else { "" };
        println!("{} -> {}{}", mount.src, dest, ro);
    }
    Ok(())
}

pub(crate) fn add(
    proj_path: PathBuf,
    src: PathBuf,
    dest: Option<PathBuf>,
    ro: bool,
) -> eyre::Result<()> {
    let (root, mut proj) = resolve(proj_path)?;

    let src = fs::canonicalize(&src).with_context(|| format!("canonicalize {}", src.display()))?;
    let dest = match dest {
        Some(dest) => {
            if !dest.is_absolute() {
                return Err(eyre!("--dest must be absolute: {}", dest.display()));
            }
            dest
        }
        None => src.clone(),
    };
    let dest_str = dest.to_str().ok_or_eyre("non-UTF-8 dest")?.to_owned();
    if dest == root {
        return Err(eyre!("destination is the project root: {}", dest.display()));
    }
    if proj.extra_mounts.contains_key(&dest_str) {
        return Err(eyre!("mount already exists at {}", dest.display()));
    }

    proj.extra_mounts.insert(
        dest_str.clone(),
        ExtraMount {
            src: src.to_str().ok_or_eyre("non-UTF-8 src")?.to_owned(),
            ro,
        },
    );

    crate::state::update_project(&root, proj)?;

    crate::stop::stop(Some(&root))?;

    println!(
        "Mounted {} -> {}{}",
        src.display(),
        dest_str,
        if ro { " (ro)" } else { "" }
    );
    Ok(())
}

pub(crate) fn umount(proj_path: PathBuf, dest: Option<PathBuf>) -> eyre::Result<()> {
    let (root, mut proj) = resolve(proj_path)?;

    let dest_str = if let Some(dest) = dest {
        dest.to_str().ok_or_eyre("non-UTF-8 dest")?.to_owned()
    } else {
        crate::pick_interactive(
            "Mount to unmount:",
            None,
            proj.extra_mounts.iter().map(|(dest, mount)| {
                let ro = if mount.ro { " (ro)" } else { "" };
                (format!("{} -> {}{}", mount.src, dest, ro), dest.clone())
            }),
        )
        .ok_or_eyre("No mounts")?
    };

    proj.extra_mounts
        .remove(&dest_str)
        .ok_or_eyre(format!("no such mount: {dest_str}"))?;
    crate::state::update_project(&root, proj)?;

    crate::stop::stop(Some(&root))?;

    println!("Unmounted {}", dest_str);
    Ok(())
}
