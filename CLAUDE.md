# shouichi-flavor

This is a personal fork of tuicr. It is never sent upstream; it is rebased onto
upstream `main` and kept to a single commit on top of it, so it stays as small
as it can be, and it does not touch `README.md`, `docs/` or `CHANGELOG.md` —
**those files still describe upstream behavior and are wrong about the keys
below.** This file is the only description of what this fork changes.

## Keys

| Key | Action |
|-----|--------|
| `r` | Toggle hunk reviewed (upstream: file reviewed) |
| `R` | Toggle file reviewed (upstream: hunk reviewed) |
| `u` | Undo the last reviewed mark (new) |

## Behavior

- Every reviewed mark moves the cursor to the next hunk's first change, so
  holding `r` walks the whole review from change to change and ends with
  everything marked. Upstream stayed put unless the reviewed filter hid the row.
- Marking the last unreviewed hunk of a file marks the whole file reviewed and
  moves on to the next file's first change, with that file's name row on screen.
- Marking a file reviewed (`R`) moves on the same way.
- Unmarking never moves.
- `u` takes back the most recent reviewed mark, file or hunk, and puts the
  cursor back where that mark was made from. It can be pressed repeatedly to
  walk back through the marks. A file mark that came with a hunk's mark is taken
  back together with it.
- `r` pressed on a file name row (or anywhere else that is not a hunk) marks
  the first hunk below the cursor, rather than refusing.
- `}` lands on the file name row, so the file being entered is on screen.
- Any move to another file or hunk scrolls it to the top of the viewport: a
  file starts at its name row, a hunk at its header.
- Moving to another hunk (`]`, `[`, or a hunk mark) lands on the hunk's first
  added or removed line instead of the context above it.
- Opening a GitHub PR — `tuicr pr <target>`, or picking it from the PR or
  Sessions tab — inside a checkout of its repository runs `gh pr checkout`
  there first. If that fails (e.g. local changes in the way), the review still
  opens, with a warning.

## Release

`~/.zsh/10-tuicr.zsh` installs tuicr from `shouichi/tuicr`'s GitHub releases
with zinit's `from"gh-r"`, so a change to this fork only reaches the shell once
it is released. After committing on `shouichi-flavor`:

```sh
git push fork shouichi-flavor
TAG="shouichi-flavor-$(date +%Y%m%d)-$(git rev-parse --short HEAD)"
git tag "$TAG" && git push fork "$TAG"
sleep 10  # let the run register
gh run watch -R shouichi/tuicr \
  "$(gh run list -R shouichi/tuicr -b "$TAG" -L1 --json databaseId -q '.[0].databaseId')"
```

`.github/workflows/shouichi-release.yml` builds `x86_64-unknown-linux-gnu` and
`aarch64-apple-darwin` (~5 min) and attaches them to a release of that tag.
Then, **in the interactive shell** — `zinit` is a zsh function, so it cannot be
run from a tool:

```sh
zinit update shouichi/tuicr
```

Upstream's `release.yml` and `build_nix.yml` are disabled on the fork; the
former publishes to crates.io, the latter runs on every push.

The tag is date + short SHA so that cutting one never means looking up the
previous one. zinit does not compare version numbers: it scrapes the tag out of
`/releases/latest` and re-downloads when the asset URL differs from the one in
`._zinit/is_release`. So the tag only has to be new, and the release has to be
the one GitHub marks Latest — hence `make_latest` in the workflow.
`Cargo.toml`'s version stays at upstream's, so `tuicr --version` prints
upstream's and the tag is what identifies the build.
