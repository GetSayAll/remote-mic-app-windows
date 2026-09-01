use std::fmt;

pub const MINIMUM_WINDOWS_VERSION: WindowsVersion = WindowsVersion::new(10, 0, 17_763);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsVersion {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

impl WindowsVersion {
    pub const fn new(major: u32, minor: u32, build: u32) -> Self {
        Self {
            major,
            minor,
            build,
        }
    }

    const fn is_at_least(self, minimum: Self) -> bool {
        self.major > minimum.major
            || (self.major == minimum.major
                && (self.minor > minimum.minor
                    || (self.minor == minimum.minor && self.build >= minimum.build)))
    }
}

impl fmt::Display for WindowsVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.build)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedWindowsVersion {
    pub current: WindowsVersion,
    pub minimum: WindowsVersion,
}

impl fmt::Display for UnsupportedWindowsVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "当前 Windows 版本 {} 不受支持；无线麦 SayAll 需要 Windows 10 1809（{}）或更高版本",
            self.current, self.minimum
        )
    }
}

impl std::error::Error for UnsupportedWindowsVersion {}

pub fn check_supported_windows(current: WindowsVersion) -> Result<(), UnsupportedWindowsVersion> {
    if current.is_at_least(MINIMUM_WINDOWS_VERSION) {
        Ok(())
    } else {
        Err(UnsupportedWindowsVersion {
            current,
            minimum: MINIMUM_WINDOWS_VERSION,
        })
    }
}

#[cfg(windows)]
pub fn check_current_windows() -> Result<(), UnsupportedWindowsVersion> {
    let current = windows_version::OsVersion::current();
    check_supported_windows(WindowsVersion::new(
        current.major,
        current.minor,
        current.build,
    ))
}

#[cfg(windows)]
pub fn show_unsupported_windows_message(error: UnsupportedWindowsVersion) {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let message = format!("{error}\r\n\r\nSayAll requires Windows 10 1809 (build 17763) or later.");
    let message: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            w!("无线麦 SayAll"),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_windows_10_1809_and_later() {
        assert_eq!(
            check_supported_windows(WindowsVersion::new(10, 0, 17_763)),
            Ok(())
        );
        assert_eq!(
            check_supported_windows(WindowsVersion::new(10, 0, 22_000)),
            Ok(())
        );
        assert_eq!(
            check_supported_windows(WindowsVersion::new(11, 0, 1)),
            Ok(())
        );
    }

    #[test]
    fn rejects_older_windows_versions() {
        let error = check_supported_windows(WindowsVersion::new(10, 0, 17_762))
            .expect_err("Windows 10 1803 must be rejected");
        assert_eq!(error.current, WindowsVersion::new(10, 0, 17_762));
        assert_eq!(error.minimum, MINIMUM_WINDOWS_VERSION);

        assert!(check_supported_windows(WindowsVersion::new(6, 3, 9_600)).is_err());
    }
}
