#!/usr/bin/env python3
"""End-to-end tests for nbox."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MANAGE = str(REPO_ROOT / "bin" / "manage-nbox")
NBOX = str(REPO_ROOT / "bin" / "nbox")
TMP = Path(tempfile.mkdtemp())

Arg = str | Path

PASS = 0
FAIL = 0


@dataclass
class ShErr:
    stderr: str


def sh(*args: Arg, cwd: Path | None = None) -> None:
    subprocess.run(args, check=True, cwd=cwd)


def sh_in(*args: Arg, stdin: str, cwd: Path | None = None) -> None:
    subprocess.run(args, input=stdin.encode(), check=True, cwd=cwd)


def sh_out(*args: Arg, cwd: Path | None = None) -> str:
    p = subprocess.run(
        args,
        capture_output=True,
        text=True,
        stdin=subprocess.DEVNULL,
        check=True,
        cwd=cwd,
    )
    return p.stdout


def sh_io(*args: Arg, stdin: str, cwd: Path | None = None) -> str:
    p = subprocess.run(
        args, capture_output=True, text=True, input=stdin, check=True, cwd=cwd
    )
    return p.stdout


def sh_out_fail(*args: Arg, cwd: Path | None = None) -> ShErr:
    p = subprocess.run(
        args, capture_output=True, text=True, stdin=subprocess.DEVNULL, cwd=cwd
    )
    assert p.returncode != 0, (
        f"expected failure but got exit 0: {args}\n  stdout: {p.stdout}\n  stderr: {p.stderr}"
    )
    return ShErr(stderr=p.stderr)


def sh_io_fail(*args: Arg, stdin: str, cwd: Path | None = None) -> ShErr:
    p = subprocess.run(args, capture_output=True, text=True, input=stdin, cwd=cwd)
    assert p.returncode != 0, (
        f"expected failure but got exit 0: {args}\n  stdout: {p.stdout}\n  stderr: {p.stderr}"
    )
    return ShErr(stderr=p.stderr)


def assert_eq(label: str, expected: str, actual: str) -> None:
    global PASS, FAIL
    if expected == actual:
        print(f"  PASS: {label}")
        PASS += 1
    else:
        print(f"  FAIL: {label} (expected {expected!r}, got {actual!r})")
        FAIL += 1


def assert_contains(needle: str, haystack: str) -> None:
    global PASS, FAIL
    if needle in haystack:
        print(f"  PASS: contains {needle!r}")
        PASS += 1
    else:
        print(f"  FAIL: expected to contain {needle!r}")
        print(f"    got: {haystack}")
        FAIL += 1


def assert_true(label: str, condition: bool) -> None:
    global PASS, FAIL
    if condition:
        print(f"  PASS: {label}")
        PASS += 1
    else:
        print(f"  FAIL: {label}")
        FAIL += 1


def project_json(project_path: Path) -> dict[str, object]:
    pj = Path(os.environ["XDG_CONFIG_HOME"]) / "nbox" / "projects.json"
    if not pj.exists():
        return {}
    data = json.loads(pj.read_text())
    return dict(data.get(str(project_path), {}))


def project_container(project_path: Path) -> str:
    cfg = project_json(project_path)
    return str(cfg["container"]) if cfg else ""


def containers_list() -> set[str]:
    return set(sh_out("podman", "ps", "-a", "--format", "{{.Names}}").splitlines())


def nbox_images() -> list[str]:
    out = sh_out("podman", "images", "--format", "{{.Repository}}:{{.Tag}}")
    images = sorted(line for line in out.splitlines() if line and ":nbox-" in line)
    assert images, "no nbox images found"
    return images


def image_choice_for(image: str) -> str:
    out = sh_out("podman", "images", "--format", "{{.Repository}}:{{.Tag}}")
    images = sorted(line for line in out.splitlines() if line and "<none>" not in line)
    for i, img in enumerate(images, 1):
        if img == image:
            return f"{i}\n"
    assert False, f"image {image!r} not found in: {images}"


def image_tests(image: str) -> None:
    image_choice = image_choice_for(image)
    # image tag is e.g. "localhost/fedora:nbox-nikita" — use the name part
    slug = image.split("/")[-1].split(":")[0]
    project = TMP / f"project-image-{slug}"
    project.mkdir()
    subdir = project / "sub" / "dir"
    subdir.mkdir(parents=True)

    sh_in(MANAGE, "create", project, stdin=image_choice)

    print("--- exec ---")

    out = sh_out(NBOX, "echo", "hello", cwd=project)
    assert_contains("hello", out)

    print("--- sudo ---")

    out = sh_out(NBOX, "sudo", "id", "-u", cwd=project)
    assert_eq("sudo runs as root", "0", out.strip())

    print("--- nested podman ---")

    has_podman = (
        subprocess.run(
            (NBOX, "podman", "--version"),
            cwd=project,
            capture_output=True,
            stdin=subprocess.DEVNULL,
        ).returncode
        == 0
    )
    if Path("/run/.containerenv").exists():
        print("  SKIP: nested podman (running inside container)")
    elif not has_podman:
        print("  SKIP: nested podman (podman not in image)")
    else:
        r = sh_io_fail(
            NBOX,
            "podman",
            "build",
            "-f",
            "-",
            project,
            stdin="FROM scratch\n",
            cwd=project,
        )
        assert_contains("Error: mounting new container", r.stderr)

        podman_project = TMP / f"project-podman-{slug}"
        podman_project.mkdir()
        sh_in(MANAGE, "create", "--podman", podman_project, stdin=image_choice)
        sh_in(
            NBOX,
            "podman",
            "build",
            "-f",
            "-",
            podman_project,
            stdin="FROM scratch\n",
            cwd=podman_project,
        )
        sh(MANAGE, "delete", podman_project)

    print("--- persistent home ---")

    sentinel = "nbox-persistence-test-" + str(os.getpid())
    home = Path.home()
    sh_out(NBOX, "sh", "-c", f"echo {sentinel} > {home}/nbox_test_persist", cwd=project)
    out = sh_out(NBOX, "cat", home / "nbox_test_persist", cwd=subdir)
    assert_eq("file persists across execs", sentinel, out.strip())
    assert_true(
        "file not on real home",
        not (Path.home() / "nbox_test_persist").exists(),
    )

    sh(MANAGE, "delete", project)


def container_create_cmd(container: str) -> list[str]:
    raw = sh_out(
        "podman", "inspect", "--format", "{{json .Config.CreateCommand}}", container
    )
    return list(json.loads(raw))


def config_freeze_tests(image: str) -> None:
    image_choice = image_choice_for(image)
    project = TMP / "project-freeze"
    project.mkdir()
    (project / ".git").mkdir()
    (project / "sub" / "dep").mkdir(parents=True)
    (project / "sub" / "dep" / ".git").mkdir()
    p = str(project)
    name = "nbox-" + p.lstrip("/").replace("/", "-")

    # Known-good command lines. If these change, review for security implications.
    expected_normal = [
        "podman",
        "run",
        "-d",
        "--name",
        name,
        "-v",
        f"{p}:{p}",
        "-v",
        f"{p}/.git:{p}/.git:ro",
        "-v",
        f"{p}/sub/dep/.git:{p}/sub/dep/.git:ro",
        "--init",
        "--userns",
        "keep-id",
        "--security-opt",
        "label=disable",
        image,
        "sleep",
        "infinity",
    ]
    expected_podman = [
        "podman",
        "run",
        "-d",
        "--name",
        name,
        "-v",
        f"{p}:{p}",
        "-v",
        f"{p}/.git:{p}/.git:ro",
        "-v",
        f"{p}/sub/dep/.git:{p}/sub/dep/.git:ro",
        "--init",
        "--userns",
        "keep-id",
        "--security-opt",
        "label=disable",
        "--device",
        "/dev/fuse",
        "--device",
        "/dev/net/tun",
        "--security-opt",
        "unmask=ALL",
        "--security-opt",
        "seccomp=unconfined",
        image,
        "sleep",
        "infinity",
    ]

    print("--- config freeze: normal ---")

    sh_in(MANAGE, "create", project, stdin=image_choice)
    cmd = container_create_cmd(name)
    assert_eq("normal nbox command", repr(expected_normal), repr(cmd))
    sh(MANAGE, "delete", project)

    print("--- config freeze: podman ---")

    sh_in(MANAGE, "create", "--podman", project, stdin=image_choice)
    cmd = container_create_cmd(name)
    assert_eq("podman nbox command", repr(expected_podman), repr(cmd))
    sh(MANAGE, "delete", project)


def system_tests(image: str) -> None:
    image_choice = image_choice_for(image)
    project_a = TMP / "project-a"
    project_a.mkdir()

    print("--- create ---")

    sh_in(MANAGE, "create", project_a, stdin=image_choice)

    container = project_container(project_a)
    assert_true("project registered in projects.json", container != "")

    out = sh_out("podman", "inspect", "--format", "{{.State.Running}}", container)
    assert_eq("container is running", "true", out.strip())

    print("--- list ---")

    out = sh_out(MANAGE, "list")
    assert_contains(str(project_a), out)
    assert_contains(container, out)

    print("--- create duplicate ---")

    out = sh_io(MANAGE, "create", project_a, stdin="1\n")
    assert_contains("already registered", out)

    print("--- nested child ---")

    child = project_a / "child"
    child.mkdir()
    r = sh_io_fail(MANAGE, "create", child, stdin="1\n")
    assert_contains("inside existing project", r.stderr)

    print("--- nested parent ---")

    project_b_inner = TMP / "project-b" / "inner"
    project_b_inner.mkdir(parents=True)

    sh_in(MANAGE, "create", project_b_inner, stdin=image_choice)

    r = sh_io_fail(MANAGE, "create", TMP / "project-b", stdin="1\n")
    assert_contains("inside this path", r.stderr)

    sh(MANAGE, "delete", project_b_inner)

    print("--- nbox no args ---")

    r = sh_out_fail(NBOX, cwd=project_a)
    assert_contains("Usage", r.stderr)

    print("--- nbox outside project ---")

    r = sh_out_fail(NBOX, "echo", "hi", cwd=TMP)
    assert_contains("No registered project", r.stderr)

    print("--- stdin ---")

    out = sh_io(NBOX, "cat", stdin="meow", cwd=project_a)
    assert_eq("cat echoes stdin", "meow", out)

    print("--- restart stopped ---")

    container = project_container(project_a)
    sh("podman", "stop", "-t", "0", container)
    out = sh_out(NBOX, "echo", "alive", cwd=project_a)
    assert_contains("alive", out)

    print("--- ro_paths saved ---")

    cfg = project_json(project_a)
    assert_eq("no ro_paths for project without .git", "[]", str(cfg.get("ro_paths")))

    print("--- ro_paths mismatch ---")

    # Simulate a .git dir appearing after create — nbox should refuse
    (project_a / ".git").mkdir()
    r = sh_out_fail(NBOX, "echo", "hi", cwd=project_a)
    assert_contains("Read-only mounts changed", r.stderr)
    (project_a / ".git").rmdir()

    print("--- create with .git ---")

    project_ro = TMP / "project-ro"
    project_ro.mkdir()
    (project_ro / ".git").mkdir()
    (project_ro / "sub").mkdir()
    (project_ro / "sub" / ".git").mkdir()

    sh_in(MANAGE, "create", project_ro, stdin=image_choice)

    cfg_ro = project_json(project_ro)
    assert_eq(
        "ro_paths has .git entries", "['.git', 'sub/.git']", str(cfg_ro.get("ro_paths"))
    )

    # .git should be read-only inside container
    r = sh_out_fail(NBOX, "touch", project_ro / ".git" / "test", cwd=project_ro)
    assert_contains("Read-only file system", r.stderr)

    # umount to bypass RO should fail (unprivileged, even with sudo)
    r = sh_out_fail(NBOX, "sudo", "umount", project_ro / ".git", cwd=project_ro)
    assert_contains("umount", r.stderr)

    # regular files should still be writable
    sh_out(NBOX, "touch", project_ro / "writable", cwd=project_ro)
    assert_true("writable file created", (project_ro / "writable").exists())

    sh(MANAGE, "delete", project_ro)

    print("--- intermediate dir ownership ---")

    deep = TMP / "deep" / "nested" / "project"
    deep.mkdir(parents=True)
    sh_in(MANAGE, "create", deep, stdin=image_choice)

    # Intermediate dirs between TMP and the mount point should be owned by
    # the user, not root.  Podman creates them as root when setting up the
    # bind mount.
    uid = str(os.getuid())
    for d in [TMP / "deep", TMP / "deep" / "nested"]:
        out = sh_out(NBOX, "stat", "-c", "%u", d, cwd=deep).strip()
        assert_eq(f"{d.name}/ owned by user", uid, out)

    sh(MANAGE, "delete", deep)

    print("--- delete ---")

    container = project_container(project_a)

    out = sh_out(MANAGE, "delete", project_a)
    assert_contains("Deleted", out)

    sh_out_fail("podman", "inspect", container)

    pj = Path(os.environ["XDG_CONFIG_HOME"]) / "nbox" / "projects.json"
    pj_text = pj.read_text() if pj.exists() else ""
    assert_true("projects.json entry gone", str(project_a) not in pj_text)

    print("--- delete nonexistent ---")

    r = sh_out_fail(MANAGE, "delete", project_a)
    assert_contains("No registered project", r.stderr)

    print("--- local image in build menu ---")

    local_images = Path(os.environ["XDG_CONFIG_HOME"]) / "nbox" / "images" / "mylocal"
    local_images.mkdir(parents=True)
    (local_images / "Containerfile").write_text("FROM scratch\n")

    # EOF on stdin makes input() fail, so check=False.
    p = subprocess.run(
        (MANAGE, "build"), capture_output=True, text=True, check=False, input=""
    )
    assert_contains("mylocal", p.stdout)

    shutil.rmtree(local_images.parent)


def main() -> None:
    global PASS, FAIL

    os.environ["XDG_CONFIG_HOME"] = str(TMP / "config")
    containers_before = containers_list()

    try:
        image_dirs = sorted(
            d.name
            for d in (REPO_ROOT / "images").iterdir()
            if d.is_dir() and (d / "Containerfile").exists()
        )
        for i, name in enumerate(image_dirs, 1):
            print(f"--- build {name} ---")
            sh_in(MANAGE, "build", stdin=f"{i}\n")

        images = nbox_images()
        fedora = next(i for i in images if "/fedora:" in i)
        system_tests(fedora)
        config_freeze_tests(fedora)
        for image in images:
            print(f"\n=== image_tests: {image} ===")
            image_tests(image)
    finally:
        containers_after = containers_list()
        for c in containers_after - containers_before:
            sh("podman", "rm", "-f", "-t", "0", c)
        shutil.rmtree(TMP)

        print()
        print("==============================")
        print(f"  PASS: {PASS}   FAIL: {FAIL}")
        print("==============================")

    sys.exit(1 if FAIL > 0 else 0)


if __name__ == "__main__":
    main()
