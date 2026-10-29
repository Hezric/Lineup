use lineup::{Backend, Entry, Inventory, Scope, Source, management::*};

#[derive(Default)]
struct Memory {
    entries: Vec<Entry>,
    fail: Option<String>,
    fail_delete: Option<String>,
    inventory_error: bool,
    elevation_error: bool,
    start_error: bool,
    writes: usize,
    admin: bool,
    warnings: Vec<lineup::InventoryWarning>,
}
impl Backend for Memory {
    fn inventory(&self) -> Result<Inventory, String> {
        if self.inventory_error {
            return Err("inventory unavailable".into());
        }
        Ok(Inventory {
            entries: self.entries.clone(),
            warnings: self.warnings.clone(),
        })
    }
    fn target_exists(&self, _: &str) -> bool {
        true
    }
    fn is_elevated(&self) -> Result<bool, String> {
        if self.elevation_error {
            return Err("token unavailable".into());
        }
        Ok(self.admin)
    }
    fn write(&mut self, entry: &Entry) -> Result<(), String> {
        self.writes += 1;
        if self.fail.as_ref() == Some(&entry.name) {
            return Err("injected write failure".into());
        }
        self.entries.retain(|e| {
            !(e.scope == entry.scope
                && e.source == entry.source
                && e.locator.eq_ignore_ascii_case(&entry.locator))
        });
        self.entries.push(entry.clone());
        Ok(())
    }
    fn delete(&mut self, entry: &Entry) -> Result<(), String> {
        self.writes += 1;
        if self.fail_delete.as_ref() == Some(&entry.name) {
            return Err("injected delete failure".into());
        }
        self.entries.retain(|e| {
            !(e.scope == entry.scope
                && e.source == entry.source
                && e.locator.eq_ignore_ascii_case(&entry.locator))
        });
        Ok(())
    }
    fn start(&self, entry: &Entry) -> Result<u32, String> {
        if self.start_error {
            return Err("launch failed".into());
        }
        assert_eq!(entry.arguments, "--test");
        Ok(1234)
    }
}
fn entry(name: &str) -> DesiredEntry {
    DesiredEntry {
        name: name.into(),
        path: r"C:\Windows\notepad.exe".into(),
        arguments: String::new(),
        working_directory: None,
        scope: Scope::User,
        elevated: false,
        delay_seconds: None,
        priority: None,
        enabled: true,
    }
}
fn manifest(entries: Vec<DesiredEntry>) -> Manifest {
    Manifest {
        schema_version: 1,
        entries,
    }
}

#[test]
fn creates_plain_entries_and_second_apply_is_unchanged() {
    let mut system = Memory::default();
    let input = manifest(vec![entry("example")]);
    let preview = plan(&system, &input).unwrap();
    assert_eq!(preview.entries.len(), 1);
    assert_eq!(preview.entries[0].action, Action::Create);
    assert_eq!(preview.entries[0].source, Source::Run);
    assert_eq!(system.writes, 0);
    assert_eq!(
        apply(&mut system, &input).unwrap().results[0].status,
        "applied"
    );
    assert_eq!(
        apply(&mut system, &input).unwrap().results[0].status,
        "unchanged"
    );
    assert_eq!(system.writes, 1);
}

#[test]
fn updates_and_replaces_same_name_sources_then_disables_without_touching_others() {
    let mut system = Memory::default();
    let mut wanted = entry("example");
    apply(
        &mut system,
        &manifest(vec![wanted.clone(), entry("unrelated")]),
    )
    .unwrap();
    wanted.arguments = "--new".into();
    assert_eq!(
        plan(&system, &manifest(vec![wanted.clone()]))
            .unwrap()
            .entries[0]
            .action,
        Action::Update
    );
    apply(&mut system, &manifest(vec![wanted.clone()])).unwrap();
    system
        .entries
        .iter_mut()
        .find(|e| e.name == "example")
        .unwrap()
        .enabled = Some(false);
    assert_eq!(
        plan(&system, &manifest(vec![wanted.clone()]))
            .unwrap()
            .entries[0]
            .action,
        Action::Enable
    );
    wanted.delay_seconds = Some(15);
    let mut shortcut = Entry::new("EXAMPLE", Scope::User, Source::Startup, "EXAMPLE.lnk");
    shortcut.path = wanted.path.clone();
    system.entries.push(shortcut);
    let preview = plan(&system, &manifest(vec![wanted.clone()])).unwrap();
    assert_eq!(preview.entries[0].source, Source::Task);
    assert_eq!(preview.entries[0].remove.len(), 2);
    apply(&mut system, &manifest(vec![wanted.clone()])).unwrap();
    assert_eq!(system.entries.len(), 2);
    assert_eq!(
        plan(&system, &manifest(vec![wanted.clone()]))
            .unwrap()
            .entries[0]
            .action,
        Action::Unchanged
    );
    wanted.enabled = false;
    assert_eq!(
        plan(&system, &manifest(vec![wanted.clone()]))
            .unwrap()
            .entries[0]
            .action,
        Action::Disable
    );
    apply(&mut system, &manifest(vec![wanted.clone()])).unwrap();
    assert_eq!(system.entries.len(), 1);
    assert_eq!(system.entries[0].name, "unrelated");
    assert_eq!(
        plan(&system, &manifest(vec![wanted])).unwrap().entries[0].action,
        Action::Unchanged
    );
}

#[test]
fn a_failed_entry_preserves_old_sources_and_does_not_stop_other_entries() {
    let mut old = Entry::new("broken", Scope::User, Source::Startup, "broken.lnk");
    old.path = "old.exe".into();
    let mut system = Memory {
        entries: vec![old.clone()],
        fail: Some("broken".into()),
        ..Default::default()
    };
    let report = apply(
        &mut system,
        &manifest(vec![entry("broken"), entry("working")]),
    )
    .unwrap();
    assert_eq!(report.status, "partial_failure");
    assert_eq!(report.results[0].status, "failed");
    assert!(
        report.results[0]
            .error
            .as_ref()
            .unwrap()
            .contains("injected")
    );
    assert_eq!(report.results[1].status, "applied");
    assert!(system.entries.contains(&old));
    assert_eq!(system.entries.len(), 2);
}

#[test]
fn manifest_reports_all_field_problems_before_any_change() {
    let problems = parse_manifest(r#"{"schemaVersion":2,"entries":[{"name":"bad/name","path":12,"priority":11,"enabled":"yes","unknown":true}]}"#).unwrap_err();
    assert!(problems.len() >= 5, "{problems:?}");
    assert!(problems.iter().any(|e| e.path == "/entries/0/priority"));
    let duplicate = r#"{"schemaVersion":1,"entries":[{"name":"example","path":"C:\\Windows\\notepad.exe"},{"name":"EXAMPLE","path":"C:\\Windows\\notepad.exe"}]}"#;
    assert!(
        parse_manifest(duplicate)
            .unwrap_err()
            .iter()
            .any(|e| e.path == "/entries/1/name")
    );
    let valid = parse_manifest(
        r#"{"schemaVersion":1,"entries":[{"name":"example","path":"%SystemRoot%\\notepad.exe"}]}"#,
    )
    .unwrap();
    assert_eq!(valid.entries[0].scope, Scope::User);
    assert!(valid.entries[0].enabled);
}

#[test]
fn remove_by_id_removes_all_same_scope_realizations_and_run_requires_unambiguous_entry() {
    let mut wanted = entry("example");
    wanted.arguments = "--test".into();
    let mut system = Memory::default();
    apply(&mut system, &manifest(vec![wanted])).unwrap();
    assert_eq!(run(&system, "example").unwrap()["pid"], 1234);
    let id = system.entries[0].id.clone();
    let mut shortcut = system.entries[0].clone();
    shortcut.source = Source::Startup;
    shortcut.id = "user:startup:example.lnk".into();
    shortcut.locator = "example.lnk".into();
    system.entries.push(shortcut);
    assert!(run(&system, "example").unwrap_err().contains("ambiguous"));
    assert_eq!(run(&system, &id).unwrap()["pid"], 1234);
    let mut machine = Entry::new("example", Scope::Machine, Source::Run, "example");
    machine.path = "other.exe".into();
    system.entries.push(machine);
    assert!(
        remove(&mut system, "example")
            .unwrap_err()
            .contains("ambiguous")
    );
    let report = remove(&mut system, &id).unwrap();
    assert_eq!(report.plan.entries[0].action, Action::Remove);
    assert_eq!(report.plan.entries[0].remove.len(), 2);
    assert_eq!(system.entries.len(), 1);
    assert_eq!(system.entries[0].scope, Scope::Machine);
}

#[test]
fn elevation_preflight_blocks_the_entire_manifest_and_elevated_retry_converges() {
    let mut machine = entry("shared");
    machine.scope = Scope::Machine;
    let mut admin = entry("administrator");
    admin.elevated = true;
    admin.delay_seconds = Some(5);
    let input = manifest(vec![entry("normal"), machine, admin]);
    let mut system = Memory::default();
    let preview = plan(&system, &input).unwrap();
    assert!(!preview.entries[0].needs_elevation);
    assert!(preview.entries[1].needs_elevation);
    assert!(preview.entries[2].needs_elevation);
    assert_eq!(preview.entries[1].source, Source::Run);
    assert_eq!(preview.entries[2].source, Source::Task);
    let result = apply(&mut system, &input).unwrap();
    assert_eq!(result.status, "elevation_required");
    assert_eq!(system.writes, 0);
    assert!(system.entries.is_empty());
    system.admin = true;
    assert_eq!(apply(&mut system, &input).unwrap().status, "success");
    assert_eq!(system.entries.len(), 3);
    assert_eq!(
        system
            .entries
            .iter()
            .filter(|e| e.scope == Scope::User)
            .count(),
        2
    );
    system.admin = false;
    assert!(
        plan(&system, &input)
            .unwrap()
            .entries
            .iter()
            .all(|e| e.action == Action::Unchanged && !e.needs_elevation)
    );
    assert_eq!(apply(&mut system, &input).unwrap().status, "success");
    let writes = system.writes;
    let mut disabled = entry("administrator");
    disabled.enabled = false;
    assert!(
        plan(&system, &manifest(vec![disabled.clone()]))
            .unwrap()
            .entries[0]
            .needs_elevation
    );
    assert_eq!(
        apply(&mut system, &manifest(vec![disabled]))
            .unwrap()
            .status,
        "elevation_required"
    );
    assert_eq!(
        remove(&mut system, "shared").unwrap().status,
        "elevation_required"
    );
    assert_eq!(system.writes, writes);
}

#[test]
fn same_name_tasks_in_user_and_machine_scopes_have_independent_identities() {
    let mut user = entry("shared");
    user.delay_seconds = Some(0);
    let mut machine = user.clone();
    machine.scope = Scope::Machine;
    let mut system = Memory {
        admin: true,
        ..Default::default()
    };
    assert_eq!(
        apply(&mut system, &manifest(vec![user, machine]))
            .unwrap()
            .status,
        "success"
    );
    assert_eq!(system.entries.len(), 2);
    assert_ne!(system.entries[0].locator, system.entries[1].locator);
}

#[test]
fn replacement_of_a_multi_action_task_does_not_delete_the_new_task() {
    let mut wanted = entry("example");
    wanted.delay_seconds = Some(5);
    let mut system = Memory::default();
    apply(&mut system, &manifest(vec![wanted.clone()])).unwrap();
    let mut second = system.entries[0].clone();
    second.id.push_str("#2");
    system.entries[0].id.push_str("#1");
    system.entries.push(second);
    let preview = plan(&system, &manifest(vec![wanted.clone()])).unwrap();
    assert_eq!(preview.entries[0].action, Action::Update);
    assert!(
        preview.entries[0].remove.is_empty(),
        "upsert replaces the same physical task"
    );
    assert_eq!(
        apply(&mut system, &manifest(vec![wanted.clone()]))
            .unwrap()
            .status,
        "success"
    );
    assert_eq!(system.entries.len(), 1);
    assert_eq!(
        plan(&system, &manifest(vec![wanted])).unwrap().entries[0].action,
        Action::Unchanged
    );
}

#[test]
fn inventory_uncertainty_blocks_only_possible_managed_realizations() {
    for (scope, name, blocked) in [
        (Some(Scope::Machine), Some("unrelated"), false),
        (Some(Scope::User), Some("unrelated"), false),
        (Some(Scope::Machine), Some("EXAMPLE"), false),
        (Some(Scope::User), Some("EXAMPLE"), true),
        (Some(Scope::User), None, true),
        (Some(Scope::Machine), None, false),
        (None, Some("unrelated"), false),
        (None, Some("EXAMPLE"), true),
        (None, None, true),
    ] {
        for enabled in [true, false] {
            let warning = lineup::InventoryWarning {
                scope,
                name: name.map(str::to_owned),
                message: "injected unreadable source or unknown entry state".into(),
            };
            let mut existing = Entry::new("example", Scope::User, Source::Startup, "example.lnk");
            existing.path = r"C:\Windows\notepad.exe".into();
            let mut system = Memory {
                entries: vec![existing],
                warnings: vec![warning.clone()],
                ..Default::default()
            };
            let mut wanted = entry("example");
            wanted.enabled = enabled;
            let input = manifest(vec![wanted]);
            assert_eq!(
                plan(&system, &input).is_err(),
                blocked,
                "{scope:?}/{name:?}, enabled={enabled}"
            );
            let result = apply(&mut system, &input);
            if blocked {
                assert!(result.unwrap_err().contains("inventory incomplete"));
                assert_eq!(system.writes, 0);
            } else {
                let report = result.unwrap();
                assert_eq!(report.status, "success");
                assert_eq!(report.plan.warnings, vec![warning]);
                assert!(system.writes > 0);
            }
        }
    }
}

#[test]
fn same_name_uncertainty_uses_the_planners_unicode_matching() {
    let system = Memory {
        warnings: vec![lineup::InventoryWarning {
            scope: Some(Scope::User),
            name: Some("ÉXAMPLE".into()),
            message: "unknown state".into(),
        }],
        ..Default::default()
    };
    assert!(plan(&system, &manifest(vec![entry("éxample")])).is_err());
}

#[test]
fn single_entry_operations_preserve_warnings_and_respect_scope_selection() {
    let mut system = Memory::default();
    let mut wanted = entry("example");
    wanted.arguments = "--test".into();
    apply(&mut system, &manifest(vec![wanted])).unwrap();
    let id = system.entries[0].id.clone();
    system.warnings.push(lineup::InventoryWarning {
        scope: Some(Scope::Machine),
        name: None,
        message: "machine folder unreadable".into(),
    });
    assert!(
        remove(&mut system, "example").is_err(),
        "name selection cannot rule out another scope"
    );
    let started = run(&system, &id).unwrap();
    assert_eq!(started["pid"], 1234);
    assert_eq!(started["warnings"][0]["scope"], "machine");
    let warning = lineup::InventoryWarning {
        scope: Some(Scope::User),
        name: Some("EXAMPLE".into()),
        message: "same-name shortcut unreadable".into(),
    };
    system.warnings.push(warning);
    let writes = system.writes;
    assert!(remove(&mut system, &id).is_err());
    assert!(run(&system, &id).is_err());
    assert_eq!(system.writes, writes);
    system.warnings.pop();
    let report = remove(&mut system, &id).unwrap();
    assert_eq!(report.status, "success");
    assert_eq!(report.plan.warnings, system.warnings);
    assert!(system.entries.is_empty());
}

#[test]
fn failed_cleanup_keeps_the_new_source_and_continues_independent_entries() {
    let old = Entry::new("example", Scope::User, Source::Startup, "example.lnk");
    let mut system = Memory {
        entries: vec![old.clone()],
        fail_delete: Some("example".into()),
        ..Default::default()
    };
    let report = apply(
        &mut system,
        &manifest(vec![entry("example"), entry("other")]),
    )
    .unwrap();
    assert_eq!(report.status, "partial_failure");
    assert_eq!(report.results[0].status, "failed");
    assert_eq!(
        report.results[0].error.as_deref(),
        Some("injected delete failure")
    );
    assert_eq!(report.results[1].status, "applied");
    assert!(system.entries.contains(&old));
    assert_eq!(system.entries.len(), 3);
    assert!(
        system
            .entries
            .iter()
            .any(|e| e.name == "example" && e.source == Source::Run)
    );
    system.fail_delete = None;
    assert_eq!(
        apply(&mut system, &manifest(vec![entry("example")]))
            .unwrap()
            .status,
        "success"
    );
    assert_eq!(system.entries.len(), 2);
}

#[test]
fn elevated_run_refuses_before_launch_and_propagates_token_and_launch_failures() {
    let mut wanted = entry("example");
    wanted.elevated = true;
    wanted.arguments = "--test".into();
    let mut system = Memory {
        admin: true,
        ..Default::default()
    };
    apply(&mut system, &manifest(vec![wanted])).unwrap();
    system.admin = false;
    let result = run(&system, "example").unwrap();
    assert_eq!(result["status"], "elevation_required");
    assert!(result.get("pid").is_none());
    system.elevation_error = true;
    assert_eq!(run(&system, "example").unwrap_err(), "token unavailable");
    assert_eq!(
        remove(&mut system, "example").unwrap_err(),
        "token unavailable"
    );
    assert_eq!(system.entries.len(), 1);
    system.elevation_error = false;
    system.admin = true;
    assert_eq!(run(&system, "example").unwrap()["pid"], 1234);
    system.start_error = true;
    assert_eq!(run(&system, "example").unwrap_err(), "launch failed");
}

#[test]
fn validation_precedes_inventory_and_inventory_failures_prevent_all_operations() {
    let mut system = Memory {
        inventory_error: true,
        ..Default::default()
    };
    let mut invalid = entry("example");
    invalid.priority = Some(11);
    assert!(
        plan(&system, &manifest(vec![invalid]))
            .unwrap_err()
            .contains("priority")
    );
    let input = manifest(vec![entry("example")]);
    assert_eq!(lineup::check(&system).unwrap_err(), "inventory unavailable");
    assert_eq!(plan(&system, &input).unwrap_err(), "inventory unavailable");
    assert_eq!(
        apply(&mut system, &input).unwrap_err(),
        "inventory unavailable"
    );
    assert_eq!(
        remove(&mut system, "example").unwrap_err(),
        "inventory unavailable"
    );
    assert_eq!(
        run(&system, "example").unwrap_err(),
        "inventory unavailable"
    );
    assert_eq!(system.writes, 0);
    system.inventory_error = false;
    assert_eq!(run(&system, "missing").unwrap_err(), "entry not found");
    assert_eq!(
        remove(&mut system, "missing").unwrap_err(),
        "entry not found"
    );
}

#[test]
fn json_syntax_and_invalid_entry_shapes_report_problems() {
    for raw in ["{", r#"{"schemaVersion":1,"entries":[null,{"name":4}]}"#] {
        assert!(!parse_manifest(raw).unwrap_err().is_empty());
    }
    let mut system = Memory::default();
    let mut disabled = entry("absent");
    disabled.enabled = false;
    let report = apply(&mut system, &manifest(vec![disabled])).unwrap();
    assert_eq!(report.results[0].status, "unchanged");
    assert_eq!(system.writes, 0);
}

#[test]
fn utf8_bom_is_an_optional_encoding_marker_not_manifest_content() {
    for raw in [
        "\u{feff} {\"schemaVersion\":1,\"entries\":[]}",
        "\u{feff}schemaVersion = 1\nentries = []\n",
    ] {
        assert!(parse_manifest(raw).unwrap().entries.is_empty());
    }
    assert!(parse_manifest("\u{feff}\u{feff}{}").is_err());
}
