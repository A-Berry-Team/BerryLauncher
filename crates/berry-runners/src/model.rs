use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunnerProvider {
    DwProton,
    CachyOs,
    GeProton,
    Wineland,
}

impl RunnerProvider {
    pub const ALL: [Self; 4] = [
        Self::DwProton,
        Self::CachyOs,
        Self::Wineland,
        Self::GeProton,
    ];

    pub fn source_url(self) -> &'static str {
        match self {
            Self::DwProton =>
                "https://dawn.wine/api/v1/repos/dawn-winery/dwproton/releases?limit=100",
            Self::CachyOs =>
                "https://api.github.com/repos/CachyOS/proton-cachyos/releases?per_page=100",
            Self::GeProton =>
                "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases?per_page=100",
            Self::Wineland =>
                "https://api.github.com/repos/nanomatters/proton-cachyos/releases?per_page=100",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::DwProton => "DWProton",
            Self::CachyOs => "CachyOS Proton",
            Self::GeProton => "GE-Proton",
            Self::Wineland => "Proton Wineland",
        }
    }

    pub fn directory_name(self) -> &'static str {
        match self {
            Self::DwProton => "dwproton",
            Self::CachyOs => "cachyos-proton",
            Self::GeProton => "proton-ge",
            Self::Wineland => "proton-wineland",
        }
    }
}

impl fmt::Display for RunnerProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[derive(Debug, Clone)]
pub struct Runner {
    pub provider: RunnerProvider,
    pub name: String,
    pub filename: String,
    pub download_url: String,
    pub installed: bool,
    pub busy: bool,
}

impl Runner {
    pub fn new(
        provider: RunnerProvider,
        name: String,
        filename: String,
        download_url: String,
    ) -> Self {
        Self {
            provider,
            name,
            filename,
            download_url,
            installed: false,
            busy: false,
        }
    }
}
