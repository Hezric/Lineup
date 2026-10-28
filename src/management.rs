use crate::{Backend, Entry, Inventory, InventoryWarning, Scope, Source};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    pub entries: Vec<DesiredEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesiredEntry {
    #[schemars(
        length(min = 1, max = 200),
        regex(pattern = r#"^(?!\.{1,2}$)(?!.*[ .]$)[^\\/:*?"<>|\u0000-\u001f]+$"#)
    )]
    pub name: String,
    #[schemars(regex(
        pattern = r#"^(?:[A-Za-z]:[\\/]|\\\\[^\\/]+[\\/][^\\/]+|%[^%]+%[\\/])[^"\u0000]*$"#
    ))]
    pub path: String,
    #[serde(default)]
    #[schemars(regex(pattern = r"^[^\u0000]*$"))]
    pub arguments: String,
    #[serde(default)]
    #[schemars(regex(
        pattern = r#"^(?:[A-Za-z]:[\\/]|\\\\[^\\/]+[\\/][^\\/]+|%[^%]+%[\\/])[^"\u0000]*$"#
    ))]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub elevated: bool,
    #[serde(default)]
    pub delay_seconds: Option<u32>,
    #[serde(default)]
    #[schemars(range(min = 0, max = 10))]
    pub priority: Option<u32>,
    #[serde(default = "yes")]
    pub enabled: bool,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct Problem {
    pub path: String,
    pub reason: String,
}
pub fn schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Manifest)).expect("schema serializes")
}
pub fn parse_manifest(raw: &str) -> Result<Manifest, Vec<Problem>> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
        vec![Problem {
            path: String::new(),
            reason: e.to_string(),
        }]
    })?;
    let validator = jsonschema::validator_for(&schema()).expect("generated schema is valid");
    let mut problems: Vec<_> = validator
        .iter_errors(&value)
        .map(|error| Problem {
            path: error.instance_path.to_string(),
            reason: error.to_string(),
        })
        .collect();
    if let Some(entries) = value.get("entries").and_then(|v| v.as_array()) {
        let mut names = std::collections::BTreeSet::new();
        for (i, entry) in entries.iter().enumerate() {
            if let Some(name) = entry.get("name").and_then(|v| v.as_str()) {
                let scope = entry
                    .get("scope")
                    .and_then(|v| v.as_str())
                    .unwrap_or("user");
                if !names.insert((scope, name.to_lowercase())) {
                    problems.push(Problem {
                        path: format!("/entries/{i}/name"),
                        reason: "duplicate name within scope (case-insensitive)".into(),
                    });
                }
            }
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    serde_json::from_value(value).map_err(|e| {
        vec![Problem {
            path: String::new(),
            reason: e.to_string(),
        }]
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Create,
    Update,
    Enable,
    Disable,
    Remove,
    Unchanged,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedEntry {
    pub name: String,
    pub scope: Scope,
    pub action: Action,
    pub source: Source,
    pub desired: Option<Entry>,
    pub remove: Vec<Entry>,
    pub needs_elevation: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub warnings: Vec<InventoryWarning>,
    pub entries: Vec<PlannedEntry>,
}

#[derive(Debug, Serialize)]
pub struct EntryResult {
    pub name: String,
    pub scope: Scope,
    pub status: String,
    pub error: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct ApplyReport {
    pub status: String,
    pub plan: Plan,
    pub results: Vec<EntryResult>,
}

fn desired_entry(backend: &impl Backend, desired: &DesiredEntry) -> Entry {
    let source = if desired.elevated
        || desired.delay_seconds.is_some()
        || desired.priority.is_some()
        || desired.working_directory.is_some()
    {
        Source::Task
    } else {
        Source::Run
    };
    let locator = if source == Source::Task {
        backend.task_locator(desired.scope, &desired.name)
    } else {
        desired.name.clone()
    };
    let mut entry = Entry::new(&desired.name, desired.scope, source, &locator);
    entry.path = backend.expand(&desired.path);
    entry.arguments = backend.expand(&desired.arguments);
    entry.working_directory = desired
        .working_directory
        .as_ref()
        .map(|s| backend.expand(s));
    entry.enabled = Some(desired.enabled);
    entry.elevated = desired.elevated;
    entry.delay_seconds = desired.delay_seconds;
    entry.priority = desired.priority;
    entry
}

fn same_configuration(a: &Entry, b: &Entry) -> bool {
    let path = |s: &str| s.replace('/', "\\").to_lowercase();
    a.source == b.source
        && a.locator.eq_ignore_ascii_case(&b.locator)
        && path(&a.path) == path(&b.path)
        && a.arguments == b.arguments
        && a.working_directory.as_deref().map(path) == b.working_directory.as_deref().map(path)
        && a.elevated == b.elevated
        && a.delay_seconds.unwrap_or(0) == b.delay_seconds.unwrap_or(0)
        && a.priority.unwrap_or(7) == b.priority.unwrap_or(7)
        && !a.boot
}

fn ensure_inventory(
    inventory: &Inventory,
    relevant: impl Fn(&InventoryWarning) -> bool,
) -> Result<(), String> {
    let blocking: Vec<_> = inventory
        .warnings
        .iter()
        .filter(|w| relevant(w))
        .map(|w| w.message.as_str())
        .collect();
    if blocking.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "inventory incomplete for managed entries: {}",
            blocking.join("; ")
        ))
    }
}

pub fn plan(backend: &impl Backend, manifest: &Manifest) -> Result<Plan, String> {
    parse_manifest(&serde_json::to_string(manifest).map_err(|e| e.to_string())?)
        .map_err(|errors| serde_json::to_string(&errors).expect("problems serialize"))?;
    let inventory = backend.inventory()?;
    ensure_inventory(&inventory, |warning| {
        manifest
            .entries
            .iter()
            .any(|wanted| warning.affects(wanted.scope, &wanted.name))
    })?;
    let entries = manifest
        .entries
        .iter()
        .map(|wanted| {
            let desired = desired_entry(backend, wanted);
            let existing: Vec<_> = inventory
                .entries
                .iter()
                .filter(|e| {
                    e.scope == wanted.scope && e.name.to_lowercase() == wanted.name.to_lowercase()
                })
                .cloned()
                .collect();
            if existing.is_empty() {
                backend.ensure_task_absent(&backend.task_locator(wanted.scope, &wanted.name))?;
            }
            let same = existing.len() == 1 && same_configuration(&existing[0], &desired);
            let action = if !wanted.enabled {
                if existing.is_empty() {
                    Action::Unchanged
                } else {
                    Action::Disable
                }
            } else if same {
                if existing[0].enabled == Some(true) {
                    Action::Unchanged
                } else {
                    Action::Enable
                }
            } else if existing.is_empty() {
                Action::Create
            } else {
                Action::Update
            };
            let needs_elevation = action != Action::Unchanged
                && (wanted.scope == Scope::Machine
                    || wanted.elevated
                    || existing.iter().any(|e| e.elevated));
            let remove = existing
                .into_iter()
                .filter(|e| {
                    !wanted.enabled
                        || e.source != desired.source
                        || !e.locator.eq_ignore_ascii_case(&desired.locator)
                })
                .collect();
            Ok(PlannedEntry {
                name: wanted.name.clone(),
                scope: wanted.scope,
                action,
                source: desired.source,
                desired: wanted.enabled.then_some(desired),
                remove,
                needs_elevation,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Plan {
        entries,
        warnings: inventory.warnings,
    })
}
pub fn apply(backend: &mut impl Backend, manifest: &Manifest) -> Result<ApplyReport, String> {
    let plan = plan(backend, manifest)?;
    execute(backend, plan)
}

fn selection(backend: &impl Backend, selector: &str) -> Result<(Inventory, Vec<Entry>), String> {
    let inventory = backend.inventory()?;
    let by_id: Vec<_> = inventory
        .entries
        .iter()
        .filter(|e| e.id.to_lowercase() == selector.to_lowercase())
        .cloned()
        .collect();
    let exact_id = !by_id.is_empty();
    let selected: Vec<Entry> = if !exact_id {
        inventory
            .entries
            .iter()
            .filter(|e| e.name.to_lowercase() == selector.to_lowercase())
            .cloned()
            .collect()
    } else {
        by_id
    };
    ensure_inventory(&inventory, |warning| {
        if exact_id {
            selected
                .iter()
                .any(|entry| warning.affects(entry.scope, &entry.name))
        } else {
            warning
                .name
                .as_deref()
                .is_none_or(|name| name.to_lowercase() == selector.to_lowercase())
        }
    })?;
    if selected.is_empty() {
        return Err("entry not found".into());
    }
    if selected.iter().any(|e| e.scope != selected[0].scope) {
        return Err("ambiguous name across scopes; use an id from list".into());
    }
    Ok((inventory, selected))
}
pub fn remove(backend: &mut impl Backend, selector: &str) -> Result<ApplyReport, String> {
    let (inventory, selected) = selection(backend, selector)?;
    let target = &selected[0];
    let remove: Vec<_> = inventory
        .entries
        .into_iter()
        .filter(|e| e.scope == target.scope && e.name.to_lowercase() == target.name.to_lowercase())
        .collect();
    let needs_elevation = target.scope == Scope::Machine || remove.iter().any(|e| e.elevated);
    execute(
        backend,
        Plan {
            warnings: inventory.warnings,
            entries: vec![PlannedEntry {
                name: target.name.clone(),
                scope: target.scope,
                action: Action::Remove,
                source: target.source,
                desired: None,
                remove,
                needs_elevation,
            }],
        },
    )
}
pub fn run(backend: &impl Backend, selector: &str) -> Result<serde_json::Value, String> {
    let (inventory, selected) = selection(backend, selector)?;
    if selected.len() != 1 {
        return Err("ambiguous name; use an id from list".into());
    }
    let entry = &selected[0];
    if entry.elevated && !backend.is_elevated()? {
        return Ok(
            serde_json::json!({"status": "elevation_required", "entry": entry, "warnings": inventory.warnings}),
        );
    }
    let pid = backend.start(entry)?;
    Ok(serde_json::json!({ "entry": entry, "pid": pid, "warnings": inventory.warnings }))
}

fn execute(backend: &mut impl Backend, plan: Plan) -> Result<ApplyReport, String> {
    if plan.entries.iter().any(|e| e.needs_elevation) && !backend.is_elevated()? {
        return Ok(ApplyReport {
            status: "elevation_required".into(),
            plan,
            results: vec![],
        });
    }
    let mut results = Vec::new();
    for item in &plan.entries {
        let result = if item.action == Action::Unchanged {
            Ok(())
        } else {
            (|| {
                if let Some(desired) = &item.desired {
                    backend.write(desired)?;
                }
                for old in &item.remove {
                    backend.delete(old)?;
                }
                Ok(())
            })()
        };
        results.push(EntryResult {
            name: item.name.clone(),
            scope: item.scope,
            status: if result.is_err() {
                "failed"
            } else if item.action == Action::Unchanged {
                "unchanged"
            } else {
                "applied"
            }
            .into(),
            error: result.err(),
        });
    }
    let status = if results.iter().any(|r| r.status == "failed") {
        "partial_failure"
    } else {
        "success"
    }
    .into();
    Ok(ApplyReport {
        status,
        plan,
        results,
    })
}
