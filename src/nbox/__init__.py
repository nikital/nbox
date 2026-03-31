import argparse
import dataclasses
import json
import os
import pwd
import subprocess
import sys
from collections.abc import Iterator
from pathlib import Path

CMD_BUILD = "build"
CMD_CREATE = "create"
CMD_DELETE = "delete"
CMD_LIST = "list"


def config_dir() -> Path:
    xdg = os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config"))
    return Path(xdg) / "nbox"


def projects_file() -> Path:
    return config_dir() / "projects.json"


def images_dirs() -> Iterator[Path]:
    yield Path(__file__).resolve().parent.parent.parent / "images"
    yield config_dir() / "images"


def compute_ro_paths(root: Path) -> list[str]:
    out = subprocess.check_output(
        [
            "fd",
            "--type",
            "d",
            "--hidden",
            "--no-ignore",
            "--prune",
            "--glob",
            ".git",
            str(root),
        ],
        text=True,
    )
    ro = []
    for line in out.splitlines():
        ro.append(str(Path(line).relative_to(root)))
    ro.sort()
    return ro


@dataclasses.dataclass
class ProjectConfig:
    container: str
    ro_paths: list[str]


def load_projects() -> dict[str, ProjectConfig]:
    f = projects_file()
    if not f.exists():
        return {}
    raw: object = json.loads(f.read_text())
    assert isinstance(raw, dict)
    result: dict[str, ProjectConfig] = {}
    for k, v in raw.items():
        assert isinstance(v, dict)
        result[k] = ProjectConfig(
            container=str(v["container"]),
            ro_paths=list(v.get("ro_paths", [])),
        )
    return result


def save_projects(projects: dict[str, ProjectConfig]) -> None:
    d = config_dir()
    d.mkdir(parents=True, exist_ok=True)
    projects_file().write_text(
        json.dumps({k: dataclasses.asdict(v) for k, v in projects.items()}, indent=2)
        + "\n"
    )


def container_name(path: Path) -> str:
    return "nbox-" + str(path).lstrip("/").replace("/", "-")


def find_project(
    cwd: Path, projects: dict[str, ProjectConfig]
) -> tuple[str, ProjectConfig] | None:
    best: tuple[str, ProjectConfig] | None = None
    best_len = -1
    for proj_path, cfg in projects.items():
        try:
            cwd.relative_to(proj_path)
        except ValueError:
            continue
        if len(proj_path) > best_len:
            best_len = len(proj_path)
            best = (proj_path, cfg)
    return best


def ensure_running(container: str) -> None:
    result = subprocess.run(
        ["podman", "inspect", "--format", "{{.State.Running}}", container],
        capture_output=True,
        text=True,
    )
    if result.stdout.strip() != "true":
        subprocess.run(["podman", "start", container], check=True)


def pick_containerfile(name: str | None = None) -> Path:
    dirs = sorted(
        d
        for root in images_dirs()
        if root.is_dir()
        for d in root.iterdir()
        if d.is_dir() and (d / "Containerfile").exists()
    )
    if not dirs:
        print("No Containerfiles found in images/", file=sys.stderr)
        sys.exit(1)
    if name is not None:
        matches = [d for d in dirs if d.name == name]
        if len(matches) != 1:
            raise RuntimeError(f"Multiple images matches: {matches}")
        return matches[0]
    print("Available images:")
    for i, d in enumerate(dirs, 1):
        print(f"  {i}) {d.name}")
    while True:
        choice = input(f"Select image [1-{len(dirs)}]: ").strip()
        if choice.isdigit() and 1 <= int(choice) <= len(dirs):
            return dirs[int(choice) - 1]
        print("Invalid choice, try again.")


def cmd_build(image: str | None) -> None:
    image_dir = pick_containerfile(image)
    pw = pwd.getpwuid(os.getuid())
    tag = f"{image_dir.name}:nbox-{pw.pw_name}"
    subprocess.check_call(
        [
            "podman",
            "build",
            str(image_dir),
            "--build-arg",
            f"USER={pw.pw_name}",
            "--build-arg",
            f"UID={pw.pw_uid}",
            "--build-arg",
            f"HOME={pw.pw_dir}",
            "-t",
            tag,
        ]
    )


def pick_image(name: str | None = None) -> str:
    result = subprocess.run(
        ["podman", "images", "--format", "{{.Repository}}:{{.Tag}}"],
        capture_output=True,
        text=True,
        check=True,
    )
    images = sorted(
        line for line in result.stdout.splitlines() if line and "<none>" not in line
    )
    if not images:
        print("No local images found. Pull an image first.", file=sys.stderr)
        sys.exit(1)
    if name is not None:
        nbox_images = [i for i in images if ":nbox-" in i]
        matches = [
            i for i in nbox_images if f"/{name}:" in i or i.startswith(f"{name}:")
        ]
        if len(matches) != 1:
            raise RuntimeError(f"Multiple images matches: {matches}")
        return matches[0]
    print("Available images:")
    for i, img in enumerate(images, 1):
        print(f"  {i}) {img}")
    while True:
        choice = input(f"Select image [1-{len(images)}]: ").strip()
        if choice.isdigit() and 1 <= int(choice) <= len(images):
            return images[int(choice) - 1]
        print("Invalid choice, try again.")


def cmd_create(path_arg: Path, podman: bool, image: str | None) -> None:
    path = path_arg.resolve()
    path_str = str(path)
    name = container_name(path)
    projects = load_projects()
    if path_str in projects:
        print(
            f"Project already registered: {path_str} -> {projects[path_str].container}"
        )
        return
    for proj_path in projects:
        proj = Path(proj_path)
        if path.is_relative_to(proj):
            print(f"Path is inside existing project: {proj}", file=sys.stderr)
            sys.exit(1)
        if proj.is_relative_to(path):
            print(f"Existing project is inside this path: {proj}", file=sys.stderr)
            sys.exit(1)
    image_tag = pick_image(image)
    ro = compute_ro_paths(path)
    ro_mounts: list[str] = []
    for rp in ro:
        abs_path = path / rp
        ro_mounts += ["-v", f"{abs_path}:{abs_path}:ro"]

    flags: list[str] = []
    if podman:
        flags = [
            "--device",
            "/dev/fuse",
            "--device",
            "/dev/net/tun",
            "--security-opt",
            "unmask=ALL",
            "--security-opt",
            "seccomp=unconfined",
        ]
    subprocess.check_call(
        [
            "podman",
            "run",
            "-d",
            "--name",
            name,
            "-v",
            f"{path_str}:{path_str}",
            *ro_mounts,
            "--init",
            "--userns",
            "keep-id",
            "--security-opt",
            "label=disable",
            *flags,
            image_tag,
            "sleep",
            "infinity",
        ],
    )
    # Podman creates intermediate dirs as root; mirror host ownership.
    # Walk up from mount point, collect dirs owned by us on the host — those
    # should be ours inside the container too.  Stop at the first dir not
    # owned by us (e.g. /tmp, /home).
    uid = os.getuid()
    gid = os.getgid()
    fix = []
    d = path.parent
    while d != d.parent:
        if d.stat().st_uid != uid:
            break
        fix.append(str(d))
        d = d.parent
    if fix:
        subprocess.check_call(
            ["podman", "exec", name, "sudo", "chown", f"{uid}:{gid}", *fix]
        )
    projects[path_str] = ProjectConfig(container=name, ro_paths=ro)
    save_projects(projects)


def cmd_list() -> None:
    projects = load_projects()
    for path, cfg in sorted(projects.items()):
        print(f"{path} -> {cfg.container}")


def pick_project() -> str:
    projects = sorted(load_projects().items())
    if not projects:
        print("No registered projects", file=sys.stderr)
        sys.exit(1)
    print("Registered projects:")
    for i, (path, cfg) in enumerate(projects, 1):
        print(f"  {i}) {path} -> {cfg.container}")
    while True:
        choice = input(f"Select project [1-{len(projects)}]: ").strip()
        if choice.isdigit() and 1 <= int(choice) <= len(projects):
            return projects[int(choice) - 1][0]
        print("Invalid choice, try again.")


def cmd_delete(path_arg: Path | None) -> None:
    if path_arg is None:
        path_str = pick_project()
    else:
        path = path_arg.resolve()
        path_str = str(path)
    projects = load_projects()
    if path_str not in projects:
        print(f"No registered project: {path_str}", file=sys.stderr)
        sys.exit(1)
    cfg = projects.pop(path_str)
    subprocess.run(["podman", "rm", "-f", "-t", "0", cfg.container], check=True)
    save_projects(projects)
    print(f"Deleted {path_str} -> {cfg.container}")


def manage() -> None:
    parser = argparse.ArgumentParser()
    cmd = parser.add_subparsers(dest="command", required=True)
    build = cmd.add_parser(CMD_BUILD, help="Build a sandbox image")
    build.add_argument("--image", help="Image name")
    cmd.add_parser(CMD_LIST, help="List registered projects")
    create = cmd.add_parser(
        CMD_CREATE, help="Register a project and start its container"
    )
    create.add_argument("path", type=Path, help="Project directory to register")
    create.add_argument(
        "--podman", action="store_true", help="Allow podman inside the sandbox"
    )
    create.add_argument("--image", help="Image name")
    delete = cmd.add_parser(
        CMD_DELETE, help="Stop and remove container, deregister project"
    )
    delete.add_argument(
        "path", type=Path, nargs="?", help="Project directory to remove"
    )
    args = parser.parse_args()
    if args.command == CMD_BUILD:
        cmd_build(args.image)
    elif args.command == CMD_LIST:
        cmd_list()
    elif args.command == CMD_CREATE:
        cmd_create(args.path, args.podman, args.image)
    elif args.command == CMD_DELETE:
        cmd_delete(args.path)


def nbox() -> None:
    args = sys.argv[1:]
    if not args:
        print("Usage: nbox <cmd> [args...]", file=sys.stderr)
        sys.exit(1)
    cwd = Path.cwd()
    projects = load_projects()
    match = find_project(cwd, projects)
    if match is None:
        print(f"No registered project contains: {cwd}", file=sys.stderr)
        sys.exit(1)
    proj_path, cfg = match
    current_ro = compute_ro_paths(Path(proj_path))
    if current_ro != cfg.ro_paths:
        print(
            f"Read-only mounts changed, recreate container with manage-nbox\n"
            f"  saved:   {cfg.ro_paths}\n"
            f"  current: {current_ro}",
            file=sys.stderr,
        )
        sys.exit(1)
    ensure_running(cfg.container)
    flags = ["-it"] if sys.stdin.isatty() else ["-i"]
    result = subprocess.run(["podman", "exec", *flags, "-w", cwd, cfg.container, *args])
    sys.exit(result.returncode)
