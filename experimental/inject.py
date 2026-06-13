#!/usr/bin/env python3

# Run in `podman unshare` to be "root".

import ctypes
import os
import sys

libc = ctypes.CDLL("libc.so.6", use_errno=True)

# Syscall numbers for x86_64
SYS_open_tree = 428
SYS_move_mount = 429
SYS_mount_setattr = 442

# open_tree flags
OPEN_TREE_CLONE = 1
OPEN_TREE_CLOEXEC = 0x80000

# move_mount flags
MOVE_MOUNT_F_EMPTY_PATH = 0x00000004

# mount_setattr flags
AT_EMPTY_PATH = 0x1000
MOUNT_ATTR_RDONLY = 0x00000001

CLONE_NEWNS = 0x00020000

AT_FDCWD = -100


def check(ret):
    if ret < 0:
        err = ctypes.get_errno()
        raise OSError(err, os.strerror(err))
    return ret


def open_tree(path):
    fd = libc.syscall(
        SYS_open_tree,
        AT_FDCWD,
        ctypes.c_char_p(path.encode()),
        OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC,
    )
    return check(fd)


class MountAttr(ctypes.Structure):
    _fields_ = [
        ("attr_set", ctypes.c_uint64),
        ("attr_clr", ctypes.c_uint64),
        ("propagation", ctypes.c_uint64),
        ("userns_fd", ctypes.c_uint64),
    ]


def make_readonly(mount_fd):
    attr = MountAttr()
    attr.attr_set = MOUNT_ATTR_RDONLY

    ret = libc.syscall(
        SYS_mount_setattr,
        mount_fd,
        ctypes.c_char_p(b""),
        AT_EMPTY_PATH,
        ctypes.byref(attr),
        ctypes.sizeof(attr),
    )
    return check(ret)


def setns(fd):
    ret = libc.setns(fd, CLONE_NEWNS)
    return check(ret)


def move_mount(mount_fd, target):
    ret = libc.syscall(
        SYS_move_mount,
        mount_fd,
        ctypes.c_char_p(b""),
        AT_FDCWD,
        ctypes.c_char_p(target.encode()),
        MOVE_MOUNT_F_EMPTY_PATH,
    )
    return check(ret)


def main():
    if os.geteuid() != 0:
        raise RuntimeError("Must run as root")

    if len(sys.argv) < 4:
        print(
            f"usage: {sys.argv[0]} <container-pid> <host-source> <container-target> [--ro]"
        )
        sys.exit(1)

    pid = int(sys.argv[1])
    host_source = os.path.abspath(sys.argv[2])
    container_target = sys.argv[3]

    # Clone mount from host namespace
    mount_fd = open_tree(host_source)

    if "--ro" in sys.argv:
        make_readonly(mount_fd)

    # Open container mount namespace
    nsfd = os.open(f"/proc/{pid}/ns/mnt", os.O_RDONLY)

    try:
        # Enter container mount namespace
        setns(nsfd)

        # Ensure target directory exists in container namespace
        os.makedirs(container_target, exist_ok=True)

        # Attach cloned mount
        move_mount(mount_fd, container_target)

        print(f"Mounted {host_source} into container pid={pid} at {container_target}")

    finally:
        os.close(nsfd)
        os.close(mount_fd)


if __name__ == "__main__":
    main()
