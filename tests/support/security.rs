use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::{Authorization::*, *},
        Storage::FileSystem::{FILE_ALL_ACCESS, FILE_GENERIC_READ},
    },
    core::{PCWSTR, PWSTR},
};
fn sid_text(sid: PSID) -> String {
    unsafe {
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(sid, &mut text).unwrap();
        let value = text.to_string().unwrap();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        value
    }
}
pub fn assert_machine_acl(sddl: &str) {
    unsafe {
        let input: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(input.as_ptr()),
            1,
            &mut descriptor,
            None,
        )
        .unwrap();
        let result = std::panic::catch_unwind(|| {
            let mut owner = PSID::default();
            let mut defaulted = windows::core::BOOL::default();
            GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted).unwrap();
            assert_eq!(sid_text(owner), "S-1-5-32-544");
            let mut control = 0;
            let mut revision = 0;
            GetSecurityDescriptorControl(descriptor, &mut control, &mut revision).unwrap();
            assert_ne!(control & SE_DACL_PROTECTED.0, 0);
            let mut present = windows::core::BOOL::default();
            let mut acl = std::ptr::null_mut();
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted).unwrap();
            assert!(present.as_bool() && !acl.is_null());
            assert_eq!((*acl).AceCount, 3);
            let mut rights = std::collections::BTreeMap::new();
            for i in 0..(*acl).AceCount {
                let mut ptr = std::ptr::null_mut();
                GetAce(acl, u32::from(i), &mut ptr).unwrap();
                let ace = &*ptr.cast::<ACCESS_ALLOWED_ACE>();
                assert_eq!(ace.Header.AceType, 0);
                rights.insert(
                    sid_text(PSID((&ace.SidStart as *const u32).cast_mut().cast())),
                    ace.Mask,
                );
            }
            assert_eq!(rights.get("S-1-5-18"), Some(&FILE_ALL_ACCESS.0));
            assert_eq!(rights.get("S-1-5-32-544"), Some(&FILE_ALL_ACCESS.0));
            assert_eq!(rights.get("S-1-5-11"), Some(&FILE_GENERIC_READ.0));
        });
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}
