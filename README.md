# nbox

Per-project Podman container sandboxes. Mount your project directory into an
isolated container, run commands inside it with `nbox`, manage sandboxes with
`manage-nbox`.

## Install

```sh
cargo install --path .
```

Requires: `podman`.

## Usage

First you need to build an image, then create a sandbox for your project, then
run commands inside it:

```sh
manage-nbox build              # pick and build a Containerfile from images/
manage-nbox create ~/proj/foo  # create a sandbox for the project
cd ~/proj/foo
nbox bash                      # run bash inside the sandbox
nbox make test                 # run any command
```

Other management commands exist, see `manage-nbox --help`.

### Mounts

By default `nbox` will mount only the project directory but you can mount
additional directories and files with `manage-nbox mount`.

By default everything is be mounted RW, except `.git` directories found in the
mounts which are mounted RO. You can optionally mount everything RO with `--ro`
flag.

If the set of mounts changes, `nbox` will refuse to run and ask you to
`manage-nbox stop` the sandbox and the new mounts will be re-injected on the
next `nbox` invocation. This includes new `.git` that appear in the mounts,
since they need to be RO and hence effectively change the set of mounts.

### Multicall symlinks

Motivating usecase: Your editor has a setting for a single binary path for an
LSP server, for example Emacs has `lsp-clients-clangd-executable`. You want to
run the LSP server in `nbox`. You can't pass `"nbox clangd"` for the executable
because then Emacs will try to invoke it literally as an executable with a space
in the name and it won't work.

So for cases that need a fixed binary path, `nbox` supports Busybox-style
multicall symlinks:

```sh
manage-nbox bin clangd
# Equivalent to nbox clangd:
$XDG_CONFIG_HOME/nbox/bin/nbox-clangd
```

## Security

Security is provided by a Podman container, it's up to you to decide if this
boundary is strong enough to run untrusted code. See `config_freeze_tests` in
[e2e.rs](./tests/e2e.rs) for the expected Podman command-line that the sandbox
will use.

Only the project directory and explicit mounts are inside the sandbox, so any
code running inside the sandbox won't be able to access any credentials you have
on your machine. (Unless you put credentials in them...)

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
sandbox and run them with `nbox rust-analyzer` (or use the `nbox-rust-analyzer`
multicall symlink).

To make a sandboxed directory trustworthy again you need to kill the sandbox
(`manage-nbox delete`), then `git clean -xdf`.

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
