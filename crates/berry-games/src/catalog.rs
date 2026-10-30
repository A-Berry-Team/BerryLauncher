#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSource {
    Hoyo {
        game_id: &'static str,
        biz: &'static str,
    },
    Kuro {
        index_url: &'static str,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct GameCatalogEntry {
    pub id: &'static str,
    pub title: &'static str,
    pub folder_name: &'static str,
    pub poster: &'static str,
    pub logo: &'static str,
    pub play_id: &'static str,
    pub source: GameSource,
}

/// A genre or feature tag shown on the game details page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameTag {
    Action,
    Adventure,
    Rpg,
    OpenWorld,
    TurnBased,
    Multiplayer,
    Fantasy,
    SciFi,
    Gacha,
}

impl GameTag {
    /// Fluent message id of the tag's label, translated by the application.
    pub fn label_id(self) -> &'static str {
        match self {
            GameTag::Action => "tag-action",
            GameTag::Adventure => "tag-adventure",
            GameTag::Rpg => "tag-rpg",
            GameTag::OpenWorld => "tag-open-world",
            GameTag::TurnBased => "tag-turn-based",
            GameTag::Multiplayer => "tag-multiplayer",
            GameTag::Fantasy => "tag-fantasy",
            GameTag::SciFi => "tag-sci-fi",
            GameTag::Gacha => "tag-gacha",
        }
    }
}

/// Frame rates offered by the FPS unlocker's preset list (each game shows up to its maximum).
pub const FPS_PRESETS: &[u32] = &[75, 90, 100, 120, 144, 165, 240];
/// Lowest frame rate the "Custom" input accepts.
pub const FPS_CUSTOM_MIN: u32 = 30;

/// What the FPS unlocker (BerryUnlock) can do for one game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FpsUnlockSupport {
    /// Highest preset offered in the list.
    pub max_preset: u32,
    /// Highest value the "Custom" input accepts (the game's own cap when it has one).
    pub max_custom: u32,
    /// Whether the game uses `refresh_delay_ms` / `init_delay_ms` (only Genshin Impact does).
    pub uses_delays: bool,
}

impl FpsUnlockSupport {
    /// The presets this game can use, in ascending order.
    pub fn presets(&self) -> Vec<u32> {
        FPS_PRESETS
            .iter()
            .copied()
            .filter(|fps| *fps <= self.max_preset)
            .collect()
    }

    /// `fps` limited to what the game accepts.
    pub fn clamp(&self, fps: u32) -> u32 {
        fps.clamp(FPS_CUSTOM_MIN, self.max_custom)
    }
}

impl GameCatalogEntry {
    /// FPS unlocker support, `None` when the unlocker cannot do anything for this game (Zenless
    /// Zone Zero has its own "Unlimited" option). Caps follow BerryUnlock's docs/GAME_SUPPORT.md:
    /// Star Rail and Wuthering Waves are capped at 120 by the unlocker; Genshin Impact and
    /// Honkai Impact 3rd apply the requested value, but 500 or more is mapped to 60, so custom
    /// values stop below that.
    pub fn fps_unlock(&self) -> Option<FpsUnlockSupport> {
        match self.id {
            "genshin-impact" => Some(FpsUnlockSupport {
                max_preset: 240,
                max_custom: 480,
                uses_delays: true,
            }),
            "honkai-star-rail" => Some(FpsUnlockSupport {
                max_preset: 120,
                max_custom: 120,
                uses_delays: false,
            }),
            "honkai-impact-3rd" => Some(FpsUnlockSupport {
                max_preset: 240,
                max_custom: 480,
                uses_delays: false,
            }),
            "wuthering-waves" => Some(FpsUnlockSupport {
                max_preset: 120,
                max_custom: 120,
                uses_delays: false,
            }),
            _ => None,
        }
    }

    pub fn localized_title(&self) -> String {
        self.title.to_string()
    }

    /// Studio that develops the game.
    pub fn developer(&self) -> &'static str {
        match self.id {
            "wuthering-waves" => "Kuro Games",
            _ => "miHoYo",
        }
    }

    /// Company that publishes the game.
    pub fn publisher(&self) -> &'static str {
        match self.id {
            "wuthering-waves" => "Kuro Games",
            _ => "HoYoverse",
        }
    }

    /// Genre and feature tags, in display order.
    pub fn tags(&self) -> &'static [GameTag] {
        use GameTag::*;
        match self.id {
            "genshin-impact" => &[
                Action,
                Rpg,
                OpenWorld,
                Adventure,
                Multiplayer,
                Fantasy,
                Gacha,
            ],
            "honkai-star-rail" => &[Rpg, TurnBased, Adventure, SciFi, Gacha],
            "zenless-zone-zero" => &[Action, Rpg, Fantasy, Gacha],
            "honkai-impact-3rd" => &[Action, Rpg, SciFi, Gacha],
            "wuthering-waves" => &[Action, Rpg, OpenWorld, Adventure, Multiplayer, SciFi, Gacha],
            _ => &[],
        }
    }

    /// File names that identify an installed copy of the game.
    pub fn executable_candidates(&self) -> &'static [&'static str] {
        match self.id {
            "genshin-impact" => &["GenshinImpact.exe", "YuanShen.exe"],
            "honkai-star-rail" => &["StarRail.exe"],
            "zenless-zone-zero" => &["ZenlessZoneZero.exe"],
            "honkai-impact-3rd" => &["BH3.exe"],
            "wuthering-waves" => &["Client-Win64-Shipping.exe"],
            _ => &[],
        }
    }

    /// Folders below the install root (besides the root itself) where the executable is normally
    /// found, so a typical install is recognised without searching the whole tree.
    fn executable_subdirs(&self) -> &'static [&'static str] {
        match self.id {
            "wuthering-waves" => &["Client/Binaries/Win64"],
            _ => &[],
        }
    }

    /// The game executable below `root`, if the game is installed there.
    pub fn find_executable(&self, root: &std::path::Path) -> Option<std::path::PathBuf> {
        /// Directories that only hold game data or unfinished downloads: thousands of files, and
        /// never an executable worth finding. Skipping them keeps the fallback search cheap.
        const SKIPPED_DIRS: &[&str] = &[
            "downloading",
            "patching",
            "repairing",
            "content",
            "paks",
            "streamingassets",
            "webcaches",
        ];

        if !root.is_dir() {
            return None;
        }
        let candidates = self.executable_candidates();
        // Cheap exact matches first (the root, then the usual subfolders), then a
        // case-insensitive search below the root.
        std::iter::once(root.to_path_buf())
            .chain(
                self.executable_subdirs()
                    .iter()
                    .map(|subdir| root.join(subdir)),
            )
            .flat_map(|dir| candidates.iter().map(move |name| dir.join(name)))
            .find(|path| path.is_file())
            .or_else(|| {
                berry_core::fs::find_file(
                    root,
                    8,
                    |dir| {
                        dir.file_name()
                            .and_then(|name| name.to_str())
                            .map_or(true, |name| {
                                !SKIPPED_DIRS
                                    .iter()
                                    .any(|skipped| skipped.eq_ignore_ascii_case(name))
                            })
                    },
                    |name| {
                        candidates
                            .iter()
                            .any(|candidate| candidate.eq_ignore_ascii_case(name))
                    },
                )
            })
    }

    pub fn is_installed(&self, root: &std::path::Path) -> bool {
        self.find_executable(root).is_some()
    }

    /// Fluent message id of the store description, translated by the application.
    pub fn description_key(&self) -> Option<&'static str> {
        match self.id {
            "genshin-impact" => Some("catalog-genshin-impact"),
            "honkai-star-rail" => Some("catalog-honkai-star-rail"),
            "zenless-zone-zero" => Some("catalog-zenless-zone-zero"),
            "honkai-impact-3rd" => Some("catalog-honkai-impact-3rd"),
            "wuthering-waves" => Some("catalog-wuthering-waves"),
            _ => None,
        }
    }
}

macro_rules! game {
    (
        $id:literal,
        $title:literal,
        $folder:literal,
        play: $play:literal,
        $source:expr
    ) => {
        GameCatalogEntry {
            id: $id,
            title: $title,
            folder_name: $folder,
            poster: concat!($id, "/poster"),
            logo: concat!($id, "/logo"),
            play_id: $play,
            source: $source,
        }
    };
}

pub const GAMES: &[GameCatalogEntry] = &[
    game!("genshin-impact", "Genshin Impact", "Genshin Impact",
        play: "com.miHoYo.GenshinImpact", GameSource::Hoyo { game_id: "gopR6Cufr3", biz: "hk4e_global" }),
    game!("honkai-star-rail", "Honkai: Star Rail", "Honkai: Star Rail",
        play: "com.HoYoverse.hkrpgoversea", GameSource::Hoyo { game_id: "4ziysqXOQ8", biz: "hkrpg_global" }),
    game!("zenless-zone-zero", "Zenless Zone Zero", "Zenless Zone Zero",
        play: "com.HoYoverse.Nap", GameSource::Hoyo { game_id: "U5hbdsT9W7", biz: "nap_global" }),
    game!("honkai-impact-3rd", "Honkai Impact 3rd", "Honkai Impact 3rd",
        play: "com.miHoYo.bh3global", GameSource::Hoyo { game_id: "5TIVvvcwtM", biz: "bh3_global" }),
    game!("wuthering-waves", "Wuthering Waves", "Wuthering Waves",
    play: "com.kurogame.wutheringwaves.global", GameSource::Kuro {
        index_url: "https://prod-alicdn-gamestarter.kurogame.com/launcher/game/G153/50004_obOHXFrFanqsaIEOmuKroCcbZkQRBC7c/index.json",
    }),
];

pub fn by_id(id: &str) -> Option<&'static GameCatalogEntry> {
    GAMES.iter().find(|game| game.id == id)
}
