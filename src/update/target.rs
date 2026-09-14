pub const fn triple() -> Option<&'static str> {
    if cfg!(all(target_arch = "aarch64", target_os = "macos")) {
        Some("aarch64-apple-darwin")
    } else if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
        Some("x86_64-apple-darwin")
    } else if cfg!(all(
        target_arch = "x86_64",
        target_os = "linux",
        target_env = "musl"
    )) {
        Some("x86_64-unknown-linux-musl")
    } else if cfg!(all(
        target_arch = "x86_64",
        target_os = "linux",
        target_env = "gnu"
    )) {
        Some("x86_64-unknown-linux-gnu")
    } else if cfg!(all(
        target_arch = "aarch64",
        target_os = "linux",
        target_env = "gnu"
    )) {
        Some("aarch64-unknown-linux-gnu")
    } else if cfg!(all(
        target_arch = "x86_64",
        target_os = "windows",
        target_env = "msvc"
    )) {
        Some("x86_64-pc-windows-msvc")
    } else if cfg!(all(
        target_arch = "aarch64",
        target_os = "windows",
        target_env = "msvc"
    )) {
        Some("aarch64-pc-windows-msvc")
    } else {
        None
    }
}

pub fn asset_name() -> Option<String> {
    let triple = triple()?;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    Some(format!("obsidian-mcp-rs-{triple}{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_platform_the_release_workflow_builds_can_name_its_own_asset() {
        const PUBLISHED: [&str; 7] = [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-musl",
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        ];
        let Some(triple) = triple() else {
            return;
        };
        assert!(
            PUBLISHED.contains(&triple),
            "{triple} is not a target the release workflow publishes"
        );
    }

    #[test]
    fn the_asset_name_matches_what_the_release_workflow_uploads() {
        let Some(name) = asset_name() else {
            return;
        };
        assert!(name.starts_with("obsidian-mcp-rs-"));
        assert_eq!(name.ends_with(".exe"), cfg!(windows));
        assert!(name.contains(triple().unwrap()));
    }
}
