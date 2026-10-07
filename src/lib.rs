use std::{
    env,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use clap::Subcommand;
use eyre::{OptionExt, eyre};

mod build;
mod create;
mod delete;
mod launch;
mod linux;
mod list;
mod mount;
mod multicall;
mod sh;
mod state;
mod stop;

pub fn nbox(args: impl Iterator<Item = impl AsRef<OsStr>>) -> eyre::Result<()> {
    launch::launch(args)?;
    unreachable!()
}

#[derive(Subcommand)]
pub enum ManageCommand {
    /// Build a sandbox image
    Build {
        /// Image name
        #[arg(long)]
        image: Option<String>,
    },
    /// Create multicall symlink nbox-<name>
    Bin {
        /// Command name
        name: String,
    },
    /// List registered projects
    List {
        /// Project directory to show
        path: Option<PathBuf>,
    },
    /// Register a project and start its container
    Create {
        /// Project directory to register
        path: PathBuf,
        /// Allow podman inside the sandbox
        #[arg(long)]
        podman: bool,
        /// Image name
        #[arg(long)]
        image: Option<String>,
    },
    /// Stop the container
    Stop {
        /// Project directory to stop
        path: Option<PathBuf>,
    },
    /// Stop and remove container, deregister project
    Delete {
        /// Project directory to remove
        path: Option<PathBuf>,
    },
    /// Configure extra mounts
    Mount {
        /// Project directory
        path: PathBuf,
        /// Host path to mount (omit to list current mounts)
        src: Option<PathBuf>,
        /// Destination inside the container (defaults to <mount src>)
        #[arg(long)]
        dest: Option<PathBuf>,
        /// Mount read-only
        #[arg(long)]
        ro: bool,
    },
    /// Remove extra mount
    Umount {
        /// Project directory
        path: PathBuf,
        /// Destination to remove (omit to pick interactively)
        dest: Option<PathBuf>,
    },
}

pub fn manage_nbox(cmd: ManageCommand) -> eyre::Result<()> {
    match cmd {
        ManageCommand::Build { image } => build::build(image),
        ManageCommand::Bin { name } => multicall::bin(&name),
        ManageCommand::List { path } => list::list(path),
        ManageCommand::Create {
            path,
            podman,
            image,
        } => create::create(path, podman, image),
        ManageCommand::Stop { path } => stop::stop(path.as_deref()),
        ManageCommand::Delete { path } => delete::delete(path),
        ManageCommand::Mount {
            path,
            src,
            dest,
            ro,
        } => {
            if let Some(src) = src {
                mount::add(path, src, dest, ro)
            } else {
                if dest.is_some() || ro {
                    return Err(eyre!("--dest and --ro require <mount src>"));
                }
                mount::list(path)
            }
        }
        ManageCommand::Umount { path, dest } => mount::umount(path, dest),
    }
}

fn config_dir() -> eyre::Result<PathBuf> {
    let xdg = env::var_os("XDG_CONFIG_HOME")
        .map(|p| PathBuf::from(p))
        .or_else(|| env::home_dir().map(|p| p.join(".config")))
        .ok_or_eyre("xdg config not found")?;
    let dir = xdg.join("nbox");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn pick_interactive<T>(
    prompt: &str,
    choice: Option<&str>,
    options: impl IntoIterator<Item = (impl AsRef<str>, T)>,
) -> Option<T> {
    // If choice was provided, pick directly.
    if let Some(choice) = choice {
        for (label, item) in options.into_iter() {
            if label.as_ref() == choice {
                return Some(item);
            }
        }
        return None;
    }

    // No choice, so prompt interactively.
    println!("{prompt}");
    let mut items = vec![];
    for (i, (label, item)) in options.into_iter().enumerate() {
        println!("{}) {}", i + 1, label.as_ref());
        items.push(item);
    }
    if items.is_empty() {
        return None;
    }

    let index = loop {
        println!("Pick option (1-{}):", items.len());
        let mut line = String::new();
        io::stdin().read_line(&mut line).ok()?;
        let line = line.trim();
        if let Ok(n) = line.parse::<usize>()
            && n >= 1
            && n <= items.len()
        {
            break n - 1;
        }
    };

    Some(items.swap_remove(index))
}

pub(crate) fn container_name(path: &Path) -> String {
    {
        // sanity - we canonicalized correctly above.
        let canonical = fs::canonicalize(path).expect("canonicalize failed");
        assert_eq!(
            canonical, path,
            "container_name called with non-canonical path"
        );
    }
    "nbox-".to_string()
        + &path
            .to_str()
            .unwrap()
            .trim_start_matches('/')
            .replace('/', "-")
}
