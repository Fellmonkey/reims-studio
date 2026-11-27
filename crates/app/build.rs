//! Embeds the Windows UTF-8 and longPathAware manifest into the launcher.

fn main() {
    #[cfg(windows)]
    {
        let mut resource = winres::WindowsResource::new();
        resource.set_manifest(include_str!("../../scripts/app.manifest"));
        #[expect(
            clippy::expect_used,
            reason = "the Windows build cannot succeed without the embedded manifest"
        )]
        resource
            .compile()
            .expect("embed the Reims Studio application manifest");
    }

    #[cfg(not(windows))]
    {
        // No Windows resources are needed on Linux.
    }
}
