//! JSON-first startup inventory and management. Windows access stays behind `Backend`.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub mod management;

#[cfg(windows)]
pub mod native;

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    #[default]
    User,
    Machine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Run,
    Run32,
    Startup,
    Task,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub scope: Scope,
    pub source: Source,
    pub locator: String,
    pub path: String,
    pub arguments: String,
    pub working_directory: Option<String>,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub startup_approved_state: Option<u32>,
    pub elevated: bool,
    pub delay_seconds: Option<u32>,
    pub priority: Option<u32>,
    pub boot: bool,
}

impl Entry {
    pub fn new(name: &str, scope: Scope, source: Source, locator: &str) -> Self {
        Self {
            id: format!(
                "{scope:?}:{source:?}:{}",
                locator.replace('%', "%25").replace('#', "%23")
            )
            .to_lowercase(),
            name: name.into(),
            scope,
            source,
            locator: locator.into(),
            path: String::new(),
            arguments: String::new(),
            working_directory: None,
            enabled: Some(true),
            startup_approved_state: None,
            elevated: false,
            delay_seconds: None,
            priority: None,
            boot: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryWarning {
    pub scope: Option<Scope>,
    pub name: Option<String>,
    pub message: String,
}
impl InventoryWarning {
    pub fn affects(&self, scope: Scope, name: &str) -> bool {
        self.scope.is_none_or(|s| s == scope)
            && self
                .name
                .as_deref()
                .is_none_or(|n| n.to_lowercase() == name.to_lowercase())
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub entries: Vec<Entry>,
    pub warnings: Vec<InventoryWarning>,
}
impl Inventory {
    pub fn read_startup_approval(&mut self, entry: &mut Entry, data: &[u8]) {
        entry.startup_approved_state = data
            .get(..4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")));
        match startup_enabled(data) {
            Ok(enabled) => entry.enabled = Some(enabled),
            Err(error) => {
                entry.enabled = None;
                self.warn(
                    Some(entry.scope),
                    Some(&entry.name),
                    format!("{}: {error}", entry.id),
                );
            }
        }
    }
    pub fn warn(&mut self, scope: Option<Scope>, name: Option<&str>, message: impl Into<String>) {
        self.warnings.push(InventoryWarning {
            scope,
            name: name.map(str::to_owned),
            message: message.into(),
        });
    }
}

pub trait Backend {
    fn inventory(&self) -> Result<Inventory, String>;
    fn target_exists(&self, path: &str) -> bool;
    fn ensure_task_absent(&self, _locator: &str) -> Result<(), String> {
        Ok(())
    }
    fn expand(&self, value: &str) -> String {
        value.into()
    }
    fn task_locator(&self, scope: Scope, name: &str) -> String {
        format!(r"\Lineup\{scope:?}\{name}")
    }
    fn is_elevated(&self) -> Result<bool, String> {
        Ok(false)
    }
    fn write(&mut self, _entry: &Entry) -> Result<(), String> {
        Err("writes unavailable".into())
    }
    fn delete(&mut self, _entry: &Entry) -> Result<(), String> {
        Err("deletes unavailable".into())
    }
    fn start(&self, _entry: &Entry) -> Result<u32, String> {
        Err("launch unavailable".into())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub inventory: Inventory,
    pub missing_targets: Vec<String>,
    pub duplicates: Vec<Vec<String>>,
}

pub fn check(backend: &impl Backend) -> Result<Health, String> {
    let inventory = backend.inventory()?;
    let mut missing_targets = Vec::new();
    let mut targets: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in &inventory.entries {
        if !backend.target_exists(&entry.path) {
            missing_targets.push(entry.id.clone());
        }
        if !entry.path.is_empty() {
            targets
                .entry(entry.path.replace('/', "\\").to_lowercase())
                .or_default()
                .push(entry.id.clone());
        }
    }
    Ok(Health {
        inventory,
        missing_targets,
        duplicates: targets.into_values().filter(|ids| ids.len() > 1).collect(),
    })
}

/// Preserve the raw argument tail: splitting it into words would change Windows quoting.
pub fn split_command(command: &str) -> (String, String) {
    let command = command.trim();
    if let Some(quoted) = command.strip_prefix('"')
        && let Some(end) = quoted.find('"')
    {
        return (quoted[..end].into(), quoted[end + 1..].trim_start().into());
    }
    let lower = command.to_ascii_lowercase();
    let end = lower
        .match_indices(".exe")
        .find_map(|(i, _)| {
            let end = i + 4;
            (command[end..].is_empty() || command[end..].starts_with(char::is_whitespace))
                .then_some(end)
        })
        .unwrap_or_else(|| command.find(char::is_whitespace).unwrap_or(command.len()));
    (command[..end].into(), command[end..].trim_start().into())
}

/// Decode observed StartupApproved states; do not extrapolate parity to unknown values.
pub fn startup_enabled(data: &[u8]) -> Result<bool, String> {
    if data.len() < 12 {
        return Err("truncated StartupApproved record".into());
    }
    let state = u32::from_le_bytes(data[..4].try_into().expect("four bytes"));
    match state {
        2 | 4 | 6 | 8 => Ok(true),
        1 | 3 | 5 | 7 | 9 => Ok(false),
        _ => Err(format!("unknown StartupApproved state {state}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_action_suffixes_do_not_collide_with_literal_task_names() {
        let mut action = Entry::new("example", Scope::User, Source::Task, r"\Lineup\example");
        action.id.push_str("#2");
        let literal = Entry::new("example#2", Scope::User, Source::Task, r"\Lineup\example#2");
        assert_ne!(action.id, literal.id);
    }
    #[test]
    fn windows_commands_keep_the_argument_tail() {
        for (raw, path, args) in [
            (
                r#""C:\Program Files\Example\app.exe" --open "a b""#,
                r"C:\Program Files\Example\app.exe",
                r#"--open "a b""#,
            ),
            (
                r"C:\Program Files\Example\app.exe /quiet",
                r"C:\Program Files\Example\app.exe",
                "/quiet",
            ),
            ("notepad.exe", "notepad.exe", ""),
            (
                "rundll32.exe example.dll,Start",
                "rundll32.exe",
                "example.dll,Start",
            ),
        ] {
            assert_eq!(split_command(raw), (path.into(), args.into()));
        }
    }
    #[test]
    fn approval_inventory_decodes_known_states_and_keeps_raw_unknown_values() {
        struct Approval(Vec<u8>);
        impl Backend for Approval {
            fn inventory(&self) -> Result<Inventory, String> {
                let mut out = Inventory::default();
                let mut entry = Entry::new("example", Scope::User, Source::Run, "example");
                entry.path = r"C:\Windows\notepad.exe".into();
                out.read_startup_approval(&mut entry, &self.0);
                out.entries.push(entry);
                Ok(out)
            }
            fn target_exists(&self, _: &str) -> bool {
                true
            }
        }
        for (state, expected) in [
            (1u32, Some(false)),
            (2, Some(true)),
            (3, Some(false)),
            (4, Some(true)),
            (5, Some(false)),
            (6, Some(true)),
            (7, Some(false)),
            (8, Some(true)),
            (9, Some(false)),
            (0, None),
            (42, None),
            (0x102, None),
        ] {
            let mut data = vec![0; 12];
            data[..4].copy_from_slice(&state.to_le_bytes());
            let backend = Approval(data);
            let health = check(&backend).unwrap();
            let entry = &health.inventory.entries[0];
            assert_eq!(entry.enabled, expected);
            assert_eq!(entry.startup_approved_state, Some(state));
            let json = serde_json::to_value(&health.inventory).unwrap();
            assert_eq!(json["entries"][0]["startupApprovedState"], state);
            assert_eq!(
                json["entries"][0]["enabled"],
                serde_json::to_value(expected).unwrap()
            );
            let manifest = management::parse_manifest(r#"{"schemaVersion":1,"entries":[{"name":"example","path":"C:\\Windows\\notepad.exe"}]}"#).unwrap();
            if let Some(enabled) = expected {
                assert!(health.inventory.warnings.is_empty());
                assert_eq!(
                    management::plan(&backend, &manifest).unwrap().entries[0].action,
                    if enabled {
                        management::Action::Unchanged
                    } else {
                        management::Action::Enable
                    }
                );
            } else {
                let warning = &health.inventory.warnings[0];
                assert_eq!(warning.scope, Some(Scope::User));
                assert_eq!(warning.name.as_deref(), Some("example"));
                assert!(warning.message.contains(&state.to_string()));
                assert!(management::plan(&backend, &manifest).is_err());
            }
        }
        for data in [vec![], vec![4], vec![4, 0, 0, 0]] {
            let health = check(&Approval(data)).unwrap();
            assert_eq!(health.inventory.entries[0].enabled, None);
            assert!(health.inventory.warnings[0].message.contains("truncated"));
        }
    }
    #[test]
    fn health_groups_case_insensitive_targets_and_reports_missing_files() {
        struct System;
        impl Backend for System {
            fn inventory(&self) -> Result<Inventory, String> {
                let mut a = Entry::new("one", Scope::User, Source::Run, "one");
                a.path = r"C:\Windows\notepad.exe".into();
                let mut b = Entry::new("two", Scope::Machine, Source::Startup, "two.lnk");
                b.path = r"c:/windows/NOTEPAD.EXE".into();
                Ok(Inventory {
                    entries: vec![
                        a,
                        b,
                        Entry::new("missing", Scope::User, Source::Run, "missing"),
                    ],
                    warnings: vec![],
                })
            }
            fn target_exists(&self, path: &str) -> bool {
                !path.is_empty()
            }
        }
        let health = check(&System).unwrap();
        assert_eq!(health.missing_targets, ["user:run:missing"]);
        assert_eq!(
            health.duplicates,
            [vec!["user:run:one", "machine:startup:two.lnk"]]
        );
    }
}
