use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum Game {
    #[serde(rename = "wow")]
    Retail,
    #[serde(rename = "wow-classic")]
    Classic,
    #[serde(rename = "wow-classic-tbc")]
    ClassicProgression,
    #[serde(rename = "wow-classic-tbc-anniv")]
    Anniversary,
    #[serde(rename = "wow-forever")]
    Forever,
}

impl Game {
    pub const ALL: [Self; 5] = [
        Self::Retail,
        Self::Classic,
        Self::ClassicProgression,
        Self::Anniversary,
        Self::Forever,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Retail => "wow",
            Self::Classic => "wow-classic",
            Self::ClassicProgression => "wow-classic-tbc",
            Self::Anniversary => "wow-classic-tbc-anniv",
            Self::Forever => "wow-forever",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Retail => "World of Warcraft",
            Self::Classic => "Classic Era",
            Self::ClassicProgression => "Mists of Pandaria Classic",
            Self::Anniversary => "TBC Anniversary",
            Self::Forever => "WoW Forever",
        }
    }
    pub fn folder(self) -> &'static str {
        match self {
            Self::Retail => "_retail_",
            Self::Classic => "_classic_era_",
            Self::ClassicProgression => "_classic_",
            Self::Anniversary => "_anniversary_",
            Self::Forever => "_forever_",
        }
    }
    pub fn package(self) -> &'static str {
        match self {
            Self::Retail => "ZygorGuidesViewer",
            Self::Classic => "ZygorGuidesViewerClassic",
            Self::ClassicProgression => "ZygorGuidesViewerClassicTBC",
            Self::Anniversary => "ZygorGuidesViewerClassicTBCAnniv",
            Self::Forever => "ZygorGuidesViewerForever",
        }
    }
    pub fn exe(self) -> &'static str {
        match self {
            Self::Retail | Self::Forever => "Wow.exe",
            _ => "WowClassic.exe",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    #[default]
    Us,
    Eu,
    Kr,
    Tw,
    Cn,
}
impl Region {
    pub fn id(self) -> &'static str {
        match self {
            Self::Us => "us",
            Self::Eu => "eu",
            Self::Kr => "kr",
            Self::Tw => "tw",
            Self::Cn => "cn",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Access {
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub plan_name: Option<String>,
    #[serde(default)]
    pub elite_expiry_hr: Option<String>,
    #[serde(default)]
    pub rebill_date_hr: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Account {
    pub username: String,
    pub display_name: String,
    pub access: Access,
}

// Deliberately neither Debug nor Serialize: secrets must not reach logs or IPC.
pub struct Session {
    pub token: zeroize::Zeroizing<String>,
    pub account: Account,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PackageStatus {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub minimessage: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Package {
    // Keep unknown future editions in the catalogue, without enabling installation.
    pub game: String,
    pub name: String,
    #[serde(default)]
    pub ver: Option<String>,
    #[serde(default)]
    pub status: PackageStatus,
}

#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub installation_id: String,
    pub phase: String,
    pub current: u64,
    pub total: Option<u64>,
}
