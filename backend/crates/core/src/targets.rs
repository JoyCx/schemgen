//! Minecraft versions a schematic can be made for.
//!
//! A target fixes two numbers written into every schematic:
//!
//! * **`MinecraftDataVersion`** — the game version the file claims to come
//!   from. Minecraft (through Litematica) upgrades a schematic stamped *older*
//!   than the running game, and warns about one stamped far newer. It is also
//!   what the palette is filtered by: a target only ever gets blocks that
//!   exist in its version.
//! * **Litematica's schematic `Version`** — 6 up to 1.20.x, 7 from 1.21. Both
//!   read block-only schematics identically (7 only changed how sleeping
//!   entities are stored), so 6 loads everywhere; 7 is written from 1.21 on
//!   because that is what Litematica itself writes there.
//!
//! Data versions come from PrismarineJS `minecraft-data`
//! (`data/pc/common/protocolVersions.json`). Adding a version is one row here
//! plus regenerating `data/block_versions.json` — see `docs/versions.md`.

use serde::Serialize;

use crate::error::{Error, Result};

/// A Minecraft version to write schematics for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Target {
    /// The game version, as players write it: `1.21.8`, `26.1`. `custom` for a
    /// target made from a bare data version.
    pub id: &'static str,
    /// `MinecraftDataVersion` / `DataVersion` stamped into the file.
    pub data_version: i32,
    /// Litematica schematic format version.
    pub schematic_version: i32,
}

/// First data version of Minecraft 1.21, where Litematica moved to
/// schematic version 7.
pub const DATA_VERSION_1_21: i32 = 3953;

/// Oldest supported version: the flattened (1.13+) block names, and the
/// oldest version Litematica and Forgematica are still meaningfully used on.
pub const FLOOR: Target = target("1.16.5", 2586);

/// What a conversion targets unless told otherwise.
pub const DEFAULT_TARGET: &str = "1.21.8";

const fn target(id: &'static str, data_version: i32) -> Target {
    Target {
        id,
        data_version,
        schematic_version: if data_version >= DATA_VERSION_1_21 {
            7
        } else {
            6
        },
    }
}

/// Every named target, oldest first.
pub const TARGETS: &[Target] = &[
    FLOOR,
    target("1.17.1", 2730),
    target("1.18.2", 2975),
    target("1.19.4", 3337),
    target("1.20.1", 3465),
    target("1.20.4", 3700),
    target("1.20.6", 3839),
    target("1.21.1", 3955),
    target("1.21.4", 4189),
    target("1.21.5", 4325),
    target("1.21.8", 4440),
    target("1.21.10", 4556),
    target("1.21.11", 4671),
    target("26.1", 4786),
    target("26.2", 4903),
    target("26.3", 5023),
];

impl Target {
    /// A named target, by its game version.
    pub fn named(id: &str) -> Option<Target> {
        let id = id.trim();
        TARGETS.iter().copied().find(|t| t.id == id)
    }

    /// A target stamped with an arbitrary data version, for game versions
    /// without a row of their own (`--data-version`). A data version that
    /// belongs to a named target is that target.
    pub fn custom(data_version: i32) -> Result<Target> {
        if let Some(t) = TARGETS.iter().find(|t| t.data_version == data_version) {
            return Ok(*t);
        }
        if data_version < FLOOR.data_version {
            return Err(Error::invalid(
                "target",
                format!(
                    "data version {data_version} is older than {} ({}), the oldest supported",
                    FLOOR.id, FLOOR.data_version
                ),
            ));
        }
        Ok(target("custom", data_version))
    }

    /// Parse what a user typed: a game version (`1.20.4`) or a bare data
    /// version (`3700`).
    pub fn parse(raw: &str) -> Result<Target> {
        let raw = raw.trim();
        if let Some(t) = Target::named(raw) {
            return Ok(t);
        }
        if let Ok(dv) = raw.parse::<i32>() {
            return Target::custom(dv);
        }
        Err(Error::invalid(
            "target",
            format!(
                "unknown Minecraft version \"{raw}\" — one of {}, or a data version number",
                TARGETS.iter().map(|t| t.id).collect::<Vec<_>>().join(", ")
            ),
        ))
    }

    /// The target conversions use when none is given.
    pub fn default_target() -> Target {
        Target::named(DEFAULT_TARGET).expect("the default target is in the table")
    }

    /// The canonical string for this target: its id, or the data version for
    /// a custom one.
    pub fn key(&self) -> String {
        if self.id == "custom" {
            self.data_version.to_string()
        } else {
            self.id.to_string()
        }
    }
}

impl Default for Target {
    fn default() -> Self {
        Target::default_target()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_ordered_and_unique() {
        for pair in TARGETS.windows(2) {
            assert!(pair[0].data_version < pair[1].data_version, "{pair:?}");
            assert_ne!(pair[0].id, pair[1].id);
        }
        assert_eq!(TARGETS[0], FLOOR);
    }

    #[test]
    fn schematic_version_switches_at_1_21() {
        assert_eq!(Target::named("1.20.6").unwrap().schematic_version, 6);
        assert_eq!(Target::named("1.21.1").unwrap().schematic_version, 7);
        assert_eq!(Target::named("26.3").unwrap().schematic_version, 7);
    }

    #[test]
    fn default_is_1_21_8() {
        let t = Target::default();
        assert_eq!((t.id, t.data_version), ("1.21.8", 4440));
    }

    #[test]
    fn parses_names_and_data_versions() {
        assert_eq!(Target::parse(" 1.20.4 ").unwrap().data_version, 3700);
        let custom = Target::parse("4435").unwrap();
        assert_eq!((custom.id, custom.data_version), ("custom", 4435));
        assert_eq!(custom.key(), "4435");
        assert_eq!(Target::named("1.21.11").unwrap().key(), "1.21.11");
        assert_eq!(Target::parse("4440").unwrap().id, "1.21.8");
        assert!(Target::parse("1.12.2").is_err());
        assert!(Target::parse("1139").is_err(), "below the floor");
        assert!(Target::parse("banana").is_err());
    }
}
