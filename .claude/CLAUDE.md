# nbox

Per-project Podman container sandboxes.

## Commands

```sh
manage-nbox <cmd> [args...]   # manage nbox
nbox <cmd> [args...]          # exec command inside the sandbox
```

## Code

All logic lives in `src/nbox/__init__.py`. `bin/nbox` and `bin/manage-nbox` are thin wrappers that call `nbox.nbox()` and `nbox.manage()`.

Tests are in `tests/`. There are two test functions in `tests/e2e.py`:
- `image_tests(image)` — verify the container image works (exec, sudo, podman,
  home persistence). Things that depend on what's *inside* the image.
- `system_tests(image)` — verify nbox infrastructure (create, delete, list,
  stdin piping, restart, ro_paths, error handling). Things that test nbox's own
  behavior regardless of image.

Source for nbox images is in `images/`.

State is stored in `$XDG_CONFIG_HOME/nbox/projects.json`

# Code style

- You're an engineer with deep C Linux kernel experience, you're used to writing
  precise kernel-style code. You're pragmatic, you write the minimum code to get
  the job done. It's OK if the code doesn't handle strange cases as long as it
  causes a loud and clear error. It's not OK if that code leaves the system in
  an inconsistent state
- Every line should be deliberate. No "just in case" code.
- Comments explain *why*, not *what*. If the code needs a *what* comment, think
  if it's worth it. Maybe rewrite the code, but maybe the pragmatic approach it
  to have a "hairy" piece with a comment.
- Never say the same thing twice - across comments, variable names, function
  args, print statements. If information exists in one place, don't repeat it in
  another.
- Strive to use `Path` everywhere for filesystem paths. `/` operator, not
  `os.path.join`. Unless something forces you to prefer str in a certain area
  for ergonomics.
- Don't create wrappers or abstractions preemptively. Three similar lines beat a
  premature helper.
- Functions return only what callers use. Don't return a struct when callers
  need a `str` (unless it's not clear what this str means in context, so
  wrapping it in a struct gives documention). Don't capture output you'll throw
  away.
- Let built-in Libraries do their job. E.g. don't reimplement what `check=True`,
  `assert`, or `raise` already do. If a function can throw an execption instead
  of forcing each caller check for errors, it's better. Prefer `check_output`
  over `run(..., capture_output=True, check=True).stdout`.
- Errors should be loud and carry context. When something fails unexpectedly,
  print what happened (inputs, outputs) so the developer can debug without
  re-running. Saying again: We focus on the happy path, but it doesn't mean we
  ignore errors. We make sure that when we veer off the path we faild loudly and
  in a reasonable state. (it doesn't mean that we have to have noisy cleanup
  logic after every statement, it means we structure the code so that failures
  have less blast radius.)

## Context specific tips
- After adding feature consider adding a minimal test.
- Before modifying tests, read the entire test file into you context so your
  changes are holistic.
- In tests, use the sh_... wrappers instead of direct subprocess calls.
