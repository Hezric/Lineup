//! Native Windows adapter. COM objects never leave the initializing thread.
use crate::{Backend, Entry, Inventory, Scope, Source, split_command};
use std::{
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, HANDLE, HLOCAL, LocalFree,
        },
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::{
            Com::*,
            Environment::ExpandEnvironmentStringsW,
            Registry::*,
            TaskScheduler::*,
            Threading::{GetCurrentProcess, OpenProcessToken},
            Variant::VARIANT,
        },
        UI::Shell::{
            FOLDERID_CommonStartup, FOLDERID_Startup, IShellLinkW, KF_FLAG_DEFAULT,
            SHGetKnownFolderPath, ShellLink,
        },
    },
    core::{BSTR, Interface, PCWSTR, PWSTR},
};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn string(buf: &[u16]) -> String {
    String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(buf.len())])
}
fn text(get: impl FnOnce(*mut BSTR) -> windows::core::Result<()>) -> windows::core::Result<String> {
    let mut value = BSTR::new();
    get(&mut value)?;
    Ok(value.to_string())
}
fn com_value<T: Default>(
    get: impl FnOnce(*mut T) -> windows::core::Result<()>,
) -> windows::core::Result<T> {
    let mut value = T::default();
    get(&mut value)?;
    Ok(value)
}
fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn root(scope: Scope) -> HKEY {
    if scope == Scope::User {
        HKEY_CURRENT_USER
    } else {
        HKEY_LOCAL_MACHINE
    }
}

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
fn open(scope: Scope, path: &str, access: REG_SAM_FLAGS) -> Result<Option<Key>, String> {
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            root(scope),
            PCWSTR(wide(path).as_ptr()),
            None,
            access,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok().map_err(message)?;
    Ok(Some(Key(key)))
}

pub struct WindowsBackend {
    sid: String,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for WindowsBackend {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

impl WindowsBackend {
    pub fn new() -> Result<Self, String> {
        let sid = current_sid()?;
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .map_err(message)?;
        }
        Ok(Self {
            sid,
            _thread: PhantomData,
        })
    }

    pub fn startup_folder(&self, scope: Scope) -> Result<PathBuf, String> {
        unsafe {
            let id = if scope == Scope::User {
                &FOLDERID_Startup
            } else {
                &FOLDERID_CommonStartup
            };
            let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).map_err(message)?;
            let result = path.to_string().map(PathBuf::from).map_err(message);
            CoTaskMemFree(Some(path.0.cast()));
            result
        }
    }

    fn approval(&self, entry: &mut Entry, out: &mut Inventory) -> Result<(), String> {
        let leaf = match entry.source {
            Source::Run => "Run",
            Source::Run32 => "Run32",
            _ => "StartupFolder",
        };
        let path = format!(r"{APPROVED}\{leaf}");
        let Some(key) = open(entry.scope, &path, KEY_QUERY_VALUE | KEY_WOW64_64KEY)? else {
            return Ok(());
        };
        let mut data = vec![0; 1024];
        let mut len = data.len() as u32;
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                PCWSTR(wide(&entry.locator).as_ptr()),
                None,
                None,
                Some(data.as_mut_ptr()),
                Some(&mut len),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        status.ok().map_err(message)?;
        out.read_startup_approval(entry, &data[..len as usize]);
        Ok(())
    }

    fn scan_run(&self, scope: Scope, source: Source, out: &mut Inventory) -> Result<(), String> {
        let view = if source == Source::Run32 {
            KEY_WOW64_32KEY
        } else {
            KEY_WOW64_64KEY
        };
        let Some(key) = open(scope, RUN, KEY_QUERY_VALUE | view)? else {
            return Ok(());
        };
        for index in 0.. {
            let mut name = vec![0; 16384];
            let mut name_len = name.len() as u32;
            let mut data = vec![0; 131072];
            let mut len = data.len() as u32;
            let mut kind = 0;
            let status = unsafe {
                RegEnumValueW(
                    key.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut name_len,
                    None,
                    Some(&mut kind),
                    Some(data.as_mut_ptr()),
                    Some(&mut len),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            status.ok().map_err(message)?;
            let name = String::from_utf16_lossy(&name[..name_len as usize]);
            if kind != REG_SZ.0 && kind != REG_EXPAND_SZ.0 {
                out.warn(
                    Some(scope),
                    Some(&name),
                    format!("{scope:?}/{source:?}/{name}: unsupported registry value type {kind}"),
                );
                continue;
            }
            let raw: Vec<u16> = data[..len as usize]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            let (path, arguments) = split_command(&expand(&string(&raw)));
            let mut entry = Entry::new(&name, scope, source, &name);
            entry.path = resolve(&path);
            entry.arguments = arguments;
            if let Err(error) = self.approval(&mut entry, out) {
                entry.enabled = None;
                out.warn(
                    Some(scope),
                    Some(&entry.name),
                    format!("{}: {error}", entry.id),
                );
            }
            out.entries.push(entry);
        }
        Ok(())
    }

    fn scan_startup(&self, scope: Scope, out: &mut Inventory) -> Result<(), String> {
        let folder = self.startup_folder(scope)?;
        let files = match std::fs::read_dir(folder) {
            Ok(files) => files,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(message(e)),
        };
        for file in files {
            let file = file.map_err(message)?;
            let path = file.path();
            if file.file_type().map_err(message)?.is_dir()
                || file
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("desktop.ini")
            {
                continue;
            }
            let locator = file.file_name().to_string_lossy().into_owned();
            let name = if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
            {
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            } else {
                locator.clone()
            };
            let mut entry = Entry::new(&name, scope, Source::Startup, &locator);
            entry.path = path.to_string_lossy().into_owned();
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
                && let Err(error) = read_shortcut(&path, &mut entry)
            {
                out.warn(
                    Some(scope),
                    Some(&entry.name),
                    format!("{}: {error}", entry.id),
                );
            }
            if let Err(error) = self.approval(&mut entry, out) {
                entry.enabled = None;
                out.warn(
                    Some(scope),
                    Some(&entry.name),
                    format!("{}: {error}", entry.id),
                );
            }
            out.entries.push(entry);
        }
        Ok(())
    }

    fn scan_tasks(
        &self,
        folder: &ITaskFolder,
        out: &mut Inventory,
        visited: &mut std::collections::BTreeSet<String>,
    ) {
        unsafe {
            let Some(path) = task_read(folder.Path(), "task folder path", None, None, out)
                .map(|p| p.to_string())
            else {
                return;
            };
            if path.eq_ignore_ascii_case(r"\Microsoft") || !visited.insert(path.to_lowercase()) {
                return;
            }
            let scope = task_folder_scope(&path);
            let mut task_names = std::collections::BTreeSet::new();
            if let Some(tasks) = task_read(
                folder.GetTasks(TASK_ENUM_HIDDEN.0),
                &format!("tasks {path}"),
                scope,
                None,
                out,
            ) {
                let count = task_read(
                    tasks.Count(),
                    &format!("task count {path}"),
                    scope,
                    None,
                    out,
                )
                .unwrap_or(0);
                for i in 1..=count {
                    let context = format!("task {path} index {i}");
                    if let Some(task) = task_read(
                        tasks.get_Item(&VARIANT::from(i)),
                        &context,
                        scope,
                        None,
                        out,
                    ) {
                        let name = task_read(task.Name(), &context, scope, None, out)
                            .map(|n| n.to_string());
                        if let Some(name) = &name {
                            task_names.insert(name.to_lowercase());
                        }
                        task_read(
                            self.read_task(&task, out),
                            &context,
                            scope,
                            name.as_deref(),
                            out,
                        );
                    }
                }
            }
            if let Some(folders) = task_read(
                folder.GetFolders(0),
                &format!("task folders {path}"),
                scope,
                None,
                out,
            ) {
                let count = task_read(
                    folders.Count(),
                    &format!("folder count {path}"),
                    scope,
                    None,
                    out,
                )
                .unwrap_or(0);
                for i in 1..=count {
                    if let Some(child) = task_read(
                        folders.get_Item(&VARIANT::from(i)),
                        &format!("task folder {path} index {i}"),
                        scope,
                        None,
                        out,
                    ) {
                        self.scan_tasks(&child, out, visited);
                    }
                }
            }
            // COM collections may filter unreadable objects; use names from the store when available.
            if let Some(root) = std::env::var_os("SystemRoot") {
                let store = PathBuf::from(root)
                    .join("System32/Tasks")
                    .join(path.trim_start_matches('\\'));
                if let Ok(files) = std::fs::read_dir(store) {
                    for file in files {
                        let file = match file {
                            Ok(file) => file,
                            Err(error) => {
                                out.warn(scope, None, format!("task-store entry {path}: {error}"));
                                continue;
                            }
                        };
                        let name = file.file_name().to_string_lossy().into_owned();
                        let full = format!("{}\\{name}", path.trim_end_matches('\\'));
                        match file.file_type() {
                            Ok(kind)
                                if kind.is_dir()
                                    && !visited.contains(&full.to_lowercase())
                                    && !full.eq_ignore_ascii_case(r"\Microsoft") =>
                            {
                                if let Some(child) = task_read(
                                    folder.GetFolder(&BSTR::from(&name)),
                                    &format!("task folder {full}"),
                                    task_folder_scope(&full),
                                    None,
                                    out,
                                ) {
                                    self.scan_tasks(&child, out, visited);
                                }
                            }
                            Ok(kind)
                                if kind.is_file() && !task_names.contains(&name.to_lowercase()) =>
                            {
                                match folder.GetTask(&BSTR::from(&name)) {
                                    Ok(task) => {
                                        task_read(
                                            self.read_task(&task, out),
                                            &format!("task {full}"),
                                            scope,
                                            Some(&name),
                                            out,
                                        );
                                    }
                                    Err(error) if task_missing(&error) => {}
                                    Err(error) => {
                                        task_read::<()>(
                                            Err(error),
                                            &format!("task {full}"),
                                            scope,
                                            Some(&name),
                                            out,
                                        );
                                    }
                                }
                            }
                            Err(error) => {
                                out.warn(scope, None, format!("task-store entry {full}: {error}"))
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    fn write_task(&self, entry: &Entry) -> Result<(), String> {
        let (sddl, flags) = task_registration(entry.scope);
        unsafe {
            let service = scheduler().map_err(message)?;
            let root = service.GetFolder(&BSTR::from(r"\")).map_err(message)?;
            let mut folder = root;
            let folder_path = entry
                .locator
                .rsplit_once('\\')
                .ok_or("invalid task locator")?
                .0;
            for component in folder_path.split('\\').filter(|s| !s.is_empty()) {
                folder = match folder.GetFolder(&BSTR::from(component)) {
                    Ok(folder) => folder,
                    Err(error)
                        if error.code().0 as u32 == 0x80070002
                            || error.code().0 as u32 == 0x80070003 =>
                    {
                        folder
                            .CreateFolder(&BSTR::from(component), &VARIANT::default())
                            .map_err(message)?
                    }
                    Err(error) => return Err(message(error)),
                };
            }
            if let Some(sddl) = sddl {
                folder
                    .SetSecurityDescriptor(&BSTR::from(sddl), 0)
                    .map_err(message)?;
            }
            let definition = self.task_definition(entry)?;
            let principal = definition.Principal().map_err(message)?;
            let identity = text(|p| {
                if entry.scope == Scope::Machine {
                    principal.GroupId(p)
                } else {
                    principal.UserId(p)
                }
            })
            .map_err(message)?;
            let logon_type = com_value(|p| principal.LogonType(p)).map_err(message)?;
            folder
                .RegisterTaskDefinition(
                    &BSTR::from(&entry.name),
                    &definition,
                    flags,
                    &VARIANT::from(identity.as_str()),
                    &VARIANT::default(),
                    logon_type,
                    &sddl.map(VARIANT::from).unwrap_or_default(),
                )
                .map_err(message)?;
            Ok(())
        }
    }

    fn task_definition(&self, entry: &Entry) -> Result<ITaskDefinition, String> {
        unsafe {
            let service = scheduler().map_err(message)?;
            let definition = service.NewTask(0).map_err(message)?;
            let principal = definition.Principal().map_err(message)?;
            let identity = if entry.scope == Scope::Machine {
                if entry.elevated {
                    "S-1-5-32-544"
                } else {
                    "S-1-5-32-545"
                }
            } else {
                &self.sid
            };
            let logon_type = if entry.scope == Scope::Machine {
                TASK_LOGON_GROUP
            } else {
                TASK_LOGON_INTERACTIVE_TOKEN
            };
            if entry.scope == Scope::Machine {
                principal
                    .SetGroupId(&BSTR::from(identity))
                    .map_err(message)?;
            } else {
                principal
                    .SetUserId(&BSTR::from(identity))
                    .map_err(message)?;
            }
            principal.SetLogonType(logon_type).map_err(message)?;
            principal
                .SetRunLevel(if entry.elevated {
                    TASK_RUNLEVEL_HIGHEST
                } else {
                    TASK_RUNLEVEL_LUA
                })
                .map_err(message)?;
            let trigger: ILogonTrigger = definition
                .Triggers()
                .map_err(message)?
                .Create(TASK_TRIGGER_LOGON)
                .map_err(message)?
                .cast()
                .map_err(message)?;
            if entry.scope == Scope::User {
                trigger.SetUserId(&BSTR::from(&self.sid)).map_err(message)?;
            }
            trigger
                .SetDelay(&BSTR::from(format!(
                    "PT{}S",
                    entry.delay_seconds.unwrap_or(0)
                )))
                .map_err(message)?;
            let settings = definition.Settings().map_err(message)?;
            settings.SetEnabled(true.into()).map_err(message)?;
            settings
                .SetDisallowStartIfOnBatteries(false.into())
                .map_err(message)?;
            settings
                .SetStopIfGoingOnBatteries(false.into())
                .map_err(message)?;
            settings
                .SetExecutionTimeLimit(&BSTR::from("PT0S"))
                .map_err(message)?;
            settings
                .SetPriority(entry.priority.unwrap_or(7) as i32)
                .map_err(message)?;
            let action: IExecAction = definition
                .Actions()
                .map_err(message)?
                .Create(TASK_ACTION_EXEC)
                .map_err(message)?
                .cast()
                .map_err(message)?;
            action.SetPath(&BSTR::from(&entry.path)).map_err(message)?;
            action
                .SetArguments(&BSTR::from(&entry.arguments))
                .map_err(message)?;
            if let Some(dir) = &entry.working_directory {
                action
                    .SetWorkingDirectory(&BSTR::from(dir))
                    .map_err(message)?;
            }
            Ok(definition)
        }
    }

    fn read_task(&self, task: &IRegisteredTask, out: &mut Inventory) -> windows::core::Result<()> {
        unsafe {
            self.read_task_definition(
                &task.Definition()?,
                &task.Name()?.to_string(),
                &task.Path()?.to_string(),
                task.Enabled()?.as_bool(),
                out,
            )
        }
    }

    fn read_task_definition(
        &self,
        definition: &ITaskDefinition,
        name: &str,
        locator: &str,
        enabled: bool,
        out: &mut Inventory,
    ) -> windows::core::Result<()> {
        unsafe {
            let principal = definition.Principal()?;
            let user = text(|p| principal.UserId(p))?;
            let group = text(|p| principal.GroupId(p))?;
            let scope = if user.eq_ignore_ascii_case(&self.sid)
                || account_sid(&user).is_some_and(|s| s == self.sid)
            {
                Scope::User
            } else if !group.is_empty()
                || [
                    "S-1-5-18",
                    "S-1-5-19",
                    "S-1-5-20",
                    "SYSTEM",
                    "LOCAL SERVICE",
                    "NETWORK SERVICE",
                ]
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&user))
            {
                Scope::Machine
            } else {
                return Ok(());
            };
            let triggers = definition.Triggers()?;
            let count: i32 = com_value(|p| triggers.Count(p))?;
            let mut logon = false;
            let mut boot = false;
            let mut delay = None;
            for i in 1..=count {
                let trigger = triggers.get_Item(i)?;
                let kind = com_value(|p| trigger.Type(p))?;
                if kind == TASK_TRIGGER_LOGON {
                    let trigger: ILogonTrigger = trigger.cast()?;
                    let user = text(|p| trigger.UserId(p))?;
                    if !user.is_empty()
                        && user != self.sid
                        && account_sid(&user).is_none_or(|sid| sid != self.sid)
                    {
                        continue;
                    }
                    logon = true;
                    let value = text(|p| trigger.Delay(p))?;
                    delay = parse_duration(&value);
                } else if kind == TASK_TRIGGER_BOOT {
                    boot = true;
                    if !logon {
                        let trigger: IBootTrigger = trigger.cast()?;
                        delay = parse_duration(&text(|p| trigger.Delay(p))?);
                    }
                }
            }
            if !logon && !boot {
                return Ok(());
            }
            let settings = definition.Settings()?;
            let priority: i32 = com_value(|p| settings.Priority(p))?;
            let actions = definition.Actions()?;
            let count: i32 = com_value(|p| actions.Count(p))?;
            let mut found = false;
            for i in 1..=count {
                let action = actions.get_Item(i)?;
                if com_value(|p| action.Type(p))? != TASK_ACTION_EXEC {
                    continue;
                }
                found = true;
                let action: IExecAction = action.cast()?;
                let mut entry = Entry::new(name, scope, Source::Task, locator);
                if count > 1 {
                    entry.id.push_str(&format!("#{i}"));
                }
                let dir = text(|p| action.WorkingDirectory(p))?;
                entry.working_directory = (!dir.is_empty()).then(|| expand(&dir));
                entry.path = resolve_in(
                    &text(|p| action.Path(p))?,
                    entry.working_directory.as_deref(),
                );
                entry.arguments = expand(&text(|p| action.Arguments(p))?);
                entry.enabled = Some(enabled);
                entry.elevated = com_value(|p| principal.RunLevel(p))? == TASK_RUNLEVEL_HIGHEST
                    || user == "S-1-5-18";
                entry.delay_seconds = delay;
                entry.priority = Some(priority as u32);
                entry.boot = boot;
                out.entries.push(entry);
            }
            if !found {
                out.warn(
                    Some(scope),
                    Some(name),
                    format!(
                        "task {locator}: no executable action (COM handler or unsupported action)"
                    ),
                );
            }
            Ok(())
        }
    }
}

impl Backend for WindowsBackend {
    // Task Scheduler collections can omit an unreadable task without returning an error.
    fn ensure_task_absent(&self, locator: &str) -> Result<(), String> {
        unsafe {
            match scheduler().and_then(|service| {
                service
                    .GetFolder(&BSTR::from(r"\"))?
                    .GetTask(&BSTR::from(locator))
            }) {
                Err(error) if task_missing(&error) => Ok(()),
                Err(error) => Err(format!(
                    "task {locator}: inventory cannot confirm absence: {error}"
                )),
                Ok(_) => Err(format!(
                    "task {locator} exists but was not enumerated; inventory is incomplete"
                )),
            }
        }
    }
    fn is_elevated(&self) -> Result<bool, String> {
        use windows::Win32::Security::{TOKEN_ELEVATION, TokenElevation};
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(message)?;
            let mut info = TOKEN_ELEVATION::default();
            let mut len = 0;
            let result = GetTokenInformation(
                token,
                TokenElevation,
                Some((&mut info as *mut TOKEN_ELEVATION).cast()),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            );
            let _ = CloseHandle(token);
            result.map_err(message)?;
            Ok(info.TokenIsElevated != 0)
        }
    }
    fn expand(&self, value: &str) -> String {
        expand(value)
    }
    fn task_locator(&self, scope: Scope, name: &str) -> String {
        match scope {
            Scope::User => format!(r"\Lineup\User-{}\{name}", self.sid),
            Scope::Machine => format!(r"\Lineup\Machine\{name}"),
        }
    }
    fn write(&mut self, entry: &Entry) -> Result<(), String> {
        if (entry.scope == Scope::Machine || entry.elevated) && !self.is_elevated()? {
            return Err("elevation required".into());
        }
        match entry.source {
            Source::Run => {
                let command = format!(
                    "\"{}\"{}{}",
                    entry.path,
                    if entry.arguments.is_empty() { "" } else { " " },
                    entry.arguments
                );
                if command.encode_utf16().count() > 260 {
                    return Err("Run command exceeds Windows' 260-character limit; use a task by specifying delaySeconds: 0".into());
                }
                let key = create_key(entry.scope, RUN)?;
                let data: Vec<u8> = wide(&command)
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect();
                unsafe {
                    RegSetValueExW(
                        key.0,
                        PCWSTR(wide(&entry.name).as_ptr()),
                        None,
                        REG_SZ,
                        Some(&data),
                    )
                    .ok()
                    .map_err(message)?;
                }
                clear_approval(entry)
            }
            Source::Task => self.write_task(entry),
            _ => Err("new entries must use Run or Task".into()),
        }
    }
    fn delete(&mut self, entry: &Entry) -> Result<(), String> {
        if (entry.scope == Scope::Machine || entry.elevated) && !self.is_elevated()? {
            return Err("elevation required".into());
        }
        match entry.source {
            Source::Run | Source::Run32 => {
                let view = if entry.source == Source::Run32 {
                    KEY_WOW64_32KEY
                } else {
                    KEY_WOW64_64KEY
                };
                if let Some(key) = open(entry.scope, RUN, KEY_SET_VALUE | view)? {
                    delete_value(&key, &entry.locator)?;
                }
                clear_approval(entry)
            }
            Source::Startup => {
                if Path::new(&entry.locator)
                    .file_name()
                    .is_none_or(|f| f != entry.locator.as_str())
                {
                    return Err("invalid Startup file locator".into());
                }
                match std::fs::remove_file(self.startup_folder(entry.scope)?.join(&entry.locator)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(message(e)),
                }
                clear_approval(entry)
            }
            Source::Task => unsafe {
                let result = scheduler().and_then(|service| {
                    service
                        .GetFolder(&BSTR::from(r"\"))?
                        .DeleteTask(&BSTR::from(&entry.locator), 0)
                });
                match result {
                    Ok(()) => Ok(()),
                    Err(e)
                        if [0x80070002, 0x80070003, 0x8004130f].contains(&(e.code().0 as u32)) =>
                    {
                        Ok(())
                    }
                    Err(e) => Err(message(e)),
                }
            },
        }
    }
    fn start(&self, entry: &Entry) -> Result<u32, String> {
        use std::os::windows::process::CommandExt;
        if entry.elevated && !self.is_elevated()? {
            return Err("run this command elevated to launch an elevated entry".into());
        }
        let mut command =
            std::process::Command::new(resolve_in(&entry.path, entry.working_directory.as_deref()));
        command.raw_arg(&entry.arguments);
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(dir) = &entry.working_directory {
            command.current_dir(expand(dir));
        }
        command.spawn().map(|child| child.id()).map_err(message)
    }
    fn inventory(&self) -> Result<Inventory, String> {
        let mut out = Inventory::default();
        for scope in [Scope::User, Scope::Machine] {
            // HKCU Run is shared by the WOW64 views; HKLM Run is redirected.
            for source in [Source::Run, Source::Run32] {
                if scope == Scope::User && source == Source::Run32 {
                    continue;
                }
                if let Err(error) = self.scan_run(scope, source, &mut out) {
                    out.warn(
                        Some(scope),
                        None,
                        format!("registry {scope:?}/{source:?}: {error}"),
                    );
                }
            }
            if let Err(error) = self.scan_startup(scope, &mut out) {
                out.warn(Some(scope), None, format!("startup {scope:?}: {error}"));
            }
        }
        let result = scheduler().and_then(|service| unsafe {
            let mut visited = std::collections::BTreeSet::new();
            self.scan_tasks(
                &service.GetFolder(&BSTR::from(r"\"))?,
                &mut out,
                &mut visited,
            );
            if !visited.contains(r"\lineup\machine") {
                match service.GetFolder(&BSTR::from(r"\Lineup\Machine")) {
                    Ok(folder) => self.scan_tasks(&folder, &mut out, &mut visited),
                    Err(error) if task_missing(&error) => {}
                    Err(error) => {
                        task_read::<()>(
                            Err(error),
                            r"task folder \Lineup\Machine",
                            Some(Scope::Machine),
                            None,
                            &mut out,
                        );
                    }
                }
            }
            Ok(())
        });
        if let Err(error) = result {
            out.warn(None, None, format!("scheduled tasks: {error}"));
        }
        out.entries.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }
    fn target_exists(&self, path: &str) -> bool {
        !path.is_empty() && Path::new(&resolve(path)).is_file()
    }
}

fn task_registration(scope: Scope) -> (Option<&'static str>, i32) {
    if scope == Scope::Machine {
        (
            Some("O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;AU)"),
            TASK_CREATE_OR_UPDATE.0 | TASK_DONT_ADD_PRINCIPAL_ACE.0,
        )
    } else {
        (None, TASK_CREATE_OR_UPDATE.0)
    }
}
fn task_folder_scope(path: &str) -> Option<Scope> {
    let path = path.to_ascii_lowercase();
    if path == r"\lineup\machine" || path.starts_with(r"\lineup\machine\") {
        Some(Scope::Machine)
    } else if path.starts_with(r"\lineup\user-") {
        Some(Scope::User)
    } else {
        None
    }
}
fn task_missing(error: &windows::core::Error) -> bool {
    [0x80070002, 0x80070003].contains(&(error.code().0 as u32))
}
fn task_read<T>(
    result: windows::core::Result<T>,
    context: &str,
    scope: Option<Scope>,
    name: Option<&str>,
    out: &mut Inventory,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            out.warn(scope, name, format!("{context}: {error}"));
            None
        }
    }
}

fn scheduler() -> windows::core::Result<ITaskService> {
    unsafe {
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        let empty = VARIANT::default();
        service.Connect(&empty, &empty, &empty, &empty)?;
        Ok(service)
    }
}

fn create_key(scope: Scope, path: &str) -> Result<Key, String> {
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            root(scope),
            PCWSTR(wide(path).as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            None,
            &mut key,
            None,
        )
        .ok()
        .map_err(message)?;
    }
    Ok(Key(key))
}
fn delete_value(key: &Key, name: &str) -> Result<(), String> {
    let status = unsafe { RegDeleteValueW(key.0, PCWSTR(wide(name).as_ptr())) };
    if status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        status.ok().map_err(message)
    }
}
fn clear_approval(entry: &Entry) -> Result<(), String> {
    let leaf = match entry.source {
        Source::Run => "Run",
        Source::Run32 => "Run32",
        Source::Startup => "StartupFolder",
        Source::Task => return Ok(()),
    };
    if let Some(key) = open(
        entry.scope,
        &format!(r"{APPROVED}\{leaf}"),
        KEY_SET_VALUE | KEY_WOW64_64KEY,
    )? {
        delete_value(&key, &entry.locator)?;
    }
    Ok(())
}

fn current_sid() -> Result<String, String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(message)?;
        let result = (|| {
            let mut len = 0;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            let mut data = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
            GetTokenInformation(
                token,
                TokenUser,
                Some(data.as_mut_ptr().cast()),
                len,
                &mut len,
            )
            .map_err(message)?;
            let user = &*(data.as_ptr().cast::<TOKEN_USER>());
            sid_string(user.User.Sid).map_err(message)
        })();
        let _ = CloseHandle(token);
        result
    }
}

fn sid_string(sid: windows::Win32::Security::PSID) -> windows::core::Result<String> {
    unsafe {
        let mut ptr = PWSTR::null();
        ConvertSidToStringSidW(sid, &mut ptr)?;
        let result = ptr.to_string();
        let _ = LocalFree(Some(HLOCAL(ptr.0.cast())));
        Ok(result?)
    }
}
fn account_sid(account: &str) -> Option<String> {
    use windows::Win32::Security::{LookupAccountNameW, PSID, SID_NAME_USE};
    if account.is_empty() {
        return None;
    }
    unsafe {
        let mut sid_len = 0;
        let mut domain_len = 0;
        let mut kind = SID_NAME_USE::default();
        let name = wide(account);
        let _ = LookupAccountNameW(
            PCWSTR::null(),
            PCWSTR(name.as_ptr()),
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
            PCWSTR(name.as_ptr()),
            Some(ptr),
            &mut sid_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &mut domain_len,
            &mut kind,
        )
        .ok()?;
        sid_string(ptr).ok()
    }
}

pub fn expand(value: &str) -> String {
    unsafe {
        let input = wide(value);
        let len = ExpandEnvironmentStringsW(PCWSTR(input.as_ptr()), None);
        if len == 0 {
            return value.into();
        }
        let mut output = vec![0; len as usize];
        if ExpandEnvironmentStringsW(PCWSTR(input.as_ptr()), Some(&mut output)) == 0 {
            return value.into();
        }
        string(&output)
    }
}
pub fn resolve(value: &str) -> String {
    resolve_in(value, None)
}

fn resolve_in(value: &str, working_directory: Option<&str>) -> String {
    let expanded = expand(value);
    let command = expanded
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(&expanded);
    if command.is_empty() {
        return String::new();
    }
    let mut names = vec![PathBuf::from(command)];
    if names[0].extension().is_none() {
        let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        names.extend(
            extensions
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|extension| PathBuf::from(format!("{command}{extension}"))),
        );
    }
    let directories: Vec<_> = if names[0].is_absolute() {
        vec![PathBuf::new()]
    } else {
        let path = std::env::var_os("PATH").unwrap_or_default();
        working_directory
            .filter(|dir| !dir.is_empty())
            .map(|dir| PathBuf::from(expand(dir)))
            .into_iter()
            .chain(std::env::split_paths(&path))
            .collect()
    };
    for directory in directories {
        for name in &names {
            let target = directory.join(name);
            if target.is_file() {
                return target.to_string_lossy().into_owned();
            }
        }
    }
    command.into()
}

fn read_shortcut(path: &Path, entry: &mut Entry) -> Result<(), String> {
    unsafe {
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(message)?;
        let persist: IPersistFile = link.cast().map_err(message)?;
        let path = wide(&path.to_string_lossy());
        persist
            .Load(PCWSTR(path.as_ptr()), STGM_READ)
            .map_err(message)?;
        let mut buffer = vec![0; 32768];
        link.GetPath(&mut buffer, std::ptr::null_mut(), 0)
            .map_err(message)?;
        entry.path = resolve(&string(&buffer));
        buffer.fill(0);
        link.GetArguments(&mut buffer).map_err(message)?;
        entry.arguments = string(&buffer);
        buffer.fill(0);
        link.GetWorkingDirectory(&mut buffer).map_err(message)?;
        let dir = string(&buffer);
        entry.working_directory = (!dir.is_empty()).then_some(dir);
        Ok(())
    }
}

pub fn parse_duration(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    let mut remaining = value.strip_prefix('P')?;
    let mut seconds = 0u32;
    if let Some(index) = remaining.find('D') {
        seconds = remaining[..index].parse::<u32>().ok()?.checked_mul(86400)?;
        remaining = &remaining[index + 1..];
        if remaining.is_empty() {
            return Some(seconds);
        }
    }
    remaining = remaining.strip_prefix('T')?;
    if remaining.is_empty() {
        return None;
    }
    for (suffix, multiplier) in [('H', 3600), ('M', 60), ('S', 1)] {
        if let Some(index) = remaining.find(suffix) {
            seconds = seconds.checked_add(
                remaining[..index]
                    .parse::<u32>()
                    .ok()?
                    .checked_mul(multiplier)?,
            )?;
            remaining = &remaining[index + 1..];
        }
    }
    remaining.is_empty().then_some(seconds)
}

#[cfg(test)]
#[path = "../tests/support/files.rs"]
mod test_files;
#[cfg(test)]
#[path = "../tests/support/security.rs"]
mod test_security;
#[cfg(test)]
mod tests {
    use super::test_files::TestFiles;
    use super::*;
    use crate::management::{self, Action, Manifest};

    #[derive(Default)]
    struct RegistrationBackend {
        entries: Vec<Entry>,
        security: Vec<(Option<&'static str>, i32)>,
        hidden: bool,
        warning: bool,
    }
    impl Backend for RegistrationBackend {
        fn inventory(&self) -> Result<Inventory, String> {
            let mut out = Inventory {
                entries: self.entries.clone(),
                warnings: vec![],
            };
            if self.warning {
                task_read::<()>(
                    Err(windows::core::Error::from_hresult(
                        windows::Win32::Foundation::E_ACCESSDENIED,
                    )),
                    r"task folder \Lineup\Machine",
                    Some(Scope::Machine),
                    None,
                    &mut out,
                );
            }
            Ok(out)
        }
        fn target_exists(&self, _: &str) -> bool {
            true
        }
        fn is_elevated(&self) -> Result<bool, String> {
            Ok(true)
        }
        // Task Scheduler collections can omit an unreadable task without returning an error.
        fn ensure_task_absent(&self, locator: &str) -> Result<(), String> {
            if self.hidden {
                Err(format!("task {locator}: access denied"))
            } else {
                Ok(())
            }
        }
        fn write(&mut self, entry: &Entry) -> Result<(), String> {
            self.security.push(task_registration(entry.scope));
            self.entries.retain(|e| e.id != entry.id);
            self.entries.push(entry.clone());
            Ok(())
        }
    }
    #[test]
    fn known_task_read_failures_warn_and_block_mutations_at_the_backend_seam() {
        let mut backend = RegistrationBackend {
            warning: true,
            ..Default::default()
        };
        let health = crate::check(&backend).unwrap();
        assert_eq!(health.inventory.warnings.len(), 1);
        assert!(
            health.inventory.warnings[0]
                .message
                .contains(r"\Lineup\Machine")
        );
        assert!(
            management::plan(&backend, &machine_manifest())
                .unwrap_err()
                .contains("incomplete")
        );
        assert!(management::apply(&mut backend, &machine_manifest()).is_err());
        assert!(backend.security.is_empty());
    }
    fn machine_manifest() -> Manifest {
        management::parse_manifest(r#"{"schemaVersion":1,"entries":[{"name":"example","path":"C:\\Windows\\notepad.exe","scope":"machine","elevated":true,"delaySeconds":1}]}"#).unwrap()
    }
    #[test]
    fn machine_task_registration_keeps_read_permissions_on_create_and_update() {
        let mut backend = RegistrationBackend::default();
        let mut manifest = machine_manifest();
        assert_eq!(
            management::apply(&mut backend, &manifest).unwrap().status,
            "success"
        );
        manifest.entries[0].arguments = "--updated".into();
        assert_eq!(
            management::plan(&backend, &manifest).unwrap().entries[0].action,
            Action::Update
        );
        assert_eq!(
            management::apply(&mut backend, &manifest).unwrap().status,
            "success"
        );
        assert_eq!(backend.security.len(), 2);
        for (sddl, flags) in backend.security {
            super::test_security::assert_machine_acl(
                sddl.expect("machine tasks need an explicit read policy"),
            );
            assert_ne!(flags & TASK_DONT_ADD_PRINCIPAL_ACE.0, 0);
        }
        assert!(task_registration(Scope::User).0.is_none());
    }
    #[test]
    fn inaccessible_unenumerated_managed_tasks_do_not_plan_as_create() {
        let backend = RegistrationBackend {
            hidden: true,
            ..Default::default()
        };
        let result = management::plan(&backend, &machine_manifest());
        assert!(
            result.is_err(),
            "a known access failure must not become create"
        );
    }

    struct TaskFixture {
        definition: ITaskDefinition,
        native: WindowsBackend,
    }
    impl TaskFixture {
        fn new(command: &str, directory: Option<&Path>) -> Self {
            let native = WindowsBackend::new().unwrap();
            let mut entry = Entry::new(
                "LineupTest-resolution",
                Scope::User,
                Source::Task,
                r"\Lineup\LineupTest-resolution",
            );
            entry.path = command.into();
            entry.working_directory = directory.map(|p| p.to_string_lossy().into_owned());
            let definition = native.task_definition(&entry).unwrap();
            Self { definition, native }
        }
    }
    impl Backend for TaskFixture {
        fn inventory(&self) -> Result<Inventory, String> {
            let mut out = Inventory::default();
            self.native
                .read_task_definition(
                    &self.definition,
                    "LineupTest-resolution",
                    r"\Lineup\LineupTest-resolution",
                    true,
                    &mut out,
                )
                .map_err(message)?;
            Ok(out)
        }
        fn target_exists(&self, path: &str) -> bool {
            self.native.target_exists(path)
        }
    }
    fn assert_task_target(fixture: &TaskFixture, expected: &Path) {
        let health = crate::check(fixture).unwrap();
        assert!(
            health.inventory.entries[0]
                .path
                .eq_ignore_ascii_case(&expected.to_string_lossy()),
            "resolved target differs: {:?} versus {:?}",
            health.inventory.entries[0].path,
            expected
        );
        assert!(health.missing_targets.is_empty());
    }

    #[test]
    fn task_inventory_and_check_unquote_executable_paths() {
        let files = TestFiles::new();
        let target = files.file("Agent.exe");
        let fixture = TaskFixture::new(&format!("\"{}\"", target.display()), None);
        assert_task_target(&fixture, &target);
        assert!(
            fixture
                .native
                .target_exists(&format!("\"{}\"", target.display()))
        );
        let missing = files.path.join("missing.exe");
        let health = crate::check(&TaskFixture::new(
            &format!("\"{}\"", missing.display()),
            None,
        ))
        .unwrap();
        assert_eq!(health.inventory.entries[0].path, missing.to_string_lossy());
        assert_eq!(health.missing_targets.len(), 1);
    }

    #[test]
    fn task_inventory_and_check_resolve_working_directory_before_path_with_pathext() {
        let files = TestFiles::new();
        for (command, file) in [
            ("example", "example.exe"),
            ("notepad", "notepad.exe"),
            ("script", "script.cmd"),
        ] {
            let target = files.file(file);
            let fixture = TaskFixture::new(command, Some(&files.path));
            assert_task_target(&fixture, &target);
        }
    }

    #[test]
    fn task_inventory_and_check_resolve_bare_names_via_path() {
        let files = TestFiles::new();
        let path = std::env::var_os("PATH").unwrap();
        let target = std::env::split_paths(&path)
            .map(|p| p.join("notepad.exe"))
            .find(|p| p.is_file())
            .expect("notepad.exe on Windows PATH");
        for directory in [None, Some(files.path.as_path())] {
            assert_task_target(&TaskFixture::new("notepad", directory), &target);
        }
    }
    #[test]
    fn scheduler_delays_use_iso8601_time_components() {
        assert_eq!(parse_duration("PT1H2M3S"), Some(3723));
        assert_eq!(parse_duration("PT30S"), Some(30));
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("P1D"), Some(86400));
        assert_eq!(parse_duration("P1DT2H3M4S"), Some(93784));
    }

    #[test]
    fn task_inventory_expands_environment_variables_in_all_command_fields() {
        let system_root = std::env::var("SystemRoot").unwrap();
        let fixture = TaskFixture::new(
            r"%SystemRoot%\System32\notepad.exe",
            Some(Path::new("%SystemRoot%")),
        );
        unsafe {
            let action: IExecAction = fixture
                .definition
                .Actions()
                .unwrap()
                .get_Item(1)
                .unwrap()
                .cast()
                .unwrap();
            action.SetArguments(&BSTR::from("%SystemRoot%")).unwrap();
        }
        let inventory = fixture.inventory().unwrap();
        let entry = &inventory.entries[0];
        assert!(
            entry
                .path
                .eq_ignore_ascii_case(&format!(r"{system_root}\System32\notepad.exe"))
        );
        assert_eq!(entry.arguments, system_root);
        assert_eq!(
            entry.working_directory.as_deref(),
            Some(system_root.as_str())
        );
    }

    #[test]
    fn task_inventory_handles_boot_triggers_multiple_actions_and_unsupported_actions() {
        let fixture = TaskFixture::new(r"C:\LineupTest-missing\one.exe", None);
        unsafe {
            let triggers = fixture.definition.Triggers().unwrap();
            triggers.Clear().unwrap();
            let boot: IBootTrigger = triggers.Create(TASK_TRIGGER_BOOT).unwrap().cast().unwrap();
            boot.SetDelay(&BSTR::from("PT12S")).unwrap();
            let actions = fixture.definition.Actions().unwrap();
            let second: IExecAction = actions.Create(TASK_ACTION_EXEC).unwrap().cast().unwrap();
            second
                .SetPath(&BSTR::from(r"C:\LineupTest-missing\two.exe"))
                .unwrap();
            let inventory = fixture.inventory().unwrap();
            assert_eq!(inventory.entries.len(), 2);
            assert!(inventory.entries[0].id.ends_with("#1"));
            assert!(inventory.entries[1].id.ends_with("#2"));
            assert!(
                inventory
                    .entries
                    .iter()
                    .all(|entry| entry.boot && entry.delay_seconds == Some(12))
            );
            actions.Clear().unwrap();
            actions.Create(TASK_ACTION_COM_HANDLER).unwrap();
            let inventory = fixture.inventory().unwrap();
            assert!(inventory.entries.is_empty());
            assert_eq!(inventory.warnings.len(), 1);
            assert_eq!(inventory.warnings[0].scope, Some(Scope::User));
            assert!(
                inventory.warnings[0]
                    .message
                    .contains("no executable action")
            );
        }
    }

    #[test]
    fn task_inventory_excludes_foreign_principals_and_foreign_logon_triggers() {
        let foreign = BSTR::from("S-1-5-21-0-0-0-1001");
        let fixture = TaskFixture::new(r"C:\LineupTest-missing\tool.exe", None);
        unsafe {
            let trigger: ILogonTrigger = fixture
                .definition
                .Triggers()
                .unwrap()
                .get_Item(1)
                .unwrap()
                .cast()
                .unwrap();
            trigger.SetUserId(&foreign).unwrap();
            assert!(fixture.inventory().unwrap().entries.is_empty());
            trigger.SetUserId(&BSTR::from(&fixture.native.sid)).unwrap();
            fixture
                .definition
                .Principal()
                .unwrap()
                .SetUserId(&foreign)
                .unwrap();
            assert!(fixture.inventory().unwrap().entries.is_empty());
        }
    }

    #[test]
    fn task_folder_failures_preserve_the_scope_used_by_mutation_preflight() {
        for (path, scope) in [
            (r"\Lineup\Machine", Some(Scope::Machine)),
            (r"\LINEUP\MACHINE\child", Some(Scope::Machine)),
            (r"\Lineup\User-S-1-5-21-1", Some(Scope::User)),
            (r"\Lineup\MachineOther", None),
            (r"\Other", None),
        ] {
            let mut inventory = Inventory::default();
            task_read::<()>(
                Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                    0x80070005u32 as i32,
                ))),
                path,
                task_folder_scope(path),
                None,
                &mut inventory,
            );
            assert_eq!(inventory.warnings.len(), 1);
            assert_eq!(inventory.warnings[0].scope, scope);
            assert_eq!(inventory.warnings[0].name, None);
        }
    }

    #[test]
    fn native_task_definitions_preserve_scope_privilege_and_logon_settings_without_registration() {
        let backend = WindowsBackend::new().unwrap();
        for (scope, elevated, expected_group) in [
            (Scope::User, false, ""),
            (Scope::User, true, ""),
            (Scope::Machine, false, "S-1-5-32-545"),
            (Scope::Machine, true, "S-1-5-32-544"),
        ] {
            let mut entry = Entry::new("example", scope, Source::Task, "unused");
            entry.path = r"C:\Windows\notepad.exe".into();
            entry.elevated = elevated;
            entry.delay_seconds = Some(17);
            entry.priority = Some(6);
            let definition = backend.task_definition(&entry).unwrap();
            unsafe {
                let principal = definition.Principal().unwrap();
                let group = text(|p| principal.GroupId(p)).unwrap();
                assert_eq!(account_sid(&group).unwrap_or(group), expected_group);
                let user = text(|p| principal.UserId(p)).unwrap();
                assert_eq!(
                    account_sid(&user).unwrap_or(user),
                    if scope == Scope::User {
                        &backend.sid
                    } else {
                        ""
                    }
                );
                assert_eq!(
                    com_value(|p| principal.RunLevel(p)).unwrap(),
                    if elevated {
                        TASK_RUNLEVEL_HIGHEST
                    } else {
                        TASK_RUNLEVEL_LUA
                    }
                );
                assert_eq!(
                    com_value(|p| principal.LogonType(p)).unwrap(),
                    if scope == Scope::User {
                        TASK_LOGON_INTERACTIVE_TOKEN
                    } else {
                        TASK_LOGON_GROUP
                    }
                );
                let trigger: ILogonTrigger = definition
                    .Triggers()
                    .unwrap()
                    .get_Item(1)
                    .unwrap()
                    .cast()
                    .unwrap();
                let user = text(|p| trigger.UserId(p)).unwrap();
                assert_eq!(
                    account_sid(&user).unwrap_or(user),
                    if scope == Scope::User {
                        &backend.sid
                    } else {
                        ""
                    }
                );
                assert_eq!(text(|p| trigger.Delay(p)).unwrap(), "PT17S");
                assert_eq!(
                    com_value(|p| definition.Settings().unwrap().Priority(p)).unwrap(),
                    6
                );
            }
        }
    }
}
