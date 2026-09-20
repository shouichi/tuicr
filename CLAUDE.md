# shouichi-flavor

This is a personal fork of tuicr. It is never sent upstream; it is rebased onto
upstream `main`, so it stays as small as it can be and it does not touch
`README.md`, `docs/` or `CHANGELOG.md` — **those files still describe upstream
behavior and are wrong about the keys below.** This file is the only description
of what this fork changes.

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
