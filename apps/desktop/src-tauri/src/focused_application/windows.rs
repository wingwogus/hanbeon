use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

use super::{FocusedApplication, FocusedApplicationSource};

const PROCESS_IMAGE_PATH_CAPACITY: usize = 32_768;

pub struct WindowsSource;

impl FocusedApplicationSource for WindowsSource {
    fn current(&mut self) -> Option<FocusedApplication> {
        let window = unsafe { GetForegroundWindow() };
        if window.is_null() {
            return None;
        }

        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if pid == 0 {
            return None;
        }

        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        let process = ProcessHandle::new(handle)?;

        let mut path = [0_u16; PROCESS_IMAGE_PATH_CAPACITY];
        let mut path_length = path.len() as u32;
        if unsafe { QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut path_length) }
            == 0
        {
            return None;
        }

        let path = String::from_utf16(&path[..path_length as usize]).ok()?;
        let executable = executable_basename(&path)?;
        Some(FocusedApplication::windows(focused_pid(pid)?, executable))
    }
}

struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn new(handle: HANDLE) -> Option<Self> {
        (!handle.is_null()).then_some(Self(handle))
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn executable_basename(path: &str) -> Option<String> {
    path.rsplit(['\\', '/'])
        .next()
        .filter(|basename| !basename.is_empty())
        .map(ToOwned::to_owned)
}

fn focused_pid(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok()
}

#[cfg(test)]
mod tests {
    use super::{executable_basename, focused_pid};

    #[test]
    fn rejects_a_process_identifier_that_does_not_fit_in_i32() {
        assert_eq!(focused_pid(u32::MAX), None);
    }

    #[test]
    fn keeps_only_windows_executable_basename() {
        assert_eq!(
            executable_basename(r"C:\Program Files\Adobe\Acrobat.exe"),
            Some("Acrobat.exe".to_string())
        );
    }

    #[test]
    fn rejects_a_path_without_a_file_name() {
        assert_eq!(executable_basename(r"C:\Program Files\Adobe\"), None);
    }

    #[test]
    fn accepts_forward_slashes_as_path_separators() {
        assert_eq!(
            executable_basename("C:/Program Files/Adobe/Acrobat.exe"),
            Some("Acrobat.exe".to_string())
        );
    }
}
