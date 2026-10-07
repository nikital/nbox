use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
};

macro_rules! assert_contains {
    ($needle:expr, $haystack:expr) => {
        assert!(
            $haystack.contains($needle),
            "expected to contain {needle:?}\n  got: {haystack:?}",
            needle = $needle,
            haystack = $haystack,
        );
    };
}

#[test]
fn e2e() {
    let env = TestEnv::new();
    build_images(&env);
    config_freeze_tests(&env);
    system_tests(&env);
    image_tests(&env, "fedora-toolbox");
    image_tests(&env, "ubuntu");
}

fn build_images(env: &TestEnv) {
    manage_ok(&["build", "--image", "fedora-toolbox"]);
    manage_ok(&["build", "--image", "ubuntu"]);

    // --- Create external images + dependencies ---

    // Clean up from previous runs
    let _ = Command::new("podman")
        .args(["rmi", "-f", "localhost/mylocal:nbox"])
        .status();
    let _ = Command::new("podman")
        .args(["rmi", "-f", "localhost/mylocal_base:nbox"])
        .status();

    let images_dir = env.config_home.join("nbox/images");

    let base_dir = images_dir.join("mylocal_base");
    fs::create_dir_all(&base_dir).expect("create mylocal_base image dir");
    let base_containerfile = concat!(
        "FROM localhost/fedora-toolbox:nbox\n",
        "ARG USER\n",
        "ARG UID\n",
        "ARG HOME\n",
        "RUN echo base >> /etc/mylocal-test\n",
    );
    fs::write(base_dir.join("Containerfile"), base_containerfile)
        .expect("write mylocal_base Containerfile");

    // mylocal depends on mylocal_base
    let local_dir = images_dir.join("mylocal");
    fs::create_dir_all(&local_dir).expect("create mylocal image dir");
    let local_containerfile = concat!(
        "FROM localhost/mylocal_base:nbox\n",
        "ARG USER\n",
        "ARG UID\n",
        "ARG HOME\n",
        "RUN echo mylocal >> /etc/mylocal-test\n",
    );
    fs::write(local_dir.join("Containerfile"), local_containerfile)
        .expect("write mylocal Containerfile");

    // (should recursively build mylocal_base first)
    manage_ok(&["build", "--image", "mylocal"]);

    // Verify images exist
    let output = Command::new("podman")
        .args(["images", "--format", "{{.Repository}}:{{.Tag}}"])
        .output()
        .expect("podman images");
    let images_list = String::from_utf8(output.stdout).unwrap();

    assert_contains!("fedora-toolbox", &images_list);
    assert_contains!("ubuntu", &images_list);
    assert_contains!("mylocal_base", &images_list);
    assert_contains!("mylocal", &images_list);

    // Verify mylocal built correctly
    let output = Command::new("podman")
        .args([
            "run",
            "--rm",
            "localhost/mylocal:nbox",
            "cat",
            "/etc/mylocal-test",
        ])
        .output()
        .expect("podman run mylocal");
    assert!(
        output.status.success(),
        "mylocal Containerfile RUN was executed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!("base\nmylocal\n", String::from_utf8(output.stdout).unwrap());
}

/// Test environment that isolates tests from the real system.
///
/// Sets `XDG_CONFIG_HOME` to a temp subdir, snapshots pre-existing podman
/// containers, and force-removes any containers created during the test on
/// teardown.
struct TestEnv {
    dir: tempfile::TempDir,
    config_home: PathBuf,
    containers_before: Vec<String>,
}

impl TestEnv {
    fn new() -> Self {
        let dir = tempfile::TempDir::new().expect("failed to create temp dir");
        let config_home = dir.path().join("config");
        fs::create_dir_all(&config_home).expect("failed to create config dir");
        unsafe { env::set_var("XDG_CONFIG_HOME", &config_home) };

        // `manage-nbox bin` needs `nbox` on PATH to know the link target.
        let nbox_bin = PathBuf::from(env!("CARGO_BIN_EXE_nbox"));
        let bin_dir = nbox_bin.parent().expect("nbox bin has a parent");
        let path = env::join_paths(
            std::iter::once(bin_dir.to_path_buf())
                .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
        )
        .expect("join PATH");
        unsafe { env::set_var("PATH", path) };

        eprintln!("Test dir: {}", dir.path().display());

        let containers_before = Self::list_containers();

        Self {
            dir,
            config_home,
            containers_before,
        }
    }

    fn list_containers() -> Vec<String> {
        let output = Command::new("podman")
            .args(["ps", "-a", "--format", "{{.Names}}"])
            .output();
        match output {
            Ok(o) if o.status.success() => String::from_utf8(o.stdout)
                .expect("non utf8 name")
                .lines()
                .filter(|l| !l.is_empty())
                .map(|s| s.to_string())
                .collect(),
            _ => vec![],
        }
    }
}

fn nbox(proj: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nbox"))
        .args(args)
        .current_dir(proj)
        .output()
        .expect("spawn nbox")
}

fn nbox_ok(proj: &Path, args: &[&str]) -> String {
    let out = nbox(proj, args);
    assert!(
        out.status.success(),
        "nbox {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn nbox_in(proj: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nbox"))
        .args(args)
        .current_dir(proj)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn nbox");
    let mut stdin_stream = child.stdin.take().unwrap();
    let mut stdout_stream = child.stdout.take().unwrap();
    let mut stderr_stream = child.stderr.take().unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    thread::scope(|s| {
        s.spawn(|| {
            stdin_stream.write_all(stdin.as_bytes()).unwrap();
            drop(stdin_stream);
        });
        s.spawn(|| stdout_stream.read_to_end(&mut stdout).unwrap());
        s.spawn(|| stderr_stream.read_to_end(&mut stderr).unwrap());
    });
    Output {
        status: child.wait().unwrap(),
        stdout,
        stderr,
    }
}

fn manage(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_manage-nbox"))
        .args(args)
        .output()
        .expect("spawn manage-nbox")
}

fn manage_ok(args: &[&str]) -> Output {
    let out = manage(args);
    assert!(
        out.status.success(),
        "manage-nbox {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn manage_err(args: &[&str]) -> String {
    let out = manage(args);
    assert!(
        !out.status.success(),
        "manage-nbox {args:?} should have failed"
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn system_tests(env: &TestEnv) {
    let tmp = env.dir.path().join("tmp");
    fs::create_dir_all(&tmp).unwrap();

    // One project exercises the whole lifecycle; only --podman needs a fresh one.
    let proj = tmp.join("proj");
    fs::create_dir_all(&proj).unwrap();
    let proj_str = proj.to_str().unwrap();
    let cname = container_name_for(&proj);

    {
        // ---- create + list + duplicate ----
        manage_ok(&["create", proj_str, "--image", "fedora-toolbox"]);

        let out = manage_ok(&["list"]);
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert_contains!(
            &fs::canonicalize(&proj).unwrap().display().to_string(),
            &stdout
        );

        let out = manage_ok(&["list", proj_str]);
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert_contains!(&cname, &stdout);

        let out = Command::new("podman")
            .args(["ps", "-a", "--format", "{{.Names}}"])
            .output()
            .unwrap();
        let names = String::from_utf8(out.stdout).unwrap();
        assert_contains!(&cname, &names);

        let stderr = manage_err(&["create", proj_str, "--image", "fedora-toolbox"]);
        assert_contains!("already exists", &stderr);
    }

    {
        // ---- nbox outside a project ----
        let out = nbox(&tmp, &["true"]);
        assert!(!out.status.success(), "nbox outside a project should fail");
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_contains!("No registered project", &stderr);
    }

    {
        // ---- stdin piping ----
        let out = nbox_in(&proj, &["cat"], "meow\n");
        assert!(
            out.status.success(),
            "nbox cat with piped stdin should work"
        );
        assert_eq!("meow\n", String::from_utf8_lossy(&out.stdout));
    }

    {
        // ---- login shell env ----
        assert!(
            nbox(
                &proj,
                &["bash", "-c", "echo 'export HELLO=box' >> ~/.bashrc"]
            )
            .status
            .success()
        );
        let out = nbox_ok(&proj, &["env"]);
        assert_contains!("HELLO=box", &out);
    }

    {
        // ---- create nesting guards ----
        let child = proj.join("child");
        fs::create_dir_all(&child).unwrap();
        let child_str = child.to_str().unwrap();

        let stderr = manage_err(&["create", child_str, "--image", "fedora-toolbox"]);
        assert_contains!("inside existing project", &stderr);

        let tmp_str = tmp.to_str().unwrap();
        let stderr = manage_err(&["create", tmp_str, "--image", "fedora-toolbox"]);
        assert_contains!("under this path", &stderr);
    }

    {
        // ---- .git read-only, drift detection, stop/restart ----
        fs::create_dir_all(proj.join(".git")).unwrap();
        fs::create_dir_all(proj.join("sub/.git")).unwrap();

        assert!(
            !nbox(&proj, &["touch", ".git/x"]).status.success(),
            ".git should be read-only"
        );

        assert!(
            nbox(&proj, &["touch", "made-by-container"])
                .status
                .success(),
            "project dir should be writable"
        );
        assert!(proj.join("made-by-container").exists());

        // New .git dir after create → drift is refused.
        fs::create_dir_all(proj.join("other/.git")).unwrap();
        let out = nbox(&proj, &["true"]);
        assert!(!out.status.success(), "drift should be refused");
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_contains!("mounts changed", &stderr);

        // stop clears the cache; next invocation remounts with the new set.
        manage_ok(&["stop", proj_str]);

        assert!(nbox(&proj, &["true"]).status.success(), "nbox after stop");
        assert!(
            !nbox(&proj, &["touch", "other/.git/x"]).status.success(),
            "new .git should also be read-only"
        );
    }

    {
        // ---- extra mounts ----
        // Directory mount; .git inside it is read-only like the project root.
        let src_dir = tmp.join("mount-src");
        fs::create_dir_all(src_dir.join(".git")).unwrap();
        fs::write(src_dir.join("hello.txt"), "hello from host\n").unwrap();
        let src_dir_str = src_dir.to_str().unwrap();

        manage_ok(&["mount", proj_str, src_dir_str]);

        let out = manage_ok(&["mount", proj_str]);
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert_contains!(&src_dir.display().to_string(), &stdout);

        assert_eq!(
            "hello from host\n",
            nbox_ok(&proj, &["cat", src_dir.join("hello.txt").to_str().unwrap()])
        );

        assert!(
            nbox(
                &proj,
                &["touch", src_dir.join("from-container").to_str().unwrap()]
            )
            .status
            .success()
        );
        assert!(src_dir.join("from-container").exists());

        assert!(
            !nbox(&proj, &["touch", src_dir.join(".git/x").to_str().unwrap()])
                .status
                .success(),
            "extra mount .git should be read-only"
        );

        // Read-write file mount.
        let rw_file = tmp.join("mount-rw-file");
        fs::write(&rw_file, "rw file\n").unwrap();
        let rw_file_str = rw_file.to_str().unwrap();
        manage_ok(&["mount", proj_str, rw_file_str]);

        let cmd = format!("echo from-container >> {}", rw_file.display());
        assert!(nbox(&proj, &["bash", "-c", cmd.as_str()]).status.success());
        assert_eq!(
            "rw file\nfrom-container\n",
            fs::read_to_string(&rw_file).unwrap()
        );

        manage_ok(&["umount", proj_str, rw_file_str]);

        // Read-only file mount.
        let src_file = tmp.join("mount-file");
        fs::write(&src_file, "ro file\n").unwrap();
        let src_file_str = src_file.to_str().unwrap();
        manage_ok(&["mount", proj_str, src_file_str, "--ro"]);

        assert_eq!("ro file\n", nbox_ok(&proj, &["cat", src_file_str]));

        assert!(
            !nbox(&proj, &["touch", src_file_str]).status.success(),
            "ro file should be read-only"
        );

        // Unmount the directory; the remaining ro file survives the restart.
        manage_ok(&["umount", proj_str, src_dir_str]);

        assert!(
            !nbox(
                &proj,
                &["test", "-e", src_dir.join("hello.txt").to_str().unwrap()]
            )
            .status
            .success(),
            "unmounted dir should be gone"
        );

        assert_eq!("ro file\n", nbox_ok(&proj, &["cat", src_file_str]));

        // Interactive umount of the remaining mount.
        let mut child = Command::new(env!("CARGO_BIN_EXE_manage-nbox"))
            .args(["umount", proj_str])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.as_mut().unwrap().write_all(b"1\n").unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "interactive umount failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let out = manage_ok(&["mount", proj_str]);
        assert_eq!("", String::from_utf8_lossy(&out.stdout), "no mounts listed");
    }

    {
        // ---- bin multicall ----
        manage_ok(&["bin", "echo"]);

        let link = env.config_home.join("nbox/bin/nbox-echo");
        assert!(link.is_symlink(), "symlink should exist");

        let out = Command::new(&link)
            .args(["hello"])
            .current_dir(&proj)
            .output()
            .expect("spawn nbox-echo");
        assert!(
            out.status.success(),
            "nbox-echo failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_contains!("hello", &String::from_utf8_lossy(&out.stdout));

        // Idempotent: same target is not an error.
        manage_ok(&["bin", "echo"]);
    }

    {
        // ---- delete nonexistent ----
        let never_created = tmp.join("never-created");
        fs::create_dir_all(&never_created).unwrap();
        let never_created_str = never_created.to_str().unwrap();

        let stderr = manage_err(&["delete", never_created_str]);
        assert!(!stderr.is_empty());
    }

    {
        // ---- delete the main project ----
        manage_ok(&["delete", proj_str]);

        let out = Command::new("podman")
            .args(["ps", "-a", "--format", "{{.Names}}"])
            .output()
            .unwrap();
        let names = String::from_utf8(out.stdout).unwrap();
        assert!(!names.contains(&cname), "container should be removed");

        let out = manage_ok(&["list"]);
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains(proj_str),
            "project should be unregistered"
        );
    }

    {
        // ---- create --podman ----
        let podman_proj = tmp.join("podman-proj");
        fs::create_dir_all(&podman_proj).unwrap();
        let podman_proj_str = podman_proj.to_str().unwrap();

        manage_ok(&[
            "create",
            podman_proj_str,
            "--podman",
            "--image",
            "fedora-toolbox",
        ]);

        assert!(
            nbox(&podman_proj, &["test", "-c", "/dev/fuse"])
                .status
                .success(),
            "container should have /dev/fuse"
        );

        manage_ok(&["delete", podman_proj_str]);
    }
}

fn config_freeze_tests(env: &TestEnv) {
    let proj = env.dir.path().join("project-freeze");
    fs::create_dir_all(proj.join(".git")).unwrap();
    fs::create_dir_all(proj.join("sub/dep/.git")).unwrap();
    let proj_str = proj.to_str().unwrap();
    let name = container_name_for(&proj);
    let tag = "localhost/fedora-toolbox:nbox";

    // Known-good command lines. If these change, review for security implications.
    #[rustfmt::skip]
    let expected_normal: Vec<String> = [
        "podman",
        "create",
        "--name", name.as_str(),
        "--init",
        "--userns", "keep-id",
        "--security-opt", "label=disable",
        "--network", "pasta:-t,auto,-u,auto,-T,auto,-U,auto",
        "--tmpfs", "/tmp",
        "--tmpfs", "/run",

        tag,

        "sleep", "infinity",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect();

    #[rustfmt::skip]
    let expected_podman: Vec<String> = [
        "podman",
        "create",
        "--name", name.as_str(),
        "--init",
        "--userns", "keep-id",
        "--security-opt", "label=disable",
        "--network", "pasta:-t,auto,-u,auto,-T,auto,-U,auto",
        "--tmpfs", "/tmp",
        "--tmpfs", "/run",

        "--device", "/dev/fuse",
        "--device", "/dev/net/tun",
        "--security-opt", "seccomp=unconfined",
        "--security-opt", "unmask=ALL",

        tag,

        "sleep", "infinity",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect();

    manage_ok(&["create", proj_str, "--image", "fedora-toolbox"]);
    assert_eq!(expected_normal, container_create_cmd(&name));
    manage_ok(&["delete", proj_str]);

    manage_ok(&["create", proj_str, "--podman", "--image", "fedora-toolbox"]);
    assert_eq!(expected_podman, container_create_cmd(&name));
    manage_ok(&["delete", proj_str]);
}

fn image_tests(env: &TestEnv, image: &str) {
    let proj = env.dir.path().join(format!("image-{image}"));
    fs::create_dir_all(&proj).unwrap();
    let proj_str = proj.to_str().unwrap();

    manage_ok(&["create", proj_str, "--image", image]);

    // sudo is configured
    assert_eq!("0\n", nbox_ok(&proj, &["sudo", "id", "-u"]));

    // ~ lives in the container overlay, not on the host home
    let host_file = env::home_dir().expect("no home dir").join("nbox_test_home");
    assert!(
        nbox(&proj, &["bash", "-c", "echo x > ~/nbox_test_home"])
            .status
            .success()
    );
    assert!(
        !host_file.exists(),
        "container home file must not appear on host home"
    );

    // nested podman: fails normally, succeeds with --podman
    let in_container = Path::new("/run/.containerenv").exists();
    let has_podman = nbox(&proj, &["podman", "--version"]).status.success();
    if !in_container && has_podman {
        let out = nbox_in(
            &proj,
            &["podman", "build", "-f", "-", proj_str],
            "FROM scratch\n",
        );
        assert!(
            !out.status.success(),
            "podman build without --podman should fail"
        );
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_contains!("mounting new container", &stderr);

        let podman_proj = env.dir.path().join(format!("image-podman-{image}"));
        fs::create_dir_all(&podman_proj).unwrap();
        let podman_proj_str = podman_proj.to_str().unwrap();
        manage_ok(&["create", podman_proj_str, "--podman", "--image", image]);

        let out = nbox_in(
            &podman_proj,
            &["podman", "build", "-f", "-", podman_proj_str],
            "FROM scratch\n",
        );
        assert!(
            out.status.success(),
            "podman build with --podman should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        manage_ok(&["delete", podman_proj_str]);
    }

    manage_ok(&["delete", proj_str]);
}

fn container_create_cmd(name: &str) -> Vec<String> {
    let raw = Command::new("podman")
        .args([
            "inspect",
            "--format",
            "{{json .Config.CreateCommand}}",
            name,
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn podman inspect")
        .wait_with_output()
        .expect("wait for podman inspect");
    assert!(raw.status.success(), "podman inspect {name} failed");
    serde_json::from_slice(&raw.stdout).unwrap_or_else(|e| {
        panic!(
            "parse CreateCommand for {name}: {e}\nraw: {}",
            String::from_utf8_lossy(&raw.stdout)
        )
    })
}

fn container_name_for(path: &std::path::Path) -> String {
    let canonical = fs::canonicalize(path).unwrap();
    format!(
        "nbox-{}",
        canonical
            .to_str()
            .unwrap()
            .trim_start_matches('/')
            .replace('/', "-")
    )
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let after = Self::list_containers();
        for c in &after {
            if !self.containers_before.contains(c) {
                eprintln!("TestEnv: removing leftover container {c:?}");
                let _ = Command::new("podman")
                    .args(["rm", "-f", "-t", "0", c])
                    .status();
            }
        }
    }
}
