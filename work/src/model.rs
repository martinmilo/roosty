#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum Kind {
    Http,
    Job,
    Mail,
    Schedule,
    Event,
    Cli,
}

impl Kind {
    pub(crate) const ALL: [Self; 6] = [
        Self::Http,
        Self::Job,
        Self::Mail,
        Self::Schedule,
        Self::Event,
        Self::Cli,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Job => "job",
            Self::Mail => "mail",
            Self::Schedule => "schedule",
            Self::Event => "event",
            Self::Cli => "cli",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct EntrypointRef {
    pub(crate) kind: Kind,
    pub(crate) name: String,
}

impl EntrypointRef {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let (kind, name) = text
            .split_once(' ')
            .ok_or_else(|| format!("expected '<kind> <name>', got {text:?}"))?;
        let kind = Kind::parse(kind).ok_or_else(|| format!("unknown entrypoint kind {kind:?}"))?;
        if name.trim().is_empty() {
            return Err("entrypoint name must not be empty".into());
        }
        Ok(Self {
            kind,
            name: name.to_owned(),
        })
    }
}

impl std::fmt::Display for EntrypointRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.kind.as_str(), self.name)
    }
}

#[derive(Debug)]
pub(crate) struct Project {
    pub(crate) name: String,
    pub(crate) stack: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) watch_out: Vec<String>,
    pub(crate) operations: Vec<(String, Operation)>,
    pub(crate) environments: Vec<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct Operation {
    pub(crate) command: String,
    pub(crate) note: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Process {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) group: Option<String>,
    pub(crate) entrypoints: Vec<Located<EntrypointRef>>,
    pub(crate) files: Vec<Located<CodeFile>>,
    pub(crate) try_text: Option<String>,
    pub(crate) outcomes: Vec<(String, Outcome)>,
    pub(crate) why: Vec<String>,
    pub(crate) checked: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Located<T> {
    // Retain the original YAML key when other invalid list items are discarded.
    pub(crate) key: String,
    pub(crate) value: T,
}

#[derive(Debug)]
pub(crate) struct CodeFile {
    pub(crate) path: String,
    pub(crate) note: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Outcome {
    pub(crate) when: String,
    pub(crate) then: String,
}

#[derive(Debug)]
pub(crate) struct Parsed<T> {
    // Keep valid fields available to validation, even when another field is invalid.
    pub(crate) value: Option<T>,
    pub(crate) problems: Vec<String>,
}

impl<T> Parsed<T> {
    pub(crate) fn failed(problem: String) -> Self {
        Self {
            value: None,
            problems: vec![problem],
        }
    }

    pub(crate) fn valid(&self) -> Option<&T> {
        if self.problems.is_empty() {
            self.value.as_ref()
        } else {
            None
        }
    }
}
