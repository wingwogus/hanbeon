//! 플랫폼별 활성 앱 조회를 스캔 루프에서 분리한다.

#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplicationIdentity {
    MacOs {
        bundle_id: Option<String>,
    },
    Windows {
        executable: String,
    },
    Linux {
        desktop_id: Option<String>,
        wm_classes: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusedApplication {
    pid: Option<i32>,
    identity: ApplicationIdentity,
}

impl FocusedApplication {
    pub fn macos(pid: i32, bundle_id: Option<String>) -> Self {
        Self {
            pid: Some(pid),
            identity: ApplicationIdentity::MacOs { bundle_id },
        }
    }

    pub fn windows(pid: i32, executable: String) -> Self {
        Self {
            pid: Some(pid),
            identity: ApplicationIdentity::Windows { executable },
        }
    }

    pub fn linux(pid: Option<i32>, desktop_id: Option<String>, wm_classes: Vec<String>) -> Self {
        Self {
            pid,
            identity: ApplicationIdentity::Linux {
                desktop_id,
                wm_classes,
            },
        }
    }

    pub fn pid(&self) -> Option<i32> {
        self.pid
    }

    pub fn identity(&self) -> &ApplicationIdentity {
        &self.identity
    }

    pub fn macos_bundle_id(&self) -> Option<&str> {
        match &self.identity {
            ApplicationIdentity::MacOs { bundle_id } => bundle_id.as_deref(),
            ApplicationIdentity::Windows { .. } | ApplicationIdentity::Linux { .. } => None,
        }
    }
}

pub trait FocusedApplicationSource {
    fn current(&mut self) -> Option<FocusedApplication>;
}

#[cfg(target_os = "macos")]
pub fn system_source() -> Box<dyn FocusedApplicationSource> {
    Box::new(macos::MacOsSource)
}

#[cfg(target_os = "windows")]
pub fn system_source() -> Box<dyn FocusedApplicationSource> {
    Box::new(windows::WindowsSource)
}

#[cfg(target_os = "linux")]
pub fn system_source() -> Box<dyn FocusedApplicationSource> {
    Box::new(linux::LinuxSource::new())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn system_source() -> Box<dyn FocusedApplicationSource> {
    Box::new(UnsupportedSource)
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
struct UnsupportedSource;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
impl FocusedApplicationSource for UnsupportedSource {
    fn current(&mut self) -> Option<FocusedApplication> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_identity_exposes_bundle_without_platform_branching() {
        let app = FocusedApplication::macos(42, Some("com.apple.Preview".into()));
        assert_eq!(app.pid(), Some(42));
        assert_eq!(app.macos_bundle_id(), Some("com.apple.Preview"));
    }

    #[test]
    fn non_macos_identity_has_no_bundle_id() {
        let app = FocusedApplication::windows(7, "Acrobat.exe".into());
        assert_eq!(app.macos_bundle_id(), None);
    }

    #[test]
    fn linux_identity_retains_a_missing_process_identifier() {
        let app = FocusedApplication::linux(None, None, vec!["Spotify".into()]);
        assert_eq!(app.pid(), None);
        assert_eq!(
            app.identity(),
            &ApplicationIdentity::Linux {
                desktop_id: None,
                wm_classes: vec!["Spotify".into()],
            }
        );
    }
}
