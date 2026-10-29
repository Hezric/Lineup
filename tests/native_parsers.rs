#![cfg(windows)]

#[test]
fn scheduler_duration_rejects_malformed_and_overflowing_values() {
    for raw in [
        "P",
        "PT",
        "1S",
        "PT-1S",
        "PT1.5S",
        "PT1S2M",
        "P4294967295D",
        "PT4294967295H",
        "PT4294967295M",
        "P49710DT86400S",
    ] {
        assert_eq!(lineup::native::parse_duration(raw), None, "{raw}");
    }
    for (raw, seconds) in [("PT0S", 0), ("PT4294967295S", u32::MAX)] {
        assert_eq!(lineup::native::parse_duration(raw), Some(seconds), "{raw}");
    }
}

#[test]
fn unresolved_paths_and_unquoted_commands_preserve_their_input() {
    assert_eq!(lineup::native::resolve(""), "");
    assert_eq!(
        lineup::native::resolve("\"C:\\LineupTest-missing\\tool.exe\""),
        "C:\\LineupTest-missing\\tool.exe"
    );
    for (raw, path, arguments) in [
        ("", "", ""),
        ("tool.cmd /q", "tool.cmd", "/q"),
        ("\"unterminated /q", "\"unterminated", "/q"),
        ("x.exe.more.exe /q", "x.exe.more.exe", "/q"),
    ] {
        assert_eq!(lineup::split_command(raw), (path.into(), arguments.into()));
    }
}
