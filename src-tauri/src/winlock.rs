//! Erkennt, ob die Windows-Sitzung gesperrt ist (Win+L, Bildschirmschoner mit Anmeldung).

#[cfg(windows)]
pub fn workstation_locked() -> bool {
    use windows_sys::Win32::System::RemoteDesktop::{
        WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW, WTS_CURRENT_SERVER_HANDLE,
        WTS_CURRENT_SESSION,
    };
    // Laut Microsoft-Doku ab Windows 8: 0 = gesperrt, 1 = entsperrt.
    const WTS_SESSIONSTATE_LOCK: i32 = 0;

    let mut buf: windows_sys::core::PWSTR = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: Puffer wird von der API angelegt und unten mit WTSFreeMemory freigegeben;
    // bei WTSSessionInfoEx enthält er eine WTSINFOEXW-Struktur.
    unsafe {
        let ok = WTSQuerySessionInformationW(WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTSSessionInfoEx, &mut buf, &mut len);
        if ok == 0 || buf.is_null() || (len as usize) < std::mem::size_of::<WTSINFOEXW>() {
            return false;
        }
        let info = &*(buf as *const WTSINFOEXW);
        let locked = info.Level == 1 && info.Data.WTSInfoExLevel1.SessionFlags == WTS_SESSIONSTATE_LOCK;
        WTSFreeMemory(buf as *mut core::ffi::c_void);
        locked
    }
}

#[cfg(not(windows))]
pub fn workstation_locked() -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn running_session_is_not_locked() {
        // Tests laufen in einer entsperrten Sitzung.
        assert!(!super::workstation_locked());
    }
}
