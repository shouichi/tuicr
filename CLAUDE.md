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
- `tuicr completions <shell>` prints a completion script (`clap_complete`).
- Gerrit verifies the server against the OS trust store (`rustls-platform-verifier`)
  instead of the bundled Mozilla roots, so a private CA trusted in the Keychain
  works. `GERRIT_CLIENT_CERT` / `GERRIT_CLIENT_KEY` (PEM paths, both or
  neither) present a client certificate for servers that require mutual TLS.
- A Gerrit change target takes a patch-set segment the way Gerrit's web UI
  writes it: `tuicr pr 141/7..8` (or the change URL ending in `/7..8`) diffs
  patch set 7 against 8, `141/8` opens patch set 8 against its parent, and
  `141/7..` diffs 7 against the current patch set. Threads on the base patch
  set show on the left. Left-side comments cannot be submitted in a
  comparison: Gerrit files them on the base patch set, which a review of the
  head revision cannot carry. A comparison keeps a review session of its own.

## Release

mise installs tuicr from `shouichi/tuicr`'s GitHub releases
(`"github:shouichi/tuicr" = "latest"` in `~/.config/mise/config.toml`), so a
change to this fork only reaches the shell once it is released. After
committing on `shouichi-flavor`:

```sh
git push fork shouichi-flavor
TAG="shouichi-flavor-$(date -u +%Y%m%d%H%M)-$(git rev-parse --short HEAD)"
git tag "$TAG" && git push fork "$TAG"
sleep 10  # let the run register
gh run watch -R shouichi/tuicr \
  "$(gh run list -R shouichi/tuicr -b "$TAG" -L1 --json databaseId -q '.[0].databaseId')"
```

`.github/workflows/shouichi-release.yml` builds `x86_64-unknown-linux-gnu` and
`aarch64-apple-darwin` (~5 min) and attaches them to a release of that tag.
The macOS job can sit in the queue and get cancelled when GitHub runs short of
arm64 runners; `gh run rerun --failed` it. Then:

```sh
mise upgrade github:shouichi/tuicr
```

Upstream's `release.yml` and `build_nix.yml` are disabled on the fork; the
former publishes to crates.io, the latter runs on every push.

mise picks the release with the highest version, splitting the tag on `-` and
comparing the pieces as numbers. The date-time piece therefore has to order
the releases on its own: two releases on one day would otherwise be ordered by
their short SHAs. It is UTC so that the order does not depend on where the tag
is cut. The same comparison picks among installed versions too, so an older
install that sorts higher shadows a newer one until it is uninstalled. mise
also holds back releases younger than `minimum_release_age`, which is why the
config excludes `github:shouichi/tuicr` from it. `Cargo.toml`'s version stays
at upstream's, so `tuicr --version` prints upstream's and the tag is what
identifies the build.
