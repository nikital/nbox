use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    os::unix::fs::{PermissionsExt as _, symlink},
    path::{Path, PathBuf},
};

use eyre::{OptionExt, eyre};

/// If the binary was invoked as `nbox-<cmd>`, return `<cmd>`.
pub(crate) fn command_from_argv0(argv0: &OsStr) -> Option<OsString> {
    let stem = Path::new(argv0).file_stem()?.to_str()?;
    Some(OsString::from(stem.strip_prefix("nbox-")?))
}

pub(crate) fn bin(name: &str) -> eyre::Result<()> {
    let nbox = which_nbox().ok_or_eyre("nbox not found in PATH")?;
    let dir = crate::config_dir()?.join("bin");
    fs::create_dir_all(&dir)?;
    let link = dir.join(format!("nbox-{name}"));

    match fs::read_link(&link) {
        Ok(target) if target == nbox => {
            println!("Link exists.");
            return Ok(());
        }
        Ok(_) => return Err(eyre!("{} exists but points elsewhere?", link.display())),
        Err(_) => {}
    }
    if link.exists() {
        return Err(eyre!("{} exists but is not a symlink", link.display()));
    }
    symlink(&nbox, &link)?;
    println!("{} -> {}", link.display(), nbox.display());
    Ok(())
}

fn which_nbox() -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join("nbox");
        let is_executable = fs::metadata(&candidate)
            .map(|m| m.is_file() && m.permissions().mode() & 0o100 != 0)
            .unwrap_or(false);
        if is_executable {
            return Some(candidate);
        }
    }
    None
}
