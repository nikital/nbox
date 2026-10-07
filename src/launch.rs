use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::{CString, OsStr, c_int, c_uint},
    fs::{self, File},
    io::{self, IsTerminal},
    os::{fd::AsRawFd, unix::ffi::OsStrExt, unix::process::CommandExt},
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

use eyre::{Context, OptionExt, eyre};
use libc::pid_t;

use crate::state::{CachedMount, CachedMountType, MountCache, Project};

pub(crate) fn launch(mut args: impl Iterator<Item = impl AsRef<OsStr>>) -> eyre::Result<()> {
    let argv0 = args.next().ok_or_eyre("missing argv[0]")?;
    let cmd = crate::multicall::command_from_argv0(argv0.as_ref());

    let cwd = env::current_dir()?;
    let Some((root, proj)) = crate::state::get_project_above(&cwd)? else {
        return Err(eyre!("No registered project contains {}", cwd.display()));
    };

    let desired = desired_mounts(&root, &proj)?;
    let state = inspect_state(&proj.container_name)?;

    if !state.running {
        if !Command::new("podman")
            .args(["start", &proj.container_name])
            .stdout(Stdio::null())
            .status()?
            .success()
        {
            return Err(eyre!("Failed to start {}", proj.container_name));
        };
        let state = inspect_state(&proj.container_name)?;
        mount_project(&state.pid, &desired)?;
        crate::state::update_project(
            &root,
            Project {
                container_name: proj.container_name.clone(),
                extra_mounts: proj.extra_mounts,
                mount_cache: Some(MountCache {
                    started_at: state.started_at,
                    pid: state.pid,
                    mounts: desired,
                }),
            },
        )?;
    } else {
        let Some(cache) = &proj.mount_cache else {
            return Err(eyre!(
                "container {} is running but has no mount cache?",
                proj.container_name
            ));
        };
        if cache.started_at != state.started_at || cache.pid != state.pid {
            return Err(eyre!(
                "container {} was restarted outside nbox, run `manage-nbox stop`",
                proj.container_name
            ));
        }
        if cache.mounts != desired {
            return Err(eyre!(
                "mounts changed for {}:\n  cached: {:?}\n  desired: {:?}\nrun `manage-nbox stop .` and retry",
                proj.container_name,
                cache.mounts,
                desired
            ));
        }
    }

    let err = Command::new("podman")
        .arg("exec")
        .arg(if io::stdin().is_terminal() {
            "-it"
        } else {
            "-i"
        })
        .args([OsStr::new("-w"), cwd.as_os_str()])
        .arg(&proj.container_name)
        .args(["bash", "-lc", "exec \"$@\"", "--"])
        .args(cmd)
        .args(args)
        .exec();

    Err(err.into())
}

fn desired_mounts(root: &Path, proj: &Project) -> eyre::Result<BTreeMap<String, CachedMount>> {
    let root_str = root.to_str().ok_or_eyre("non-UTF-8 root path")?.to_owned();
    let mut mounts = BTreeMap::new();
    mounts.insert(
        root_str.clone(),
        CachedMount {
            src: root_str,
            ro: false,
            kind: CachedMountType::Dir {
                ro_relative_paths: discover_ro_mounts(root)?,
            },
        },
    );
    for (dest, extra) in &proj.extra_mounts {
        let src = Path::new(&extra.src);
        let kind = if src.is_dir() {
            CachedMountType::Dir {
                ro_relative_paths: discover_ro_mounts(src)?,
            }
        } else {
            CachedMountType::File
        };
        mounts.insert(
            dest.clone(),
            CachedMount {
                src: extra.src.clone(),
                ro: extra.ro,
                kind,
            },
        );
    }
    Ok(mounts)
}

fn discover_ro_mounts(root: &Path) -> eyre::Result<BTreeSet<String>> {
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false);

    let found = Arc::new(Mutex::new(Vec::new()));
    let err = Arc::new(Mutex::new(None));
    builder.build_parallel().run(|| {
        let found = Arc::clone(&found);
        let err = Arc::clone(&err);
        Box::new(move |result| match result {
            Ok(entry) => {
                let is_git = entry.file_type().is_some_and(|t| t.is_dir())
                    && entry.file_name() == OsStr::new(".git");
                if is_git {
                    found.lock().unwrap().push(entry.path().to_path_buf());
                    ignore::WalkState::Skip
                } else {
                    ignore::WalkState::Continue
                }
            }
            Err(e) => {
                *err.lock().unwrap() = Some(e);
                ignore::WalkState::Quit
            }
        })
    });

    if let Some(e) = err.lock().unwrap().take() {
        return Err(e.into());
    }

    let mut rels = BTreeSet::new();
    for abs in found.lock().unwrap().iter() {
        let rel = abs.strip_prefix(root).context("git dir outside root")?;
        rels.insert(rel.to_str().ok_or_eyre("non-UTF-8 .git path")?.to_owned());
    }
    Ok(rels)
}

struct ContainerState {
    running: bool,
    started_at: String,
    pid: String,
}

fn inspect_state(container: &str) -> eyre::Result<ContainerState> {
    let out = crate::sh::output([
        "podman",
        "inspect",
        "--format",
        "{{.State.Running}}\t{{.State.StartedAt}}\t{{.State.Pid}}",
        container,
    ])?;
    let mut fields = out.trim_ascii_end().split('\t');
    let running = fields.next().ok_or_eyre("missing Running")? == "true";
    let started_at = fields.next().ok_or_eyre("missing StartedAt")?.to_owned();
    let pid = fields.next().ok_or_eyre("missing Pid")?.to_owned();
    Ok(ContainerState {
        running,
        started_at,
        pid,
    })
}

fn mount_project(container_pid: &str, mounts: &BTreeMap<String, CachedMount>) -> eyre::Result<()> {
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::last_os_error()).context("fork");
    }
    if pid == 0 {
        let code = if let Err(err) = mount_project_forked(container_pid, mounts) {
            eprintln!("nbox: failed to mount: {err:?}");
            1
        } else {
            0
        };
        unsafe { libc::_exit(code) };
    }

    let mut status = 0;
    if unsafe { libc::waitpid(pid, &mut status, 0) } < 0 {
        return Err(io::Error::last_os_error()).context("waitpid");
    }
    if !libc::WIFEXITED(status) || libc::WEXITSTATUS(status) != 0 {
        return Err(eyre!("failed to mount project into container"));
    }
    Ok(())
}

fn mount_project_forked(
    container_pid: &str,
    mounts: &BTreeMap<String, CachedMount>,
) -> eyre::Result<()> {
    enter_podman_namespace()?;

    // Clone each subtree and mark read-only while we have CAP_SYS_ADMIN in this
    // podman's namespace.
    let clones = mounts
        .iter()
        .map(|(dest, mount)| -> eyre::Result<_> {
            let src = Path::new(&mount.src);
            let mut mounter =
                Mounter::new_subtree(src).with_context(|| format!("clone {}", src.display()))?;
            if mount.ro {
                mounter
                    .make_self_readonly()
                    .with_context(|| format!("make {} read-only", src.display()))?;
            }
            if let CachedMountType::Dir { ro_relative_paths } = &mount.kind {
                for rel in ro_relative_paths {
                    mounter
                        .make_readonly(Path::new(rel))
                        .with_context(|| format!("make {} read-only", src.join(rel).display()))?;
                }
            }

            Ok((Path::new(dest), mount, mounter))
        })
        .collect::<Result<Vec<_>, _>>()?;

    enter_mount_namespace(container_pid)?;
    for (dest, mount, mounter) in clones {
        if let CachedMountType::File = mount.kind {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create mountpoint parent {}", parent.display()))?;
            }
            if !dest.exists() {
                File::create(dest)
                    .with_context(|| format!("create mountpoint file {}", dest.display()))?;
            }
        } else {
            fs::create_dir_all(dest)
                .with_context(|| format!("create mountpoint {}", dest.display()))?;
        }
        mounter
            .move_mount(dest)
            .with_context(|| format!("move {}", dest.display()))?;
    }
    Ok(())
}

/// Enter Podman's pause process NS.
///
/// From man podman-system-migrate.1: Rootless Podman uses a pause process to
/// keep the unprivileged namespaces alive. We need its user namespace to get
/// CAP_SYS_ADMIN over the container mounts, and its mount namespace to see the
/// host paths we want to clone.
fn enter_podman_namespace() -> eyre::Result<()> {
    let runroot = crate::sh::output(["podman", "info", "--format", "{{.Store.RunRoot}}"])?;
    let runtime = Path::new(runroot.trim_ascii_end())
        .parent()
        .ok_or_eyre("podman runroot has no parent")?;
    let pidfile = runtime.join("libpod/tmp/pause.pid");
    let pid =
        fs::read_to_string(&pidfile).with_context(|| format!("read {}", pidfile.display()))?;
    let pid = pid.trim_ascii();
    // Make sure it's parsable as an int to avoid weird paths
    let Ok(_) = pid.parse::<pid_t>() else {
        return Err(eyre!("bad pause pid {pid:?}"));
    };

    let userns = File::open(format!("/proc/{pid}/ns/user"))?;
    let mntns = File::open(format!("/proc/{pid}/ns/mnt"))?;

    unsafe {
        if libc::setns(userns.as_raw_fd(), 0) < 0 {
            return Err(io::Error::last_os_error()).context("setns for user failed");
        }
        if libc::setns(mntns.as_raw_fd(), 0) < 0 {
            return Err(io::Error::last_os_error()).context("setns for mnt failed");
        }
    }

    Ok(())
}

fn enter_mount_namespace(pid: &str) -> eyre::Result<()> {
    let mntns = File::open(format!("/proc/{pid}/ns/mnt"))?;

    unsafe {
        if libc::setns(mntns.as_raw_fd(), 0) < 0 {
            return Err(io::Error::last_os_error()).context("setns for mnt failed");
        }
    }
    Ok(())
}

struct Mounter {
    rootfd: c_int,
}

impl Mounter {
    fn new_subtree(root: &Path) -> eyre::Result<Self> {
        let root_ffi = CString::new(root.as_os_str().as_bytes())?;
        let rootfd = unsafe {
            crate::linux::open_tree(
                libc::AT_FDCWD,
                &root_ffi,
                libc::OPEN_TREE_CLONE | libc::OPEN_TREE_CLOEXEC,
            )
        }
        .context("open_tree")?;
        Ok(Self { rootfd })
    }

    fn make_readonly(&mut self, sub: &Path) -> eyre::Result<()> {
        let sub_ffi = CString::new(sub.as_os_str().as_bytes())?;
        unsafe {
            let tree = crate::linux::open_tree(
                self.rootfd,
                &sub_ffi,
                libc::OPEN_TREE_CLONE | libc::OPEN_TREE_CLOEXEC,
            )
            .context("open_tree on subtree")?;
            crate::linux::mount_setattr(
                tree,
                c"",
                libc::AT_EMPTY_PATH as c_uint,
                &mut libc::mount_attr {
                    attr_set: libc::MOUNT_ATTR_RDONLY,
                    attr_clr: 0,
                    propagation: 0,
                    userns_fd: 0,
                },
            )
            .context("mount_setattr readonly")?;
            crate::linux::move_mount(
                tree,
                c"",
                self.rootfd,
                &sub_ffi,
                libc::MOVE_MOUNT_F_EMPTY_PATH,
            )
            .context("move_mount subtree")?;
        }
        Ok(())
    }

    fn make_self_readonly(&mut self) -> eyre::Result<()> {
        unsafe {
            crate::linux::mount_setattr(
                self.rootfd,
                c"",
                libc::AT_EMPTY_PATH as c_uint,
                &mut libc::mount_attr {
                    attr_set: libc::MOUNT_ATTR_RDONLY,
                    attr_clr: 0,
                    propagation: 0,
                    userns_fd: 0,
                },
            )
            .context("mount_setattr self readonly")?;
        }
        Ok(())
    }

    fn move_mount(self, dest: &Path) -> eyre::Result<()> {
        let dest = CString::new(dest.as_os_str().as_bytes())?;
        unsafe {
            crate::linux::move_mount(
                self.rootfd,
                c"",
                libc::AT_FDCWD,
                &dest,
                libc::MOVE_MOUNT_F_EMPTY_PATH,
            )
        }
        .context("move_mount")?;
        Ok(())
    }
}
