use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::model::{DiffFile, FilePatch};

/// Apply `.tuicrignore` rules from the repository root to a diff file set.
pub fn filter_diff_files(repo_root: &Path, diff_files: Vec<DiffFile>) -> Vec<DiffFile> {
    record_generated(
        repo_root,
        diff_files.iter().map(|f| f.display_path().as_path()),
    );
    let mut matcher = Matcher::new(repo_root);
    diff_files
        .into_iter()
        .filter(|file| !matcher.is_ignored(file.display_path()))
        .collect()
}

/// Apply `.tuicrignore` rules from the repository root to raw file patches,
/// before hunks are parsed and highlighted, so an ignored file never pays
/// the syntax-highlighting cost (a large minified bundle otherwise stalls
/// PR open even though it is excluded from the review).
pub fn filter_file_patches(repo_root: &Path, patches: Vec<FilePatch>) -> Vec<FilePatch> {
    record_generated(repo_root, patches.iter().filter_map(|p| p.display_path()));
    let mut matcher = Matcher::new(repo_root);
    patches
        .into_iter()
        .filter(|patch| {
            patch
                .display_path()
                .is_none_or(|path| !matcher.is_ignored(path))
        })
        .collect()
}

/// Apply `.tuicrignore` (and `.gitignore`, for `!`-unignore patterns) rules to a
/// list of paths. Used by the cheap status probe to verify that survives the
/// ignore filter without paying the full-diff cost.
pub fn filter_paths(repo_root: &Path, paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut matcher = Matcher::new(repo_root);
    paths
        .into_iter()
        .filter(|p| !matcher.is_ignored(p))
        .collect()
}

pub fn has_tuicrignore(repo_root: &Path) -> bool {
    repo_root.join(".tuicrignore").is_file()
}

/// `.tuicrignore` at the repository root decides first. Otherwise the path
/// is walked the way git walks it: top-down, each directory checked before
/// anything inside it, so an excluded directory hides its contents and its
/// own `.gitignore` is never read. Each component is matched against the
/// `.gitignore` files of its ancestors, deepest first; the first file with a
/// matching rule decides.
struct Matcher<'a> {
    repo_root: &'a Path,
    tuicrignore: Option<Gitignore>,
    /// `.gitignore` per repo-relative directory, loaded on first use.
    gitignores: HashMap<PathBuf, Option<Gitignore>>,
}

impl<'a> Matcher<'a> {
    fn new(repo_root: &'a Path) -> Self {
        Self {
            repo_root,
            tuicrignore: load(repo_root, &repo_root.join(".tuicrignore")),
            gitignores: HashMap::new(),
        }
    }

    fn is_ignored(&mut self, path: &Path) -> bool {
        if let Some(decided) = self.tuicrignore.as_ref().and_then(|m| {
            match m.matched_path_or_any_parents(path, false) {
                Match::None => None,
                matched => Some(matched.is_ignore()),
            }
        }) {
            return decided;
        }
        // Diff paths are repo-relative; anything else (`..`, absolute) would
        // read `.gitignore` files outside the repository.
        if !path.components().all(|c| matches!(c, Component::Normal(_))) {
            return false;
        }
        let mut prefix = PathBuf::new();
        let mut components = path.components().peekable();
        while let Some(component) = components.next() {
            prefix.push(component);
            let is_dir = components.peek().is_some();
            if self.excludes(&prefix, is_dir) {
                return true;
            }
        }
        false
    }

    /// Whether the `.gitignore` files above `path` exclude it, the deepest
    /// file with a matching rule deciding.
    fn excludes(&mut self, path: &Path, is_dir: bool) -> bool {
        for dir in path.ancestors().skip(1) {
            let repo_root = self.repo_root;
            let gitignore = self.gitignores.entry(dir.to_path_buf()).or_insert_with(|| {
                let dir = repo_root.join(dir);
                let file = dir.join(".gitignore");
                // Git refuses to follow an in-tree `.gitignore` symlink.
                (!file.is_symlink()).then(|| load(&dir, &file)).flatten()
            });
            let (Some(gitignore), Ok(relative)) = (gitignore.as_ref(), path.strip_prefix(dir))
            else {
                continue;
            };
            match gitignore.matched(relative, is_dir) {
                Match::None => {}
                matched => return matched.is_ignore(),
            }
        }
        false
    }
}

/// Diff paths last seen with `linguist-generated` set. Every diff passes
/// through the filters above before its files reach the review session,
/// which marks these reviewed when it first sees them.
static GENERATED: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

pub fn is_generated(path: &Path) -> bool {
    GENERATED.lock().is_ok_and(|g| g.contains(path))
}

fn record_generated<'p>(repo_root: &Path, paths: impl IntoIterator<Item = &'p Path>) {
    let paths: Vec<&Path> = paths.into_iter().collect();
    let generated = generated(repo_root, paths.iter().copied());
    if let Ok(mut recorded) = GENERATED.lock() {
        for path in paths {
            if generated.contains(path) {
                recorded.insert(path.to_path_buf());
            } else {
                recorded.remove(path);
            }
        }
    }
}

/// Paths whose `linguist-generated` attribute is set, the way GitHub
/// collapses them in a PR diff:
/// https://github.com/github-linguist/linguist/blob/main/docs/overrides.md#generated-code
fn generated<'p>(repo_root: &Path, paths: impl IntoIterator<Item = &'p Path>) -> HashSet<PathBuf> {
    let mut input = Vec::new();
    for path in paths {
        input.extend_from_slice(path.as_os_str().as_encoded_bytes());
        input.push(0);
    }
    if input.is_empty() {
        return HashSet::new();
    }
    let Ok(mut child) = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["check-attr", "-z", "--stdin", "linguist-generated"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return HashSet::new();
    };
    // git answers as it reads, so feeding it from this thread could block
    // on a full stdout pipe.
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output();
    let _ = writer.join();
    let Some(output) = output.ok().filter(|o| o.status.success()) else {
        return HashSet::new();
    };
    output
        .stdout
        .split(|b| *b == 0)
        .collect::<Vec<_>>()
        .chunks_exact(3)
        .filter(|record| matches!(record[2], b"set" | b"true"))
        .map(|record| PathBuf::from(String::from_utf8_lossy(record[0]).into_owned()))
        .collect()
}

fn load(root: &Path, file: &Path) -> Option<Gitignore> {
    if !file.is_file() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(root);
    let _ = builder.add(file);
    builder.build().ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::*;
    use crate::model::FileStatus;

    fn make_diff_file(path: &str) -> DiffFile {
        DiffFile {
            old_path: None,
            new_path: Some(PathBuf::from(path)),
            status: FileStatus::Modified,
            hunks: Vec::new(),
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
            content_hash: 0,
        }
    }

    #[test]
    fn keeps_all_files_when_tuicrignore_is_missing() {
        let dir = tempdir().expect("failed to create temp dir");
        let files = vec![
            make_diff_file("src/main.rs"),
            make_diff_file("target/debug/app"),
        ];

        let filtered = filter_diff_files(dir.path(), files);

        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn filters_matching_files() {
        let dir = tempdir().expect("failed to create temp dir");
        let ignore_path = dir.path().join(".tuicrignore");
        fs::write(&ignore_path, "target/\n*.lock\n").expect("failed to write .tuicrignore");

        let files = vec![
            make_diff_file("src/main.rs"),
            make_diff_file("target/debug/app"),
            make_diff_file("Cargo.lock"),
        ];

        let filtered = filter_diff_files(dir.path(), files);
        let kept_paths: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept_paths, vec!["src/main.rs"]);
    }

    #[test]
    fn supports_unignore_rules() {
        let dir = tempdir().expect("failed to create temp dir");
        let ignore_path = dir.path().join(".tuicrignore");
        fs::write(&ignore_path, "generated/\n!generated/keep.rs\n")
            .expect("failed to write .tuicrignore");

        let files = vec![
            make_diff_file("generated/drop.rs"),
            make_diff_file("generated/keep.rs"),
            make_diff_file("src/main.rs"),
        ];

        let filtered = filter_diff_files(dir.path(), files);
        let kept_paths: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept_paths, vec!["generated/keep.rs", "src/main.rs"]);
    }

    #[test]
    fn respects_gitignore() {
        let dir = tempdir().expect("failed to create temp dir");
        let gitignore = dir.path().join(".gitignore");
        fs::write(&gitignore, "target/\n*.log\n").expect("failed to write .gitignore");

        let files = vec![
            make_diff_file("src/main.rs"),
            make_diff_file("target/debug/app"),
            make_diff_file("build.log"),
        ];

        let filtered = filter_diff_files(dir.path(), files);
        let kept: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept, vec!["src/main.rs"]);
    }

    #[test]
    fn tuicrignore_overrides_gitignore() {
        let dir = tempdir().expect("failed to create temp dir");
        let gitignore = dir.path().join(".gitignore");
        fs::write(&gitignore, "*.lock\n").expect("failed to write .gitignore");
        let tuicrignore = dir.path().join(".tuicrignore");
        fs::write(&tuicrignore, "!Cargo.lock\n").expect("failed to write .tuicrignore");

        let files = vec![
            make_diff_file("Cargo.lock"),
            make_diff_file("yarn.lock"),
            make_diff_file("src/lib.rs"),
        ];

        let filtered = filter_diff_files(dir.path(), files);
        let kept: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        // Cargo.lock is un-ignored by .tuicrignore, yarn.lock stays ignored
        assert_eq!(kept, vec!["Cargo.lock", "src/lib.rs"]);
    }

    #[test]
    fn gitignore_alone_filters_without_tuicrignore() {
        let dir = tempdir().expect("failed to create temp dir");
        let gitignore = dir.path().join(".gitignore");
        fs::write(&gitignore, "dist/\n").expect("failed to write .gitignore");

        let files = vec![
            make_diff_file("src/index.ts"),
            make_diff_file("dist/bundle.js"),
        ];

        let filtered = filter_diff_files(dir.path(), files);
        let kept: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept, vec!["src/index.ts"]);
    }

    #[test]
    fn nested_gitignore_reincludes_what_the_root_one_ignores() {
        let dir = tempdir().expect("failed to create temp dir");
        fs::write(dir.path().join(".gitignore"), "**/packages/*\n")
            .expect("failed to write .gitignore");
        fs::create_dir(dir.path().join("ui")).expect("failed to create ui/");
        fs::write(dir.path().join("ui/.gitignore"), "!packages/*\n")
            .expect("failed to write ui/.gitignore");

        let files = vec![
            make_diff_file("ui/packages/utils/src/api.ts"),
            make_diff_file("server/packages/Newtonsoft.Json/lib.dll"),
        ];

        let filtered = filter_diff_files(dir.path(), files);
        let kept: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept, vec!["ui/packages/utils/src/api.ts"]);
    }

    #[test]
    fn nested_gitignore_ignores_relative_to_its_directory() {
        let dir = tempdir().expect("failed to create temp dir");
        fs::create_dir(dir.path().join("web")).expect("failed to create web/");
        fs::write(dir.path().join("web/.gitignore"), "/dist\n")
            .expect("failed to write web/.gitignore");

        let files = vec![
            make_diff_file("web/dist/bundle.js"),
            make_diff_file("dist/release.txt"),
        ];

        let filtered = filter_diff_files(dir.path(), files);
        let kept: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept, vec!["dist/release.txt"]);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinked_gitignore() {
        let dir = tempdir().expect("failed to create temp dir");
        fs::create_dir(dir.path().join("sub")).expect("failed to create sub/");
        fs::write(dir.path().join("rules"), "*.log\n").expect("failed to write rules");
        std::os::unix::fs::symlink("../rules", dir.path().join("sub/.gitignore"))
            .expect("failed to link sub/.gitignore");
        std::os::unix::fs::symlink("rules", dir.path().join(".gitignore"))
            .expect("failed to link .gitignore");

        let files = vec![make_diff_file("sub/a.log"), make_diff_file("b.log")];

        let filtered = filter_diff_files(dir.path(), files);
        let kept: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept, vec!["sub/a.log", "b.log"]);
    }

    #[test]
    fn tuicrignore_overrides_nested_gitignore() {
        let dir = tempdir().expect("failed to create temp dir");
        fs::create_dir(dir.path().join("web")).expect("failed to create web/");
        fs::write(dir.path().join("web/.gitignore"), "*.lock\n")
            .expect("failed to write web/.gitignore");
        fs::write(dir.path().join(".tuicrignore"), "!web/yarn.lock\n")
            .expect("failed to write .tuicrignore");

        let files = vec![make_diff_file("web/yarn.lock")];

        assert_eq!(filter_diff_files(dir.path(), files).len(), 1);
    }

    #[test]
    fn nested_gitignore_cannot_reinclude_inside_an_excluded_directory() {
        let dir = tempdir().expect("failed to create temp dir");
        fs::write(dir.path().join(".gitignore"), "logs/\n").expect("failed to write .gitignore");
        fs::create_dir(dir.path().join("logs")).expect("failed to create logs/");
        fs::write(dir.path().join("logs/.gitignore"), "!keep.txt\n")
            .expect("failed to write logs/.gitignore");

        let files = vec![make_diff_file("logs/keep.txt")];

        assert!(filter_diff_files(dir.path(), files).is_empty());
    }

    #[test]
    fn directory_reinclude_does_not_reinclude_files_inside_it() {
        let dir = tempdir().expect("failed to create temp dir");
        fs::write(dir.path().join(".gitignore"), "*.log\n").expect("failed to write .gitignore");
        fs::create_dir(dir.path().join("sub")).expect("failed to create sub/");
        fs::write(dir.path().join("sub/.gitignore"), "!sub2/\n")
            .expect("failed to write sub/.gitignore");

        let files = vec![make_diff_file("sub/sub2/a.log")];

        assert!(filter_diff_files(dir.path(), files).is_empty());
    }

    #[test]
    fn does_not_read_gitignore_outside_the_repository() {
        let outer = tempdir().expect("failed to create temp dir");
        fs::write(outer.path().join(".gitignore"), "*\n").expect("failed to write .gitignore");
        let repo = outer.path().join("repo");
        fs::create_dir(&repo).expect("failed to create repo/");

        let files = vec![make_diff_file("../escape.rs")];

        assert_eq!(filter_diff_files(&repo, files).len(), 1);
    }

    #[test]
    fn handles_deleted_file_paths() {
        let dir = tempdir().expect("failed to create temp dir");
        let ignore_path = dir.path().join(".tuicrignore");
        fs::write(&ignore_path, "generated/\n").expect("failed to write .tuicrignore");

        let deleted = DiffFile {
            old_path: Some(PathBuf::from("generated/old.txt")),
            new_path: None,
            status: FileStatus::Deleted,
            hunks: Vec::new(),
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
            content_hash: 0,
        };
        let kept = make_diff_file("src/lib.rs");

        let filtered = filter_diff_files(dir.path(), vec![deleted, kept]);
        let kept_paths: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept_paths, vec!["src/lib.rs"]);
    }

    fn git_repo() -> tempfile::TempDir {
        let dir = tempdir().expect("failed to create temp dir");
        let status = std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(dir.path())
            .status()
            .expect("failed to run git init");
        assert!(status.success());
        dir
    }

    #[test]
    fn records_linguist_generated_files() {
        let dir = git_repo();
        fs::write(
            dir.path().join(".gitattributes"),
            "*.pb.go linguist-generated\ngen/** linguist-generated=true\ngen/keep.rs -linguist-generated\n",
        )
        .expect("failed to write .gitattributes");

        let files = vec![
            make_diff_file("api/gen_test.pb.go"),
            make_diff_file("gen/gen_test.rs"),
            make_diff_file("gen/keep.rs"),
            make_diff_file("src/gen_test.rs"),
        ];

        assert_eq!(filter_diff_files(dir.path(), files).len(), 4);
        assert!(is_generated(Path::new("api/gen_test.pb.go")));
        assert!(is_generated(Path::new("gen/gen_test.rs")));
        assert!(!is_generated(Path::new("gen/keep.rs")));
        assert!(!is_generated(Path::new("src/gen_test.rs")));
    }

    #[test]
    fn session_marks_generated_files_reviewed_once() {
        let dir = git_repo();
        fs::write(
            dir.path().join(".gitattributes"),
            "*.gen.rs linguist-generated\n",
        )
        .expect("failed to write .gitattributes");
        let generated = make_diff_file("src/session_test.gen.rs");
        let path = generated.display_path().clone();
        let files = filter_diff_files(dir.path(), vec![generated]);

        let mut session = crate::model::ReviewSession::new(
            dir.path().to_path_buf(),
            "HEAD".to_string(),
            None,
            crate::model::SessionDiffSource::WorkingTree,
        );
        session.add_diff_file(&files[0]);
        assert!(session.is_file_reviewed(&path));

        session.get_file_mut(&path).unwrap().reviewed = false;
        session.add_diff_file(&files[0]);
        assert!(!session.is_file_reviewed(&path));
    }
}
