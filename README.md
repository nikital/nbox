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

Security is provided by a Podman container, it's up to you to decide if this
boundary is strong enough to run untrusted code. See `config_freeze_tests` in
[e2e.py](./tests/e2e.py) for the expected Podman command-line that the sandbox
will use.

Only the project directory is inside the sandbox, so any code running inside the
sandbox won't be able to access any credentials you have on your
machine. (Unless you put credentials in the project folder itself...)

`.git` directories found under the project root are mounted read-only into the
container. This allows you to treat Git as a "trusted" tool that can be used
outside of the sandbox to see what changed in a project and to control exactly
what you commit. Also `git push` will be done outside of the sandbox as it needs
credentials.

Once untrusted code has been loaded into the sandbox (e.g. you ran an LLM coding
agent or you installed random dependencies), make sure you never execute
anything in the project directory outside of the sandbox. Even if you audited
the changes with Git, the environment may be poisoned with malicious code in
`.gitignore`d paths like `node_modules` / `__pycache__` / venv / `.o`
files. Beware of indirect execution via LSP servers, for example `rust-analyzer`
will happily run `build.rs` from a dependency. Install LSP servers in the
sandbox and run them with `nbox rust-analyzer`.

To make a sandboxed directory trustworthy again you need to kill the sandbox
(`manage-nbox delete`), then `git clean -xdf`.

Another note regarding Git: If the set of read-only directories changes, `nbox`
will refuse to run and ask you to recreate the sandbox. I didn't find a sane way
to adjust mounted volumes in a running container so that's what we have for now.

If you want to run Podman inside the sandbox, you need to relax the sandbox a
bit using `manage-nbox create --podman`. (As of writing - adds `/dev/fuse`,
`/dev/net/tun`, disables seccomp, unmasks all paths.) The main use-case is
development of nbox itself.

## Add personal custom images

nbox looks for Containerfiles in two places:

1. Built-in `images/` directory (ships with nbox)
2. `$XDG_CONFIG_HOME/nbox/images/` (default `~/.config/nbox/images/`)

To add your own image, create a directory with a `Containerfile`. See `images/`
for examples.
