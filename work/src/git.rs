use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

use crate::dates::Period;
use crate::entrypoints::Inventory;
use crate::repository::{ENTRYPOINTS_FILE, Repository};

#[derive(Debug)]
struct Commit {
    id: String,
    parents: Vec<String>,
    timestamp: i64,
    subject: String,
    files: HashSet<Vec<u8>>,
}

pub(crate) struct Git {
    root: PathBuf,
    prefix: Vec<u8>,
    commits: Vec<Commit>,
    dirty: HashSet<Vec<u8>>,
}

pub(crate) struct EntrypointChanges {
    pub(crate) added: Vec<String>,
    pub(crate) removed: Vec<String>,
}

pub(crate) struct Changes {
    pub(crate) first_day: String,
    pub(crate) entrypoints: Option<EntrypointChanges>,
    pub(crate) processes: Vec<(String, Vec<String>)>,
}

impl Changes {
    pub(crate) fn summary(&self) -> String {
        let entrypoints = match &self.entrypoints {
            Some(changes) => format!(
                "{} added, {} removed",
                changes.added.len(),
                changes.removed.len()
            ),
            None => "not compared".to_owned(),
        };
        format!(
            "entrypoints: {entrypoints}   processes: {} changed",
            self.processes.len()
        )
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.processes.is_empty()
            && self
                .entrypoints
                .as_ref()
                .is_none_or(|changes| changes.added.is_empty() && changes.removed.is_empty())
    }
}

impl Git {
    pub(crate) fn read(root: &Path) -> Result<Option<Self>, String> {
        let mut git = Self {
            root: root.to_owned(),
            prefix: Vec::new(),
            commits: Vec::new(),
            dirty: HashSet::new(),
        };
        let location = git.command(&["rev-parse", "--is-inside-work-tree"])?;
        if !location.status.success() {
            if String::from_utf8_lossy(&location.stderr).contains("not a git repository") {
                return Ok(None);
            }
            return Err(git_error(&location));
        }
        if location.stdout != b"true\n" {
            return Ok(None);
        }
        git.prefix = git.checked(&["rev-parse", "--show-prefix"])?;
        if git.prefix.last() == Some(&b'\n') {
            git.prefix.pop();
        }
        let head = git.command(&["rev-parse", "--verify", "--quiet", "HEAD"])?;
        if head.status.success() {
            let history = git.checked(&[
                "log",
                "--topo-order",
                "--full-history",
                "--root",
                "--format=%x00%H%x00%P%x00%ct%x00%s",
                "--encoding=UTF-8",
                "-z",
                "--name-only",
                "--no-renames",
                "--no-relative",
                "--diff-merges=first-parent",
                "--no-ext-diff",
                "--no-textconv",
                "--no-notes",
                "--no-decorate",
                "--no-show-signature",
                "HEAD",
                "--",
            ])?;
            git.commits = parse_history(&history)?;
        } else if head.status.code() != Some(1) {
            return Err(git_error(&head));
        }
        git.dirty = parse_status(&git.checked(&[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ])?)?;
        Ok(Some(git))
    }

    fn process(&self) -> Command {
        let mut command = Command::new("git");
        command.current_dir(&self.root).args([
            "--no-pager",
            "--no-optional-locks",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "color.ui=false",
        ]);
        // Git reads must not refresh the index, run a repository helper, or fetch
        // missing objects from a partial clone.
        command
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("LC_ALL", "C");
        command
    }

    fn command(&self, args: &[&str]) -> Result<Output, String> {
        self.process()
            .args(args)
            .output()
            .map_err(|error| format!("Cannot read Git repository: {error}"))
    }

    fn checked(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let output = self.command(args)?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(git_error(&output))
        }
    }

    // The path of a repository file as Git spells it. Only plain relative paths map to
    // one; a path that climbs out or starts at the root names nothing in the repository.
    fn path(&self, relative: &str) -> Result<Vec<u8>, String> {
        let mut path = self.prefix.clone();
        for component in Path::new(relative).components() {
            match component {
                Component::CurDir => continue,
                Component::Normal(name) => {
                    if !path.is_empty() && path.last() != Some(&b'/') {
                        path.push(b'/');
                    }
                    path.extend_from_slice(name.as_encoded_bytes());
                }
                _ => return Err(format!("{relative:?} is not a path inside the repository")),
            }
        }
        Ok(path)
    }

    fn inventory_at(&self, commit: &Commit) -> Result<Option<Inventory>, String> {
        let path = OsString::from_vec(self.path(ENTRYPOINTS_FILE)?);
        let tree = self
            .process()
            .args(["ls-tree", "-z", "--full-tree", &commit.id, "--"])
            .arg(path)
            .output()
            .map_err(|error| format!("Cannot read Git repository: {error}"))?;
        if !tree.status.success() {
            return Err(git_error(&tree));
        }
        if tree.stdout.is_empty() {
            return Ok(None);
        }
        // ls-tree -z writes `<mode> <type> <object>\t<path>\0`; without the tab it is not
        // an entry, and taking the whole line as the header would misread it silently.
        let Some(separator) = tree.stdout.iter().position(|byte| *byte == b'\t') else {
            return Err(format!(
                "Unexpected Git ls-tree output: {:?}",
                String::from_utf8_lossy(&tree.stdout)
            ));
        };
        let header = &tree.stdout[..separator];
        let fields: Vec<_> = header.split(|byte| *byte == b' ').collect();
        if fields.len() != 3 || fields[1] != b"blob" {
            return Ok(None);
        }
        let id = std::str::from_utf8(fields[2]).map_err(|_| "Invalid Git object id")?;
        let bytes = self.checked(&["cat-file", "blob", id])?;
        let text = String::from_utf8(bytes)
            .map_err(|_| format!("{ENTRYPOINTS_FILE} in Git is not UTF-8"))?;
        let inventory = Inventory::parse(&text);
        Ok(inventory.problems.is_empty().then_some(inventory))
    }

    pub(crate) fn changes(
        &self,
        repository: &Repository,
        period: &Period,
    ) -> Result<Changes, String> {
        // Do not stop a history walk at an old timestamp: descendants and branch
        // commits can have dates that run backwards.
        let baseline = self
            .commits
            .iter()
            .enumerate()
            .filter(|(_, commit)| commit.timestamp < period.start)
            // Equal dates keep the first (newest in history) snapshot.
            .max_by_key(|(index, commit)| (commit.timestamp, std::cmp::Reverse(*index)))
            .map(|(_, commit)| commit);
        let entrypoints = match (baseline, repository.inventory.as_ref()) {
            (Some(baseline), Some(now)) if now.problems.is_empty() => {
                self.inventory_at(baseline)?.map(|then| {
                    let then = then.references();
                    let now = now.references();
                    EntrypointChanges {
                        added: now.difference(&then).cloned().collect(),
                        removed: then.difference(&now).cloned().collect(),
                    }
                })
            }
            _ => None,
        };
        let mut recent: Vec<_> = self
            .commits
            .iter()
            .filter(|commit| period.contains(commit.timestamp))
            .collect();
        // Stable sorting preserves descendant-before-ancestor order for equal dates.
        recent.sort_by_key(|commit| std::cmp::Reverse(commit.timestamp));
        let mut processes = Vec::new();
        for record in &repository.processes {
            let Some(process) = record.parsed.valid() else {
                continue;
            };
            let paths: HashSet<_> = std::iter::once(record.file.as_str())
                .chain(process.files.iter().map(|item| item.value.path.as_str()))
                .filter_map(|path| self.path(path).ok())
                .collect();
            let subjects: Vec<_> = recent
                .iter()
                .filter(|commit| !commit.files.is_disjoint(&paths))
                .map(|commit| commit.subject.clone())
                .collect();
            if !subjects.is_empty() {
                processes.push((record.id.clone(), subjects));
            }
        }
        Ok(Changes {
            first_day: period.first_day.to_string(),
            entrypoints,
            processes,
        })
    }

    pub(crate) fn needs_attention(
        &self,
        repository: &Repository,
    ) -> Result<Vec<(String, String)>, String> {
        let commits: HashMap<_, _> = self
            .commits
            .iter()
            .map(|commit| (commit.id.as_str(), commit))
            .collect();
        let mut attention = Vec::new();
        for record in &repository.processes {
            let Some(process) = record.parsed.valid() else {
                continue;
            };
            let record_path = self.path(&record.file)?;
            if self.dirty.contains(&record_path) {
                continue;
            }
            let Some(last) = self
                .commits
                .iter()
                .find(|commit| commit.files.contains(&record_path))
            else {
                continue;
            };
            let mut ancestors = HashSet::new();
            let mut pending = vec![last.id.as_str()];
            while let Some(id) = pending.pop() {
                if ancestors.insert(id) {
                    if let Some(commit) = commits.get(id) {
                        pending.extend(commit.parents.iter().map(String::as_str));
                    }
                }
            }
            let newer_files: HashSet<_> = self
                .commits
                .iter()
                .filter(|commit| !ancestors.contains(commit.id.as_str()))
                .flat_map(|commit| commit.files.iter())
                .chain(self.dirty.iter())
                .collect();
            let files: BTreeSet<_> = process
                .files
                .iter()
                .map(|item| item.value.path.as_str())
                .collect();
            for file in files {
                if self
                    .path(file)
                    .is_ok_and(|path| newer_files.contains(&path))
                {
                    attention.push((record.id.clone(), file.to_owned()));
                }
            }
        }
        Ok(attention)
    }
}

fn git_error(output: &Output) -> String {
    format!(
        "Cannot read Git repository: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

fn parse_history(bytes: &[u8]) -> Result<Vec<Commit>, String> {
    let mut fields = bytes.split(|byte| *byte == 0).peekable();
    let mut commits = Vec::new();
    while fields.next().is_some() {
        if fields.peek().is_none() {
            break;
        }
        let mut text = || -> Result<String, String> {
            let field = fields.next().ok_or("Incomplete Git history")?;
            String::from_utf8(field.to_vec()).map_err(|_| "Git history is not UTF-8".into())
        };
        let id = text()?;
        let parents = text()?.split_whitespace().map(str::to_owned).collect();
        let timestamp = text()?.parse().map_err(|_| "Invalid Git commit date")?;
        let subject = text()?;
        let mut files = HashSet::new();
        let mut first_file = true;
        while let Some(mut file) = fields.next_if(|field| !field.is_empty()) {
            if first_file {
                file = file.strip_prefix(b"\n").unwrap_or(file);
                first_file = false;
            }
            files.insert(file.to_vec());
        }
        commits.push(Commit {
            id,
            parents,
            timestamp,
            subject,
            files,
        });
    }
    Ok(commits)
}

fn parse_status(bytes: &[u8]) -> Result<HashSet<Vec<u8>>, String> {
    let mut fields = bytes.split(|byte| *byte == 0);
    let mut files = HashSet::new();
    while let Some(field) = fields.next().filter(|field| !field.is_empty()) {
        if field.len() < 4 || field[2] != b' ' {
            return Err("Invalid Git status output".into());
        }
        files.insert(field[3..].to_vec());
        if field[..2]
            .iter()
            .any(|status| matches!(status, b'R' | b'C'))
        {
            files.insert(fields.next().ok_or("Incomplete Git rename")?.to_vec());
        }
    }
    Ok(files)
}
