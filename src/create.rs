use std::{collections::BTreeSet, fs, path::PathBuf, process::Command};

use eyre::{Context, OptionExt, eyre};

fn pick_image(name: Option<&str>) -> eyre::Result<String> {
    let images = crate::sh::output(["podman", "images", "--format", "{{.Repository}}:{{.Tag}}"])?;

    let chosen = crate::pick_interactive(
        "Choose image:",
        name,
        images.lines().filter_map(|full| {
            let short = full.strip_suffix(":nbox")?;
            let short = short.strip_prefix("localhost/")?;
            Some((short.to_string(), full.to_string()))
        }),
    )
    .ok_or_eyre("No image selected")?;

    Ok(chosen)
}

pub(crate) fn create(path: PathBuf, podman: bool, image: Option<String>) -> eyre::Result<()> {
    let path = fs::canonicalize(&path)?;
    let name = crate::container_name(&path);
    let path_str = path.to_str().ok_or_eyre("non-UTF-8 path")?;

    if let Some((existing, _)) = crate::state::get_project_above(&path)? {
        return Err(if existing == path {
            eyre!("Project already exists at {}", path.display())
        } else {
            eyre!("Path is inside existing project: {}", existing.display())
        });
    }

    for (proj_path, _) in crate::state::get_projects()? {
        if proj_path.starts_with(&path) {
            return Err(eyre!(
                "Existing project is under this path: {}",
                proj_path.display()
            ));
        }
    }

    let image_tag = pick_image(image.as_deref())?;

    let mut command = Command::new("podman");
    command.args(["create"]);
    command.args(["--name", &name]);
    command.args(["--init"]);
    command.args(["--userns", "keep-id"]);
    command.args(["--security-opt", "label=disable"]);
    command.args(["--network", "pasta:-t,auto,-u,auto,-T,auto,-U,auto"]);
    command.args(["--tmpfs", "/tmp"]);
    command.args(["--tmpfs", "/run"]);

    // BTreeSet for dedup / deterministic order
    let mut option: BTreeSet<(&str, &str)> = BTreeSet::new();
    if podman {
        option.insert(("--device", "/dev/fuse"));
        option.insert(("--device", "/dev/net/tun"));
        option.insert(("--security-opt", "unmask=ALL"));
        option.insert(("--security-opt", "seccomp=unconfined"));
    }
    for (option, value) in &option {
        command.args([option, value]);
    }

    command.args([&image_tag]);
    command.args(["sleep", "infinity"]);

    let status = command.status().context("failed to spawn podman create")?;
    if !status.success() {
        return Err(eyre!(
            "podman run failed with exit code {:?}",
            status.code()
        ));
    }

    crate::state::insert_project(
        &path,
        crate::state::Project {
            container_name: name.clone(),
            extra_mounts: Default::default(),
            mount_cache: None,
        },
    )?;

    println!("Created {path_str} -> {name}");
    Ok(())
}
