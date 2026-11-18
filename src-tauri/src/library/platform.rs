// The platform a game is played as: the one the player chose in the game's menu, else one read from where it is
// (a folder named for a console, D:\Games\Switch\...) or what starts it (an emulator's exe). Neither says anything
// for a PC game or a recompilation; About then shows the platforms IGDB lists for it.

use super::{Game, Launch};
use std::path::Path;

/// Emulators by their exe's name, and the platform they run.
const EMULATORS: [(&str, &str); 22] = [
    ("ryujinx", "Nintendo Switch"),
    ("yuzu", "Nintendo Switch"),
    ("suyu", "Nintendo Switch"),
    ("sudachi", "Nintendo Switch"),
    ("citron", "Nintendo Switch"),
    ("eden", "Nintendo Switch"),
    ("rpcs3", "PlayStation 3"),
    ("pcsx2", "PlayStation 2"),
    ("pcsx2-qt", "PlayStation 2"),
    ("duckstation", "PlayStation"),
    ("epsxe", "PlayStation"),
    ("shadps4", "PlayStation 4"),
    ("xenia", "Xbox 360"),
    ("xenia_canary", "Xbox 360"),
    ("xemu", "Xbox"),
    ("dolphin", "GameCube / Wii"),
    ("cemu", "Wii U"),
    ("ppsspp", "PSP"),
    ("ppssppwindows64", "PSP"),
    ("vita3k", "PlayStation Vita"),
    ("citra", "Nintendo 3DS"),
    ("azahar", "Nintendo 3DS"),
];

/// Folders named for a platform, as people sort their games.
const FOLDERS: [(&str, &str); 30] = [
    ("switch", "Nintendo Switch"),
    ("switch 2", "Nintendo Switch 2"),
    ("switch2", "Nintendo Switch 2"),
    ("ps1", "PlayStation"),
    ("psx", "PlayStation"),
    ("ps2", "PlayStation 2"),
    ("ps3", "PlayStation 3"),
    ("ps4", "PlayStation 4"),
    ("ps5", "PlayStation 5"),
    ("psp", "PSP"),
    ("vita", "PlayStation Vita"),
    ("xbox", "Xbox"),
    ("xbox360", "Xbox 360"),
    ("xbox 360", "Xbox 360"),
    ("gamecube", "GameCube"),
    ("gc", "GameCube"),
    ("wii", "Wii"),
    ("wiiu", "Wii U"),
    ("wii u", "Wii U"),
    ("n64", "Nintendo 64"),
    ("snes", "Super Nintendo"),
    ("nes", "NES"),
    ("gba", "Game Boy Advance"),
    ("gb", "Game Boy"),
    ("nds", "Nintendo DS"),
    ("3ds", "Nintendo 3DS"),
    ("dreamcast", "Dreamcast"),
    ("saturn", "Sega Saturn"),
    ("megadrive", "Mega Drive"),
    ("genesis", "Mega Drive"),
];

/// The platform `g` is played as, as far as its exe and folders tell.
pub fn detect(g: &Game) -> Option<String> {
    let exe = match &g.launch {
        Launch::Exe(p) | Launch::ExeArgs(p, _) => Some(p.as_path()),
        Launch::Uri(_) => None,
    };
    if let Some(stem) = exe.and_then(Path::file_stem).map(|s| s.to_string_lossy().to_lowercase())
        && let Some((_, p)) = EMULATORS.iter().find(|(e, _)| *e == stem)
    {
        return Some((*p).to_string());
    }
    // The folders above the game's own, nearest first: D:\Games\Switch\Zelda → Switch.
    g.dir.ancestors().skip(1).filter_map(Path::file_name).find_map(|name| {
        let name = name.to_string_lossy().to_lowercase();
        FOLDERS.iter().find(|(f, _)| *f == name).map(|(_, p)| (*p).to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn game(dir: &str, launch: Launch) -> Game {
        Game::new("x".into(), "X".into(), None, PathBuf::from(dir), launch)
    }

    #[test]
    fn a_console_folder_names_the_platform() {
        let g = game(r"D:\Games\Switch\Zelda", Launch::Exe(r"D:\Games\Switch\Zelda\run.exe".into()));
        assert_eq!(detect(&g).as_deref(), Some("Nintendo Switch"));
        let g = game(r"D:\games\PS2\Shadow", Launch::Exe(r"D:\games\PS2\Shadow\play.bat".into()));
        assert_eq!(detect(&g).as_deref(), Some("PlayStation 2"));
    }

    #[test]
    fn an_emulator_names_its_platform() {
        let g = game(r"C:\Emu\RPCS3", Launch::ExeArgs(r"C:\Emu\RPCS3\rpcs3.exe".into(), "--no-gui x".into()));
        assert_eq!(detect(&g).as_deref(), Some("PlayStation 3"));
    }

    #[test]
    fn a_pc_game_or_a_recompilation_has_none() {
        let g = game(r"D:\games\PC\DriveClub", Launch::Exe(r"D:\games\PC\DriveClub\game.exe".into()));
        assert_eq!(detect(&g), None);
        let g = game(r"C:\games\recomps\totk", Launch::Exe(r"C:\games\recomps\totk\totk.exe".into()));
        assert_eq!(detect(&g), None);
    }
}
