// The region the player picked in Windows' settings.

use windows::Win32::Globalization::{GetUserDefaultGeoName, GetUserDefaultLocaleName};

/// Two letters for a country ("DE", ISO 3166-1), three digits for a wider region ("419", UN M.49), or empty when
/// Windows doesn't say.
pub fn code() -> String {
    let mut buf = [0u16; 16];
    // SAFETY: the call gets the buffer with its length and writes at most that. It returns the characters written
    // with the closing null, or 0 when it fails.
    let len = unsafe { GetUserDefaultGeoName(&mut buf) };
    String::from_utf16_lossy(&buf[..(len.max(1) - 1) as usize])
}

/// The player's locale ("es-CL"), or empty when Windows doesn't say: the language About's descriptions come in
/// unless Settings picks another.
pub fn locale() -> String {
    let mut buf = [0u16; 85];
    // SAFETY: the call gets the buffer with its length (LOCALE_NAME_MAX_LENGTH) and writes at most that. It returns
    // the characters written with the closing null, or 0 when it fails.
    let len = unsafe { GetUserDefaultLocaleName(&mut buf) };
    String::from_utf16_lossy(&buf[..(len.max(1) - 1) as usize])
}
