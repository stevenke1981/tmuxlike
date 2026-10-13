//! Clipboard access belongs to the attaching client, and only follows a user action.

#[cfg(windows)]
mod native {
    use std::io;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{GlobalFree, HWND};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, HWND_MESSAGE,
    };

    const UNICODE_TEXT: u32 = 13; // CF_UNICODETEXT

    struct Window(HWND);
    impl Window {
        fn new() -> io::Result<Self> {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            // A message-only owner works in conhost, Windows Terminal and a test window station.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    null(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    null_mut(),
                    null_mut(),
                    null(),
                )
            };
            if hwnd.is_null() {
                Err(io::Error::last_os_error())
            } else {
                Ok(Self(hwnd))
            }
        }
    }
    impl Drop for Window {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }

    struct Open;
    impl Open {
        fn new(owner: HWND) -> io::Result<Self> {
            let mut error = io::Error::other("clipboard is busy");
            for attempt in 0..5 {
                if unsafe { OpenClipboard(owner) } != 0 {
                    return Ok(Self);
                }
                error = io::Error::last_os_error();
                if attempt < 4 {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            Err(error)
        }
    }
    impl Drop for Open {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }

    pub fn write(text: &str) -> io::Result<()> {
        if text.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "clipboard text contains NUL",
            ));
        }
        let data: Vec<u16> = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\r\n")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let owner = Window::new()?;
        let _open = Open::new(owner.0)?;
        unsafe {
            let mem = GlobalAlloc(GMEM_MOVEABLE, data.len() * 2);
            if mem.is_null() {
                return Err(io::Error::last_os_error());
            }
            let ptr = GlobalLock(mem).cast::<u16>();
            if ptr.is_null() {
                let error = io::Error::last_os_error();
                GlobalFree(mem);
                return Err(error);
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            GlobalUnlock(mem);
            if EmptyClipboard() == 0 || SetClipboardData(UNICODE_TEXT, mem).is_null() {
                let error = io::Error::last_os_error();
                GlobalFree(mem);
                return Err(error);
            }
            // Windows owns the allocation after SetClipboardData succeeds.
        }
        Ok(())
    }

    pub fn read() -> io::Result<String> {
        let _open = Open::new(null_mut())?;
        unsafe {
            let mem = GetClipboardData(UNICODE_TEXT);
            if mem.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "clipboard has no Unicode text",
                ));
            }
            let bytes = GlobalSize(mem);
            if bytes == 0 || bytes > 16 * 1024 * 1024 {
                return Err(io::Error::other("clipboard text too large or unavailable"));
            }
            let ptr = GlobalLock(mem).cast::<u16>();
            if ptr.is_null() {
                return Err(io::Error::last_os_error());
            }
            let data = std::slice::from_raw_parts(ptr, bytes / 2);
            let result = data
                .iter()
                .position(|c| *c == 0)
                .ok_or_else(|| io::Error::other("clipboard text has no terminator"))
                .and_then(|end| {
                    String::from_utf16(&data[..end])
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
                });
            GlobalUnlock(mem);
            result
        }
    }
}

#[cfg(windows)]
pub use native::{read, write};

#[cfg(not(windows))]
pub fn read() -> std::io::Result<String> {
    Err(std::io::Error::other(
        "system paste is available on Windows; use your terminal's paste shortcut",
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn native_clipboard_roundtrip_in_isolated_process() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "clipboard::tests::isolated_worker",
                "--ignored",
                "--nocapture",
            ])
            .env("TMUXLIKE_ISOLATED_CLIPBOARD_WORKER", "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    #[ignore = "only invoked by the parent test in a fresh isolated process"]
    fn isolated_worker() {
        assert_eq!(
            std::env::var("TMUXLIKE_ISOLATED_CLIPBOARD_WORKER").as_deref(),
            Ok("1")
        );
        use std::ptr::{null, null_mut};
        use windows_sys::Win32::System::StationsAndDesktops::*;
        use windows_sys::Win32::System::Threading::GetCurrentThreadId;
        unsafe {
            // A desktop alone shares the user's clipboard. A fresh window station does not.
            let station = CreateWindowStationW(null(), 0, 0x37f, null());
            assert!(
                !station.is_null(),
                "CreateWindowStationW: {}",
                std::io::Error::last_os_error()
            );
            let old_station = GetProcessWindowStation();
            let old_desktop = GetThreadDesktop(GetCurrentThreadId());
            assert_ne!(SetProcessWindowStation(station), 0);
            let name: Vec<u16> = "tmuxlike-test\0".encode_utf16().collect();
            let desktop = CreateDesktopW(name.as_ptr(), null(), null_mut(), 0, 0x1ff, null());
            assert!(
                !desktop.is_null(),
                "CreateDesktopW: {}",
                std::io::Error::last_os_error()
            );
            assert_ne!(SetThreadDesktop(desktop), 0);
            let text = "tmuxlike 繁體中文 日本語 😀 e\u{301}\nsecond line";
            let written = write(text);
            let read_back = if written.is_ok() {
                read()
            } else {
                Err(std::io::Error::other("write failed"))
            };
            let rejected = write("invalid\0text");
            let unchanged = if written.is_ok() {
                read()
            } else {
                Err(std::io::Error::other("write failed"))
            };
            assert_ne!(SetThreadDesktop(old_desktop), 0);
            assert_ne!(SetProcessWindowStation(old_station), 0);
            CloseDesktop(desktop);
            CloseWindowStation(station);
            written.unwrap();
            assert!(rejected.is_err());
            assert_eq!(read_back.unwrap(), text.replace('\n', "\r\n"));
            assert_eq!(unchanged.unwrap(), text.replace('\n', "\r\n"));
        }
    }
}
