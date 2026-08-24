use objc2_app_kit::NSWorkspace;

use super::{FocusedApplication, FocusedApplicationSource};

pub struct MacOsSource;

impl FocusedApplicationSource for MacOsSource {
    fn current(&mut self) -> Option<FocusedApplication> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        Some(FocusedApplication::macos(
            app.processIdentifier(),
            app.bundleIdentifier().map(|id| id.to_string()),
        ))
    }
}
