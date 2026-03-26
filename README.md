# nbox

Per-project Podman container sandboxes. Mount your project directory into an
isolated container, run commands inside it with `nbox`, manage sandboxes with
`manage-nbox`.

## Install

```sh
git clone <repo> && cd nbox
./install.sh      # symlinks bin/* into ~/.local/bin
```

Requires: `podman`, `fd`.

## Usage

Build an image, create a sandbox for your project, then run commands inside it:

```sh
manage-nbox build              # pick and build a Containerfile from images/
manage-nbox create ~/proj/foo  # create a sandbox for the project
cd ~/proj/foo
nbox bash                      # run bash inside the sandbox
nbox make test                 # run any command
```

Other management commands exist, see `manage-nbox --help`.

## Security

See [`config_freeze_tests`](./tests/e2e.py) for the expected Podman
configuration that the sandbox will use.

`.git` directories found under the project root are mounted read-only into the
container. If the set of read-only directories changes, `nbox` will refuse to
run and ask you to recreate the sandbox. I didn't find a sane way to adjust
mounted volumes in a running container so that's what we have for now.

If you want to run Podman inside the sandbox, you need to relax it a bit using
`manage-nbox create --podman`. (As of writing - adds `/dev/fuse`,
`/dev/net/tun`, disables seccomp, unmasks all paths.) The main use-case is
development of nbox itself.

## Add personal custom images

nbox looks for Containerfiles in two places:

1. Built-in `images/` directory (ships with nbox)
2. `$XDG_CONFIG_HOME/nbox/images/` (default `~/.config/nbox/images/`)

To add your own image, create a directory with a `Containerfile`. See `images/`
for examples.
