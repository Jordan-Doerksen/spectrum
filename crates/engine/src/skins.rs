//! One themed embed system, four channel skins. Each band = a label + emoji + accent
//! color + the shared severity meter. (Finance and politics are separate channels.)

use crate::analyze::Category;

pub struct Skin {
    pub label: &'static str,
    pub emoji: &'static str,
    pub color: u32, // Discord embed accent (0xRRGGBB)
}

pub fn skin(cat: Category) -> Skin {
    match cat {
        Category::Financial   => Skin { label: "FINANCE",      emoji: "💹", color: 0x2E7D46 }, // green
        Category::Political   => Skin { label: "POLITICS",     emoji: "🏛", color: 0xC8922E }, // amber
        Category::Technology  => Skin { label: "TECH",         emoji: "🔬", color: 0x2F7FD8 }, // blue
        Category::Catastrophe => Skin { label: "CRISIS · WAR", emoji: "🔴", color: 0xB23A3A }, // red
        Category::Drop        => Skin { label: "—",            emoji: "·",  color: 0x555555 },
    }
}

/// The shared severity meter, e.g. ●●●○ for a 3.
pub fn meter(severity: u8) -> String {
    (1..=4).map(|i| if i <= severity { '●' } else { '○' }).collect()
}
