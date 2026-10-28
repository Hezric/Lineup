#![cfg(windows)]
use lineup::{
    Backend, Entry, Scope, Source,
    management::{self, DesiredEntry, Manifest},
    native::WindowsBackend,
};
use windows::{
    Win32::{
        Foundation::CloseHandle,
        System::{
            Com::*,
            Registry::*,
            TaskScheduler::*,
            Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
            Variant::VARIANT,
        },
        UI::Shell::{IShellLinkW, ShellLink},
    },
    core::{BSTR, Interface, PCWSTR},
};

struct Cleanup {
    backend: WindowsBackend,
    entries: Vec<Entry>,
    folders: Vec<String>,
    approval_key_created: bool,
}
impl Cleanup {
    fn clean(&mut self) -> Result<(), String> {
        let mut errors = vec![];
        for entry in &self.entries {
            if let Err(error) = self.backend.delete(entry) {
                errors.push(format!("{}: {error}", entry.name));
            }
        }
        unsafe {
            if let Ok(service) = service()
                && let Ok(root) = service.GetFolder(&BSTR::from(r"\"))
            {
                for path in self.folders.iter().rev() {
                    if let Err(error) = root.DeleteFolder(&BSTR::from(path), 0)
                        && ![0x80070002, 0x80070003].contains(&(error.code().0 as u32))
                    {
                        errors.push(error.to_string());
                    }
                }
            }
            if self.approval_key_created {
                let status = RegDeleteKeyExW(
                    HKEY_CURRENT_USER,
                    windows::core::w!(
                        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
                    ),
                    KEY_WOW64_64KEY.0,
                    None,
                );
                if status.0 != 0 && status.0 != 2 {
                    errors.push(format!("approval key cleanup: {status:?}"));
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Err(error) = self.clean() {
            eprintln!("integration cleanup failed: {error}");
        }
    }
}
fn service() -> windows::core::Result<ITaskService> {
    unsafe {
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        let empty = VARIANT::default();
        service.Connect(&empty, &empty, &empty, &empty)?;
        Ok(service)
    }
}
fn wanted(name: &str, path: &str) -> DesiredEntry {
    DesiredEntry {
        name: name.into(),
        path: path.into(),
        arguments: "--list".into(),
        working_directory: None,
        scope: Scope::User,
        elevated: false,
        delay_seconds: None,
        priority: None,
        enabled: true,
    }
}

#[test]
fn current_user_round_trip() {
    if std::env::var("LINEUP_REAL_TESTS").as_deref() != Ok("1") {
        eprintln!("real Windows round trip skipped; set LINEUP_REAL_TESTS=1");
        return;
    }
    let backend = WindowsBackend::new().unwrap();
    let prefix = format!(
        "LineupTest-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let path = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let run_name = format!("{prefix}-run");
    let task_name = format!("{prefix}-task");
    let link_name = format!("{prefix}-link");
    let run_entry = Entry::new(&run_name, Scope::User, Source::Run, &run_name);
    let task_entry = Entry::new(
        &task_name,
        Scope::User,
        Source::Task,
        &backend.task_locator(Scope::User, &task_name),
    );
    let link_entry = Entry::new(
        &link_name,
        Scope::User,
        Source::Startup,
        &format!("{link_name}.lnk"),
    );
    let before = backend.inventory().unwrap();
    assert!(before.entries.iter().all(|e| !e.name.starts_with(&prefix)));
    let mut guard = Cleanup {
        backend,
        entries: vec![run_entry, task_entry.clone(), link_entry.clone()],
        folders: vec![],
        approval_key_created: false,
    };
    if !guard.backend.is_elevated().unwrap() {
        assert!(
            before
                .entries
                .iter()
                .all(|e| !e.name.starts_with("LineupTest-preflight-"))
        );
        guard.entries.push(Entry::new(
            "LineupTest-preflight-user",
            Scope::User,
            Source::Run,
            "LineupTest-preflight-user",
        ));
        let refused = std::process::Command::new(env!("CARGO_BIN_EXE_lineup"))
            .args([
                "apply",
                "--manifest",
                "tests/fixtures/elevation-preflight.json",
            ])
            .output()
            .unwrap();
        assert_eq!(refused.status.code(), Some(3));
        let refusal: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
        assert_eq!(refusal["report"]["status"], "elevation_required");
        assert_eq!(guard.backend.inventory().unwrap().entries, before.entries);
    }
    unsafe {
        let root = service().unwrap().GetFolder(&BSTR::from(r"\")).unwrap();
        for folder in [
            r"\Lineup".to_owned(),
            task_entry.locator.rsplit_once('\\').unwrap().0.to_owned(),
        ] {
            match root.GetFolder(&BSTR::from(&folder)) {
                Ok(_) => {}
                Err(error) if [0x80070002, 0x80070003].contains(&(error.code().0 as u32)) => {
                    guard.folders.push(folder)
                }
                Err(error) => panic!("cannot inspect task test folder: {error}"),
            }
        }
    }
    let plain = wanted(&run_name, &path);
    let mut task = wanted(&task_name, &path);
    task.delay_seconds = Some(17);
    task.priority = Some(6);
    task.working_directory = Some(
        std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
    );
    let manifest = Manifest {
        schema_version: 1,
        entries: vec![plain.clone(), task.clone()],
    };
    let preview = management::plan(&guard.backend, &manifest).unwrap();
    assert!(
        preview
            .entries
            .iter()
            .all(|e| e.action == management::Action::Create)
    );
    let applied = management::apply(&mut guard.backend, &manifest).unwrap();
    assert_eq!(applied.status, "success", "{applied:?}");
    assert_eq!(
        management::apply(&mut guard.backend, &manifest)
            .unwrap()
            .results
            .iter()
            .filter(|r| r.status == "unchanged")
            .count(),
        2
    );
    let inventory = guard.backend.inventory().unwrap();
    let actual = inventory
        .entries
        .iter()
        .find(|e| e.name == task_name)
        .unwrap();
    assert_eq!(actual.delay_seconds, Some(17));
    assert_eq!(actual.priority, Some(6));
    assert!(!actual.elevated);
    assert_eq!(actual.working_directory, task.working_directory);
    unsafe {
        let mut key = HKEY::default();
        let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            windows::core::w!(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
            ),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            None,
            &mut key,
            Some(&mut disposition),
        )
        .ok()
        .unwrap();
        guard.approval_key_created = disposition == REG_CREATED_NEW_KEY;
        let mut disabled = [0u8; 12];
        disabled[0] = 3;
        let name: Vec<u16> = run_name.encode_utf16().chain(Some(0)).collect();
        let write = RegSetValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            REG_BINARY,
            Some(&disabled),
        );
        let _ = RegCloseKey(key);
        write.ok().unwrap();
    }
    assert!(
        guard
            .backend
            .inventory()
            .unwrap()
            .entries
            .iter()
            .find(|e| e.name == run_name)
            .unwrap()
            .enabled
            == Some(false)
    );
    assert_eq!(
        management::plan(&guard.backend, &manifest).unwrap().entries[0].action,
        management::Action::Enable
    );
    assert_eq!(
        management::apply(&mut guard.backend, &manifest)
            .unwrap()
            .status,
        "success"
    );
    assert!(
        guard
            .backend
            .inventory()
            .unwrap()
            .entries
            .iter()
            .find(|e| e.name == run_name)
            .unwrap()
            .enabled
            == Some(true)
    );
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
        let persist: IPersistFile = link.cast().unwrap();
        link.SetPath(PCWSTR(
            path.encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>()
                .as_ptr(),
        ))
        .unwrap();
        link.SetArguments(PCWSTR(
            "--list"
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>()
                .as_ptr(),
        ))
        .unwrap();
        let link_path = guard
            .backend
            .startup_folder(Scope::User)
            .unwrap()
            .join(&link_entry.locator);
        assert!(!link_path.exists());
        persist
            .Save(
                PCWSTR(
                    link_path
                        .to_string_lossy()
                        .encode_utf16()
                        .chain(Some(0))
                        .collect::<Vec<_>>()
                        .as_ptr(),
                ),
                true,
            )
            .unwrap();
    }
    let inventory = guard.backend.inventory().unwrap();
    let link = inventory
        .entries
        .iter()
        .find(|e| e.name == link_name)
        .unwrap();
    assert_eq!(link.path, path);
    assert_eq!(link.arguments, "--list");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_lineup"))
        .args(["run", &run_name])
        .output()
        .unwrap();
    assert!(output.status.success());
    let started: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let pid = started["started"]["pid"].as_u64().unwrap() as u32;
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            let wait = WaitForSingleObject(handle, 5000);
            let _ = CloseHandle(handle);
            assert_eq!(wait.0, 0, "the owned test child must exit promptly");
        }
    }
    let mut disabled = plain;
    disabled.enabled = false;
    assert_eq!(
        management::apply(
            &mut guard.backend,
            &Manifest {
                schema_version: 1,
                entries: vec![disabled]
            }
        )
        .unwrap()
        .status,
        "success"
    );
    guard.clean().unwrap();
    let after = guard.backend.inventory().unwrap();
    assert!(after.entries.iter().all(|e| !e.name.starts_with(&prefix)));
    assert_eq!(
        before.entries, after.entries,
        "round trip must preserve all existing entries"
    );
}
