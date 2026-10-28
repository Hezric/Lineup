#![cfg(windows)]
use lineup::{
    Backend, Entry, Inventory, Scope, Source,
    management::{self, Action, DesiredEntry, Manifest},
    native::WindowsBackend,
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
        Security::{Authorization::ConvertSidToStringSidW, *},
        System::{
            Com::*,
            Registry::*,
            TaskScheduler::*,
            Threading::{GetCurrentProcess, OpenProcessToken},
            Variant::VARIANT,
        },
    },
    core::{BSTR, Interface, PCWSTR, PWSTR},
};
#[path = "support/files.rs"]
mod files;
#[path = "support/limited.rs"]
mod limited;
#[path = "support/security.rs"]
mod security;

fn text(get: impl FnOnce(*mut BSTR) -> windows::core::Result<()>) -> String {
    let mut value = BSTR::new();
    get(&mut value).unwrap();
    value.to_string()
}
fn value<T: Default>(get: impl FnOnce(*mut T) -> windows::core::Result<()>) -> T {
    let mut value = T::default();
    get(&mut value).unwrap();
    value
}
fn try_service() -> windows::core::Result<ITaskService> {
    unsafe {
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        let empty = VARIANT::default();
        service.Connect(&empty, &empty, &empty, &empty)?;
        Ok(service)
    }
}
fn service() -> ITaskService {
    try_service().unwrap()
}
fn sid_text(sid: PSID) -> String {
    unsafe {
        let mut ptr = PWSTR::null();
        ConvertSidToStringSidW(sid, &mut ptr).unwrap();
        let result = ptr.to_string().unwrap();
        let _ = LocalFree(Some(HLOCAL(ptr.0.cast())));
        result
    }
}
fn account_sid(account: &str) -> String {
    unsafe {
        if account.starts_with("S-1-") {
            return account.into();
        }
        let input: Vec<u16> = account.encode_utf16().chain(Some(0)).collect();
        let mut sid_len = 0;
        let mut domain_len = 0;
        let mut kind = SID_NAME_USE::default();
        let _ = LookupAccountNameW(
            PCWSTR::null(),
            PCWSTR(input.as_ptr()),
            None,
            &mut sid_len,
            None,
            &mut domain_len,
            &mut kind,
        );
        let mut sid = vec![0usize; (sid_len as usize).div_ceil(std::mem::size_of::<usize>())];
        let mut domain = vec![0u16; domain_len as usize];
        let ptr = PSID(sid.as_mut_ptr().cast());
        LookupAccountNameW(
            PCWSTR::null(),
            PCWSTR(input.as_ptr()),
            Some(ptr),
            &mut sid_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &mut domain_len,
            &mut kind,
        )
        .unwrap();
        sid_text(ptr)
    }
}
fn current_sid() -> String {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).unwrap();
        let mut len = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut data = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
        let result = GetTokenInformation(
            token,
            TokenUser,
            Some(data.as_mut_ptr().cast()),
            len,
            &mut len,
        );
        let _ = CloseHandle(token);
        result.unwrap();
        sid_text((*data.as_ptr().cast::<TOKEN_USER>()).User.Sid)
    }
}
fn prepare_probe_directory(path: &Path, user: &str) {
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SE_FILE_OBJECT, SetNamedSecurityInfoW,
    };
    unsafe {
        let sddl =
            format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{user})S:(ML;OICI;NW;;;ME)");
        let sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            1,
            &mut sd,
            None,
        )
        .unwrap();
        let result = (|| -> windows::core::Result<()> {
            let mut present = windows::core::BOOL::default();
            let mut defaulted = windows::core::BOOL::default();
            let mut dacl = std::ptr::null_mut();
            let mut sacl = std::ptr::null_mut();
            GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted)?;
            GetSecurityDescriptorSacl(sd, &mut present, &mut sacl, &mut defaulted)?;
            let name: Vec<u16> = path
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            SetNamedSecurityInfoW(
                PCWSTR(name.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION
                    | LABEL_SECURITY_INFORMATION,
                None,
                None,
                Some(dacl),
                Some(sacl),
            )
            .ok()
        })();
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        result.unwrap();
    }
}
fn missing(error: &windows::core::Error) -> bool {
    [0x80070002, 0x80070003].contains(&(error.code().0 as u32))
}
fn instance_finished(error: &windows::core::Error) -> bool {
    missing(error) || error.code().0 as u32 == 0x8004130b
}
struct Cleanup {
    running: Vec<IRunningTask>,
    backend: WindowsBackend,
    entries: Vec<Entry>,
    folders: Vec<String>,
    original_acl: Vec<(String, String)>,
}
impl Cleanup {
    fn clean(&mut self) -> Result<(), String> {
        let mut errors = vec![];
        for task in self.running.drain(..) {
            unsafe {
                if let Err(error) = task.Stop()
                    && !instance_finished(&error)
                {
                    errors.push(error.to_string());
                }
                let start = Instant::now();
                loop {
                    match task.Refresh().and_then(|_| task.State()) {
                        Ok(state) if state != TASK_STATE_RUNNING && state != TASK_STATE_QUEUED => {
                            break;
                        }
                        Err(error) if instance_finished(&error) => break,
                        Err(error) => {
                            errors.push(error.to_string());
                            break;
                        }
                        _ => {}
                    }
                    if start.elapsed() > Duration::from_secs(5) {
                        errors.push("owned task did not stop".into());
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        for entry in &self.entries {
            if let Err(error) = self.backend.delete(entry) {
                errors.push(format!("{}: {error}", entry.name));
            }
        }
        unsafe {
            let root = try_service()
                .and_then(|service| service.GetFolder(&BSTR::from(r"\")))
                .map_err(|e| e.to_string())?;
            let mut pending = vec![];
            for (path, sddl) in self.original_acl.drain(..) {
                if let Err(error) = root
                    .GetFolder(&BSTR::from(&path))
                    .and_then(|folder| folder.SetSecurityDescriptor(&BSTR::from(&sddl), 0))
                {
                    errors.push(error.to_string());
                    pending.push((path, sddl));
                }
            }
            self.original_acl = pending;
            for path in self.folders.iter().rev() {
                if let Err(error) = root.DeleteFolder(&BSTR::from(path), 0)
                    && !missing(&error)
                {
                    errors.push(error.to_string());
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
            eprintln!("elevated test cleanup failed: {error}");
        }
    }
}
fn owned_directory() -> PathBuf {
    let path = std::env::current_dir().unwrap();
    assert!(
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("LineupTest-")
    );
    path
}
#[test]
#[ignore = "invoked only by a test-owned scheduled task"]
fn task_elevation_probe() {
    let directory = owned_directory();
    let backend = WindowsBackend::new().unwrap();
    std::fs::write(directory.join("elevation.json"), serde_json::to_vec(&serde_json::json!({"elevated": backend.is_elevated().unwrap(), "pid": std::process::id()})).unwrap()).unwrap();
}
#[test]
#[ignore = "invoked only by a test-owned linked-token child"]
fn unelevated_read_probe() {
    let directory = owned_directory();
    let mut backend = WindowsBackend::new().unwrap();
    assert!(!backend.is_elevated().unwrap());
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(directory.join("manifest.json")).unwrap()).unwrap();
    assert!(
        manifest
            .entries
            .iter()
            .all(|e| e.name.starts_with("LineupTest-"))
    );
    let inventory = backend.inventory().unwrap();
    for entry in &manifest.entries {
        assert!(
            inventory
                .entries
                .iter()
                .any(|e| e.name == entry.name && e.scope == entry.scope)
        );
    }
    let preview = management::plan(&backend, &manifest).unwrap();
    assert!(
        preview
            .entries
            .iter()
            .all(|e| e.action == Action::Unchanged)
    );
    unsafe {
        let root = service().GetFolder(&BSTR::from(r"\")).unwrap();
        for entry in &manifest.entries {
            if entry.scope == Scope::Machine && (entry.elevated || entry.delay_seconds.is_some()) {
                let task = root
                    .GetTask(&BSTR::from(backend.task_locator(entry.scope, &entry.name)))
                    .unwrap();
                let error = task
                    .SetEnabled(false.into())
                    .expect_err("ordinary readers must not modify machine tasks");
                assert_eq!(error.code(), windows::Win32::Foundation::E_ACCESSDENIED);
            }
        }
    }
    let mut changed = manifest;
    changed.entries[0].arguments.push_str(" --changed");
    assert_eq!(
        management::apply(&mut backend, &changed).unwrap().status,
        "elevation_required"
    );
    std::fs::write(directory.join("read-ok"), b"ok").unwrap();
}
fn desired(
    name: String,
    path: &Path,
    scope: Scope,
    elevated: bool,
    delay: Option<u32>,
) -> DesiredEntry {
    DesiredEntry {
        name,
        path: path.to_string_lossy().into_owned(),
        arguments: "--ignored --exact task_elevation_probe --nocapture".into(),
        working_directory: None,
        scope,
        elevated,
        delay_seconds: delay,
        priority: None,
        enabled: true,
    }
}
fn verify_definitions(backend: &WindowsBackend, manifest: &Manifest, expected_sid: &str) {
    unsafe {
        let root = service().GetFolder(&BSTR::from(r"\")).unwrap();
        security::assert_machine_acl(
            &root
                .GetFolder(&BSTR::from(r"\Lineup\Machine"))
                .unwrap()
                .GetSecurityDescriptor(7)
                .unwrap()
                .to_string(),
        );
        for entry in &manifest.entries {
            if entry.delay_seconds.is_none() && !entry.elevated {
                let name: Vec<u16> = entry.name.encode_utf16().chain(Some(0)).collect();
                let mut data = vec![0u16; 32768];
                let mut size = (data.len() * 2) as u32;
                RegGetValueW(
                    HKEY_LOCAL_MACHINE,
                    windows::core::w!(r"Software\Microsoft\Windows\CurrentVersion\Run"),
                    PCWSTR(name.as_ptr()),
                    RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
                    None,
                    Some(data.as_mut_ptr().cast()),
                    Some(&mut size),
                )
                .ok()
                .unwrap();
                let raw =
                    String::from_utf16_lossy(&data[..data.iter().position(|c| *c == 0).unwrap()]);
                assert_eq!(raw, format!("\"{}\" {}", entry.path, entry.arguments));
                continue;
            }
            let task = root
                .GetTask(&BSTR::from(backend.task_locator(entry.scope, &entry.name)))
                .unwrap();
            let definition = task.Definition().unwrap();
            let triggers = definition.Triggers().unwrap();
            assert_eq!(value(|p| triggers.Count(p)), 1);
            let trigger = triggers.get_Item(1).unwrap();
            assert_eq!(value(|p| trigger.Type(p)), TASK_TRIGGER_LOGON);
            let trigger: ILogonTrigger = trigger.cast().unwrap();
            let delay = text(|p| trigger.Delay(p));
            if let Some(seconds) = entry.delay_seconds {
                assert_eq!(delay, format!("PT{seconds}S"));
            } else {
                assert!(delay.is_empty() || delay == "PT0S");
            }
            let principal = definition.Principal().unwrap();
            assert_eq!(
                value(|p| principal.RunLevel(p)),
                if entry.elevated {
                    TASK_RUNLEVEL_HIGHEST
                } else {
                    TASK_RUNLEVEL_LUA
                }
            );
            if entry.scope == Scope::Machine {
                assert_eq!(
                    account_sid(&text(|p| principal.GroupId(p))),
                    if entry.elevated {
                        "S-1-5-32-544"
                    } else {
                        "S-1-5-32-545"
                    }
                );
                assert_eq!(value(|p| principal.LogonType(p)), TASK_LOGON_GROUP);
                assert!(text(|p| trigger.UserId(p)).is_empty());
                security::assert_machine_acl(&task.GetSecurityDescriptor(7).unwrap().to_string());
            } else {
                assert_eq!(account_sid(&text(|p| principal.UserId(p))), expected_sid);
                assert_eq!(account_sid(&text(|p| trigger.UserId(p))), expected_sid);
                assert_eq!(
                    value(|p| principal.LogonType(p)),
                    TASK_LOGON_INTERACTIVE_TOKEN
                );
            }
            let actions = definition.Actions().unwrap();
            assert_eq!(value(|p| actions.Count(p)), 1);
            let action: IExecAction = actions.get_Item(1).unwrap().cast().unwrap();
            assert_eq!(text(|p| action.Path(p)), entry.path);
            assert_eq!(text(|p| action.Arguments(p)), entry.arguments);
            assert_eq!(
                text(|p| action.WorkingDirectory(p)),
                entry.working_directory.clone().unwrap_or_default()
            );
        }
    }
}
#[test]
fn elevated_round_trip() {
    if std::env::var("LINEUP_ELEVATED_TESTS").as_deref() != Ok("1") {
        eprintln!(
            "elevated round trip disabled; set LINEUP_ELEVATED_TESTS=1 in an elevated session"
        );
        return;
    }
    let backend = WindowsBackend::new().unwrap();
    assert!(
        backend.is_elevated().unwrap(),
        "elevated integration requires an administrator token; nothing changed"
    );
    let files = files::TestFiles::new();
    prepare_probe_directory(&files.path, &current_sid());
    let prefix = files
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let exe = files.file("probe.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
    let specs = [
        ("machine-run", Scope::Machine, false, None),
        ("machine-task", Scope::Machine, false, Some(1)),
        ("user-elevated", Scope::User, true, None),
        ("machine-elevated", Scope::Machine, true, Some(2)),
    ];
    let mut manifest = Manifest {
        schema_version: 1,
        entries: vec![],
    };
    for (suffix, scope, elevated, delay) in specs {
        let name = format!("{prefix}-{suffix}");
        let directory = files.path.join(&name);
        let mut entry = desired(name, &exe, scope, elevated, delay);
        if suffix != "machine-run" {
            std::fs::create_dir(&directory).unwrap();
            entry.working_directory = Some(directory.to_string_lossy().into_owned());
        }
        manifest.entries.push(entry);
    }
    let before = backend.inventory().unwrap();
    assert!(before.entries.iter().all(|e| !e.name.starts_with(&prefix)));
    let mut guard = Cleanup {
        running: vec![],
        backend,
        entries: vec![],
        folders: vec![],
        original_acl: vec![],
    };
    for entry in &manifest.entries {
        assert!(entry.name.starts_with("LineupTest-"));
        let source = if entry.elevated || entry.delay_seconds.is_some() {
            Source::Task
        } else {
            Source::Run
        };
        let locator = if source == Source::Task {
            guard.backend.task_locator(entry.scope, &entry.name)
        } else {
            entry.name.clone()
        };
        let mut cleanup = Entry::new(&entry.name, entry.scope, source, &locator);
        cleanup.elevated = entry.elevated;
        guard.entries.push(cleanup);
    }
    unsafe {
        let root = service().GetFolder(&BSTR::from(r"\")).unwrap();
        let user_folder = guard
            .backend
            .task_locator(Scope::User, "unused")
            .rsplit_once('\\')
            .unwrap()
            .0
            .to_owned();
        for path in [
            r"\Lineup".to_owned(),
            user_folder,
            r"\Lineup\Machine".to_owned(),
        ] {
            match root.GetFolder(&BSTR::from(&path)) {
                Ok(folder) if path == r"\Lineup\Machine" => guard
                    .original_acl
                    .push((path, folder.GetSecurityDescriptor(7).unwrap().to_string())),
                Ok(_) => {}
                Err(error) if missing(&error) => guard.folders.push(path),
                Err(error) => panic!("cannot inspect test folder: {error}"),
            }
        }
    }
    let report = management::apply(&mut guard.backend, &manifest).unwrap();
    assert_eq!(report.status, "success", "{report:?}");
    verify_definitions(&guard.backend, &manifest, &current_sid());
    for entry in &mut manifest.entries {
        if entry.scope == Scope::Machine && entry.delay_seconds.is_some() {
            entry.delay_seconds = Some(entry.delay_seconds.unwrap() + 1);
        }
    }
    let report = management::apply(&mut guard.backend, &manifest).unwrap();
    assert_eq!(report.status, "success", "{report:?}");
    verify_definitions(&guard.backend, &manifest, &current_sid());
    assert!(
        management::apply(&mut guard.backend, &manifest)
            .unwrap()
            .results
            .iter()
            .all(|r| r.status == "unchanged")
    );
    let read_dir = files.path.join(format!("{prefix}-read"));
    std::fs::create_dir(&read_dir).unwrap();
    std::fs::write(
        read_dir.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let split_token = limited::run(&exe, &read_dir).unwrap();
    if split_token {
        assert!(read_dir.join("read-ok").is_file());
    }
    unsafe {
        let root = service().GetFolder(&BSTR::from(r"\")).unwrap();
        for entry in &manifest.entries {
            if entry.delay_seconds.is_none() && !entry.elevated {
                continue;
            }
            let task = root
                .GetTask(&BSTR::from(
                    guard.backend.task_locator(entry.scope, &entry.name),
                ))
                .unwrap();
            let running = task.Run(&VARIANT::default()).unwrap();
            guard.running.push(running.clone());
            let result_file =
                Path::new(entry.working_directory.as_ref().unwrap()).join("elevation.json");
            let start = Instant::now();
            while !result_file.is_file() || task.GetInstances(0).unwrap().Count().unwrap() != 0 {
                assert!(
                    start.elapsed() < Duration::from_secs(30),
                    "owned task probe timed out"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
            assert_eq!(task.LastTaskResult().unwrap(), 0);
            let result: serde_json::Value = serde_json::from_slice(
                &std::fs::read(
                    Path::new(entry.working_directory.as_ref().unwrap()).join("elevation.json"),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(result["elevated"], entry.elevated || !split_token);
            assert!(result["pid"].as_u64().unwrap() > 0);
        }
    }
    guard.clean().unwrap();
    let after: Inventory = guard.backend.inventory().unwrap();
    assert!(after.entries.iter().all(|e| !e.name.starts_with(&prefix)));
    assert_eq!(before.entries, after.entries);
}
