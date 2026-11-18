// The text on the Windows clipboard, for a key the player copied from a web page (Settings > System > SteamGridDB).

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

/// CF_UNICODETEXT: UTF-16 text ending in a NUL.
const UNICODE_TEXT: u32 = 13;

/// The clipboard's text, None when it holds none or another program has it open.
pub fn text() -> Option<String> {
    // SAFETY: the clipboard is opened for this thread with no owner window and always closed again below; the handle
    // GetClipboardData returns belongs to the clipboard and is only read between GlobalLock and GlobalUnlock, within
    // the size GlobalSize gives, while the clipboard stays open.
    unsafe {
        IsClipboardFormatAvailable(UNICODE_TEXT).ok()?;
        OpenClipboard(None).ok()?;
        let text = (|| {
            let HANDLE(handle) = GetClipboardData(UNICODE_TEXT).ok()?;
            let global = HGLOBAL(handle);
            let ptr = GlobalLock(global).cast::<u16>();
            if ptr.is_null() {
                return None;
            }
            let units = GlobalSize(global) / 2;
            let all = std::slice::from_raw_parts(ptr, units);
            let end = all.iter().position(|&u| u == 0).unwrap_or(units);
            let text = String::from_utf16_lossy(&all[..end]);
            let _ = GlobalUnlock(global);
            Some(text)
        })();
        let _ = CloseClipboard();
        text
    }
}
