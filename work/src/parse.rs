use std::collections::HashSet;

use saphyr::{LoadableYamlNode, YamlOwned};

use crate::model::{
    CodeFile, EntrypointRef, Located, Operation, Outcome, Parsed, Process, Project,
};

fn document(text: &str) -> Result<YamlOwned, String> {
    let mut documents =
        YamlOwned::load_from_str(text).map_err(|error| format!("cannot parse YAML: {error}"))?;
    if documents.len() != 1 {
        return Err("expected one YAML mapping document".into());
    }
    let value = documents.remove(0);
    if !matches!(value, YamlOwned::Mapping(_)) {
        return Err("expected a YAML mapping".into());
    }
    Ok(value)
}

#[derive(Default)]
struct Check {
    problems: Vec<String>,
}

fn key_path(parent: &str, key: &str) -> String {
    if parent.is_empty() {
        key.to_owned()
    } else {
        format!("{parent}.{key}")
    }
}

impl Check {
    fn problem(&mut self, key: &str, message: impl std::fmt::Display) {
        self.problems.push(format!("{key}: {message}"));
    }

    fn max_length(&mut self, text: &str, key: &str, limit: usize) {
        if text.chars().count() > limit {
            self.problem(key, format!("must have at most {limit} characters"));
        }
    }

    fn string(&mut self, value: &YamlOwned, key: &str, multiline: bool) -> Option<String> {
        match value.as_str() {
            Some(text) if multiline || !text.contains(['\n', '\r']) => Some(text.to_owned()),
            _ => {
                self.problem(
                    key,
                    if multiline {
                        "must be a string"
                    } else {
                        "must be a one-line string"
                    },
                );
                None
            }
        }
    }

    fn field(
        &mut self,
        node: &YamlOwned,
        key: &str,
        parent: &str,
        required: bool,
        multiline: bool,
    ) -> Option<String> {
        let path = key_path(parent, key);
        let Some(value) = node.as_mapping_get(key) else {
            if required {
                self.problem(&path, "is required");
            }
            return None;
        };
        let value = self.string(value, &path, multiline)?;
        if required && value.trim().is_empty() {
            self.problem(&path, "must not be empty");
            return None;
        }
        Some(value)
    }

    fn mapping<'a>(&mut self, value: &'a YamlOwned, key: &str) -> Vec<(String, &'a YamlOwned)> {
        let Some(mapping) = value.as_mapping() else {
            self.problem(key, "must be a map");
            return Vec::new();
        };
        mapping
            .iter()
            .filter_map(|(name, value)| {
                let location = if key.is_empty() {
                    "key".to_owned()
                } else {
                    format!("{key} key")
                };
                self.string(name, &location, false)
                    .map(|name| (name, value))
            })
            .collect()
    }

    fn record(&mut self, node: &YamlOwned, parent: &str, allowed_keys: &[&str]) {
        for (key, _) in self.mapping(node, parent) {
            if !allowed_keys.contains(&key.as_str()) {
                self.problem(&key_path(parent, &key), "unknown key");
            }
        }
    }

    fn map_field<'a>(&mut self, node: &'a YamlOwned, key: &str) -> Vec<(String, &'a YamlOwned)> {
        node.as_mapping_get(key)
            .map(|value| self.mapping(value, key))
            .unwrap_or_default()
    }

    fn sequence<'a>(&mut self, node: &'a YamlOwned, key: &str) -> &'a [YamlOwned] {
        let Some(value) = node.as_mapping_get(key) else {
            return &[];
        };
        if let Some(sequence) = value.as_sequence() {
            sequence
        } else {
            self.problem(key, "must be a list");
            &[]
        }
    }

    fn strings(&mut self, node: &YamlOwned, key: &str, multiline: bool) -> Vec<String> {
        self.sequence(node, key)
            .iter()
            .enumerate()
            .filter_map(|(index, value)| {
                self.string(value, &format!("{key}.{}", index + 1), multiline)
            })
            .collect()
    }
}

pub(crate) fn project(text: &str) -> Parsed<Project> {
    let node = match document(text) {
        Ok(node) => node,
        Err(error) => return Parsed::failed(error),
    };
    let mut check = Check::default();
    check.record(
        &node,
        "",
        &[
            "name",
            "stack",
            "description",
            "watch_out",
            "operations",
            "environments",
        ],
    );
    let name = check
        .field(&node, "name", "", true, false)
        .unwrap_or_default();
    let stack = check.field(&node, "stack", "", false, false);
    let description = check.field(&node, "description", "", false, false);
    let watch_out = check.strings(&node, "watch_out", false);
    if let Some(items) = node
        .as_mapping_get("watch_out")
        .and_then(YamlOwned::as_sequence)
    {
        if items.len() > 7 {
            check.problem("watch_out", "must have at most 7 items");
        }
        for (index, item) in items.iter().enumerate() {
            if let Some(text) = item.as_str() {
                check.max_length(text, &format!("watch_out.{}", index + 1), 160);
            }
        }
    }
    let mut operations = Vec::new();
    for (name, value) in check.map_field(&node, "operations") {
        let key = key_path("operations", &name);
        let (command, note) = if value.is_string() {
            (check.string(value, &key, false), None)
        } else if value.is_mapping() {
            check.record(value, &key, &["command", "note"]);
            (
                check.field(value, "command", &key, true, false),
                check.field(value, "note", &key, false, false),
            )
        } else {
            check.problem(&key, "must be a command string or a map");
            (None, None)
        };
        if let Some(note) = &note {
            check.max_length(note, &key_path(&key, "note"), 160);
        }
        if let Some(command) = command {
            operations.push((name, Operation { command, note }));
        }
    }
    let environments = check
        .map_field(&node, "environments")
        .into_iter()
        .filter_map(|(name, value)| {
            let key = key_path("environments", &name);
            let value = check.string(value, &key, false)?;
            check.max_length(&value, &key, 160);
            Some((name, value))
        })
        .collect();
    Parsed {
        value: Some(Project {
            name,
            stack,
            description,
            watch_out,
            operations,
            environments,
        }),
        problems: check.problems,
    }
}

pub(crate) fn process(text: &str) -> Parsed<Process> {
    let node = match document(text) {
        Ok(node) => node,
        Err(error) => return Parsed::failed(error),
    };
    let mut check = Check::default();
    check.record(
        &node,
        "",
        &[
            "name",
            "description",
            "group",
            "entrypoints",
            "files",
            "try",
            "outcomes",
            "why",
            "checked",
        ],
    );
    let name = check
        .field(&node, "name", "", true, false)
        .unwrap_or_default();
    let description = check
        .field(&node, "description", "", true, false)
        .unwrap_or_default();
    let group = check.field(&node, "group", "", false, false);
    let mut entrypoints = Vec::new();
    let mut seen = HashSet::new();
    for (index, value) in check.sequence(&node, "entrypoints").iter().enumerate() {
        let key = format!("entrypoints.{}", index + 1);
        let Some(text) = check.string(value, &key, false) else {
            continue;
        };
        match EntrypointRef::parse(&text) {
            Ok(reference) => {
                if !seen.insert(reference.clone()) {
                    check.problem(&key, format!("duplicate entrypoint {reference}"));
                }
                entrypoints.push(Located {
                    key,
                    value: reference,
                });
            }
            Err(error) => check.problem(&key, error),
        }
    }
    let mut files = Vec::new();
    for (index, value) in check.sequence(&node, "files").iter().enumerate() {
        let key = format!("files.{}", index + 1);
        let (path, note) = if value.is_string() {
            (check.string(value, &key, false), None)
        } else if value.is_mapping() {
            check.record(value, &key, &["path", "note"]);
            (
                check.field(value, "path", &key, true, false),
                check.field(value, "note", &key, false, false),
            )
        } else {
            check.problem(&key, "must be a path string or a map");
            (None, None)
        };
        if let Some(path) = path {
            files.push(Located {
                key: if value.is_mapping() {
                    key_path(&key, "path")
                } else {
                    key
                },
                value: CodeFile { path, note },
            });
        }
    }
    let try_text = check.field(&node, "try", "", false, true);
    let mut outcomes = Vec::new();
    for (name, value) in check.map_field(&node, "outcomes") {
        let key = key_path("outcomes", &name);
        if !value.is_mapping() {
            check.problem(&key, "must be a map");
            continue;
        }
        check.record(value, &key, &["when", "then"]);
        let when = check.field(value, "when", &key, true, false);
        let then = check.field(value, "then", &key, true, false);
        if let (Some(when), Some(then)) = (when, then) {
            outcomes.push((name, Outcome { when, then }));
        }
    }
    let why = check.strings(&node, "why", true);
    let checked = node.as_mapping_get("checked").and_then(|value| {
        if let Some(text) = value
            .as_str()
            .filter(|text| crate::dates::parse(text).is_some())
        {
            Some(text.to_owned())
        } else {
            check.problem("checked", "must be an existing date written YYYY-MM-DD");
            None
        }
    });
    Parsed {
        value: Some(Process {
            name,
            description,
            group,
            entrypoints,
            files,
            try_text,
            outcomes,
            why,
            checked,
        }),
        problems: check.problems,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_12_strings_and_mapping_order_are_preserved() {
        let parsed = project(
            "name: on\noperations:\n  z-last: run-last\n  a-first:\n    command: run-first\n    note: 'yes'\nenvironments:\n  z: one\n  a: two\n",
        );
        assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
        let project = parsed.value.unwrap();
        assert_eq!(project.name, "on");
        assert_eq!(project.operations[0].0, "z-last");
        assert_eq!(project.operations[1].1.note.as_deref(), Some("yes"));
        assert_eq!(project.environments[0].0, "z");
    }

    #[test]
    fn invalid_fields_do_not_discard_valid_references() {
        let parsed = process(
            "name: Export\ndescription: Exports orders.\nwhy: not-a-list\nentrypoints:\n  - job ExportOrders\nfiles:\n  - src/export.rs\n  - path: src/order.rs\n    note: []\noutcomes:\n  exported:\n    when: true\n    then: done\n",
        );
        for key in ["why", "files.2.note", "outcomes.exported.when"] {
            assert!(
                parsed
                    .problems
                    .iter()
                    .any(|problem| problem.starts_with(key)),
                "{key}: {:?}",
                parsed.problems
            );
        }
        let process = parsed.value.unwrap();
        assert_eq!(process.entrypoints[0].value.to_string(), "job ExportOrders");
        assert_eq!(process.files[0].value.path, "src/export.rs");
    }

    #[test]
    fn invalid_list_items_do_not_shift_later_diagnostic_positions() {
        let parsed = process(
            "name: Prune\ndescription: Remove old plants.\nentrypoints:\n  - false\n  - job Prune\n  - unknown Prune\n  - job Prune\nfiles:\n  - 17\n  - src/prune.rs\n  - {}\n  - path: src/garden.rs\n",
        );
        assert!(
            parsed
                .problems
                .iter()
                .any(|problem| { problem == "entrypoints.4: duplicate entrypoint job Prune" })
        );
        let process = parsed.value.unwrap();
        assert_eq!(process.entrypoints[0].key, "entrypoints.2");
        assert_eq!(process.entrypoints[1].key, "entrypoints.4");
        assert_eq!(process.files[0].key, "files.2");
        assert_eq!(process.files[1].key, "files.4.path");
    }

    #[test]
    fn literal_and_folded_text_have_yaml_semantics() {
        let parsed = process(
            "name: Check stock\ndescription: Check available stock.\ntry: |\n  First line.\n  Second line.\nwhy:\n  - >-\n    Keep inventory\n    accurate.\n",
        );
        let process = parsed.valid().unwrap();
        assert_eq!(
            process.try_text.as_deref(),
            Some("First line.\nSecond line.\n")
        );
        assert_eq!(process.why, ["Keep inventory accurate."]);
    }

    #[test]
    fn malformed_and_multiple_documents_are_rejected() {
        assert!(process("name: [unfinished").value.is_none());
        assert!(project("name: One\n---\nname: Two\n").value.is_none());
        assert!(project("[one, two]").value.is_none());
    }

    #[test]
    fn unknown_keys_are_reported_in_every_record_shape() {
        let project = project(
            "name: Inventory\nowner: someone\noperations:\n  export:\n    command: bin/export\n    environment: local\n",
        );
        assert_eq!(
            project.problems,
            [
                "owner: unknown key",
                "operations.export.environment: unknown key"
            ]
        );

        let process = process(
            "name: Export\ndescription: Export orders.\nretries: 3\nfiles:\n  - path: src/export.rs\n    purpose: Converts orders.\noutcomes:\n  ready:\n    when: processing finishes\n    then: download becomes available\n    because: a file was saved\n",
        );
        assert_eq!(
            process.problems,
            [
                "retries: unknown key",
                "files.1.purpose: unknown key",
                "outcomes.ready.because: unknown key"
            ]
        );
    }

    #[test]
    fn wrong_nested_shapes_name_their_paths_without_trailing_dots() {
        let project = project("name: Inventory\noperations:\n  export: []\n");
        assert_eq!(
            project.problems,
            ["operations.export: must be a command string or a map"]
        );
        let process = process(
            "name: Export\ndescription: Export orders.\nfiles: [17]\noutcomes:\n  ready: done\n",
        );
        assert_eq!(
            process.problems,
            [
                "files.1: must be a path string or a map",
                "outcomes.ready: must be a map"
            ]
        );
    }

    #[test]
    fn watch_out_limits_count_unicode_characters_and_all_list_items() {
        let at_limit = format!(
            "name: Inventory\nwatch_out:\n{}",
            format!("  - {}\n", "庭".repeat(160)).repeat(7)
        );
        let parsed = project(&at_limit);
        assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);

        let too_long = project(&format!(
            "name: Inventory\nwatch_out:\n  - {}\n",
            "庭".repeat(161)
        ));
        assert_eq!(
            too_long.problems,
            ["watch_out.1: must have at most 160 characters"]
        );

        let too_many = project(&format!("{at_limit}  - 17\n"));
        assert!(
            too_many
                .problems
                .iter()
                .any(|problem| problem == "watch_out: must have at most 7 items")
        );
        assert!(
            too_many
                .problems
                .iter()
                .any(|problem| problem.starts_with("watch_out.8:"))
        );
    }
}
