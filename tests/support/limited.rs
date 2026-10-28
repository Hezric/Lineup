use std::path::Path;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::*,
        System::Threading::*,
    },
    core::{PCWSTR, PWSTR},
};
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct Child(Handle);
impl Drop for Child {
    fn drop(&mut self) {
        unsafe {
            if WaitForSingleObject(self.0.0, 0).0 != 0 {
                let _ = TerminateProcess(self.0.0, 1);
                let _ = WaitForSingleObject(self.0.0, 5000);
            }
        }
    }
}
// No linked token means Windows cannot lower this session's scheduled-task elevation.
pub fn run(exe: &Path, directory: &Path) -> Result<bool, String> {
    unsafe {
        let child = (|| -> windows::core::Result<Option<Child>> {
            let mut handle = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle)?;
            let original = Handle(handle);
            let mut elevation = TOKEN_ELEVATION_TYPE::default();
            let mut length = 0;
            GetTokenInformation(
                original.0,
                TokenElevationType,
                Some((&mut elevation as *mut TOKEN_ELEVATION_TYPE).cast()),
                std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32,
                &mut length,
            )?;
            if elevation == TokenElevationTypeDefault {
                return Ok(None);
            }
            let mut linked = TOKEN_LINKED_TOKEN::default();
            GetTokenInformation(
                original.0,
                TokenLinkedToken,
                Some((&mut linked as *mut TOKEN_LINKED_TOKEN).cast()),
                std::mem::size_of::<TOKEN_LINKED_TOKEN>() as u32,
                &mut length,
            )?;
            let token = Handle(linked.LinkedToken);
            let application: Vec<u16> = exe
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut command: Vec<u16> = format!(
                "\"{}\" --ignored --exact unelevated_read_probe --nocapture",
                exe.display()
            )
            .encode_utf16()
            .chain(Some(0))
            .collect();
            let directory: Vec<u16> = directory
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let startup = STARTUPINFOW {
                cb: std::mem::size_of::<STARTUPINFOW>() as u32,
                ..Default::default()
            };
            let mut process = PROCESS_INFORMATION::default();
            CreateProcessWithTokenW(
                token.0,
                CREATE_PROCESS_LOGON_FLAGS(0),
                PCWSTR(application.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                CREATE_NO_WINDOW,
                None,
                PCWSTR(directory.as_ptr()),
                &startup,
                &mut process,
            )?;
            let _thread = Handle(process.hThread);
            Ok(Some(Child(Handle(process.hProcess))))
        })()
        .map_err(|e| e.to_string())?;
        let Some(child) = child else {
            eprintln!("ordinary-token runtime checks unavailable: this account has no split token");
            return Ok(false);
        };
        if WaitForSingleObject(child.0.0, 30000).0 != 0 {
            return Err("owned limited-token probe timed out".into());
        }
        let mut status = 0;
        GetExitCodeProcess(child.0.0, &mut status).map_err(|e| e.to_string())?;
        if status != 0 {
            return Err(format!("limited-token probe failed with exit {status}"));
        }
        Ok(true)
    }
}
