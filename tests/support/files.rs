use std::path::PathBuf;
use windows::{Win32::System::Registry::*, core::PCWSTR};
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn string(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())])
}
pub struct TestFiles {
    pub path: PathBuf,
    root: PathBuf,
}
impl TestFiles {
    pub fn new() -> Self {
        for (hive, key) in [
            (
                HKEY_LOCAL_MACHINE,
                r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            ),
            (HKEY_CURRENT_USER, "Environment"),
        ] {
            let mut data = vec![0u16; 32768];
            let mut size = (data.len() * 2) as u32;
            let status = unsafe {
                RegGetValueW(
                    hive,
                    PCWSTR(wide(key).as_ptr()),
                    windows::core::w!("TEMP"),
                    RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                    None,
                    Some(data.as_mut_ptr().cast()),
                    Some(&mut size),
                )
            };
            if status.ok().is_ok() && !string(&data).is_empty() {
                let directory = PathBuf::from(string(&data));
                let root = directory.canonicalize().unwrap();
                let path = directory.join(format!(
                    "LineupTest-resolution-{}-{} Program Files",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                std::fs::create_dir(&path).unwrap();
                return Self { path, root };
            }
        }
        panic!("Machine or User TEMP must be configured");
    }
    pub fn file(&self, name: &str) -> PathBuf {
        let path = self.path.join(name);
        std::fs::write(&path, []).unwrap();
        path
    }
}
impl Drop for TestFiles {
    fn drop(&mut self) {
        let target = self.path.canonicalize().unwrap();
        assert_eq!(target.parent(), Some(self.root.as_path()));
        std::fs::remove_dir_all(target).unwrap();
        assert!(!self.path.exists());
    }
}
