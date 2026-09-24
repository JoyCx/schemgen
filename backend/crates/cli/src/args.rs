//! Command-line parsing and help.
//!
//! `schemgen2 convert model.glb --out-dir ~/.minecraft/schematics` produces the
//! same file as dropping that model into the web UI with the same settings:
//! both build a [`Settings`] from the same defaults and ranges and hand it to
//! the same pipeline.

use std::collections::{HashMap, HashSet};

use schemgen_core::settings::block_id;
use schemgen_core::Settings;

pub const HELP: &str = "\
SchemGen2 — GLB/glTF → Litematica converter

USAGE:
    schemgen2 <COMMAND> [OPTIONS]

COMMANDS:
    serve                     Start the HTTP API and web UI (default with no command)
    convert <FILE>...         Convert models to schematics without a server
    palette                   Print the block palette (--target for another version)
    targets                   Print the Minecraft versions schematics can target
    schema                    Print the settings schema as JSON
    build-table <DIR> [OUT]   Rebuild the color table from a texture pack
    help, version

Run `schemgen2 help <COMMAND>` for the options of one command.
";

pub const HELP_SERVE: &str = "\
schemgen2 serve — HTTP API + web UI on one port

USAGE:
    schemgen2 serve [OPTIONS]

NETWORK:
    --port <N>            Port to bind (default 3001, or PORT); 0 picks a free one
    --host <ADDR>         Address to bind (default 127.0.0.1). Anything else exposes
                          the server to your network — use --token with it
    --allow-host <NAMES>  Extra host names requests may use, comma-separated
    --token <TOKEN>       Require `Authorization: Bearer <TOKEN>` on every /api call
                          but /api/health (also SCHEMGEN_TOKEN)
    --token-file <PATH>   Read the token from PATH, or create one there if missing

FILES:
    --work-dir <DIR>      Uploads and outputs (default: the user cache folder)
    --ui-dir <DIR>        Built web UI to serve (default: frontend/dist if found;
                          also SCHEMGEN_UI_DIR)
    --job-ttl <HOURS>     Forget finished jobs and delete their files after this
                          long (default 24; 0 keeps them until restart)

CONVERSION:
    --target <VERSION>    Default Minecraft version (default 1.21.8)
    --max-jobs <N>        Conversions running at once (default: CPU count)
    --python <PATH>       Python interpreter with trimesh installed (default python3,
                          python on Windows; also SCHEMGEN_PYTHON)
    --palette <FILE>      Color table to use instead of the built-in one

PROCESS:
    --exit-with-stdin     Stop when standard input closes (for launchers)
    --pid-file <PATH>     Write the process id to PATH while running

Once it accepts connections the server prints one line to stdout:
`listening http://127.0.0.1:<port>`. See docs/api.md for the routes.
";

pub const HELP_CONVERT: &str = "\
schemgen2 convert — convert models to schematics without a server

USAGE:
    schemgen2 convert <FILE>... [OPTIONS]

OUTPUT:
    -o, --output <FILE>       Write to this exact path (single input only)
    -d, --out-dir <DIR>       Write into this folder; ~ and %APPDATA% expand and
                              the folder is created if missing
    -n, --name <NAME>         Schematic name (default: the input file stem)
    -f, --format <FORMAT>     litematic (default), schem (Sponge v2, WorldEdit/FAWE),
                              schem-v3 (WorldEdit 7.3+) or nbt (structure blocks)
    With neither -o nor -d, the file lands next to its input.

TARGET:
    -t, --target <VERSION>    Minecraft version to write for (default 1.21.8). Sets
                              the stamped data version and limits the palette to
                              blocks that exist there. `schemgen2 targets` lists them.
    --data-version <N>        Stamp this exact MinecraftDataVersion instead (also
                              SCHEMGEN_DATA_VERSION)

GEOMETRY:
    --max-size <N>            Longest axis in blocks (default 128, at most 2048)
    --voxel-size <F>          Explicit voxel pitch in model units; overrides --max-size
    --ram-limit <GB>          Color-sampler memory budget (default 4)

COLOR:
    --no-dither               Skip 8x8 Bayer ordered dithering
    --no-color                One block everywhere instead of matching colors
    --block <ID>              Block for --no-color: white, netherrack, or a full ID
    --brightness <F>          -1..1  (default 0)
    --contrast <F>            0..3   (default 1)
    --saturation <F>          0..3   (default 1)

LIGHTING SEPARATION (see docs/pipeline.md):
    --light-dir <x,y,z>       Key light direction in model space (default 0.35,0.85,0.4)
    --light-ambient <F>       0..1   (default 0.32)
    --light-gloss <F>         0..1   (default 0.5)
    --specular <F>            0..4   (default 1.1)
    --highlight-rejection <F> 0..1   (default 0.75)
    --highlight-recovery <F>  0..1   (default 1)
    --delight <F>             0..1   (default 0 = off)

RUN:
    --threads <N>             Convert N files at once (default 1)
    --python <PATH>           Python interpreter with trimesh installed (default
                              python3, python on Windows; also SCHEMGEN_PYTHON)
    --palette <FILE>          Color table to use instead of the built-in one
    -q, --quiet               No progress lines
    -j, --json                Print a JSON result object on stdout

EXIT CODES:
    0 success   1 conversion failed   2 bad usage
";

/// Long options that consume the next argument.
const VALUE_OPTS: &[&str] = &[
    "output",
    "out-dir",
    "name",
    "target",
    "data-version",
    "format",
    "max-size",
    "voxel-size",
    "ram-limit",
    "threads",
    "block",
    "brightness",
    "contrast",
    "saturation",
    "light-dir",
    "light-ambient",
    "light-gloss",
    "specular",
    "highlight-rejection",
    "highlight-recovery",
    "delight",
    "port",
    "host",
    "allow-host",
    "token",
    "token-file",
    "work-dir",
    "ui-dir",
    "job-ttl",
    "max-jobs",
    "pid-file",
    "python",
    "palette",
];

/// One-letter aliases. The bool marks the ones that take a value.
const SHORT_OPTS: &[(char, &str, bool)] = &[
    ('o', "output", true),
    ('d', "out-dir", true),
    ('n', "name", true),
    ('t', "target", true),
    ('f', "format", true),
    ('p', "port", true),
    ('q', "quiet", false),
    ('j', "json", false),
    ('h', "help", false),
    ('V', "version", false),
];

fn is_known_flag(name: &str) -> bool {
    matches!(
        name,
        "no-dither" | "no-color" | "quiet" | "json" | "help" | "version" | "exit-with-stdin"
    )
}

#[derive(Debug, Default)]
pub struct ParsedArgs {
    pub command: String,
    pub positional: Vec<String>,
    pub opts: HashMap<String, String>,
    pub flags: HashSet<String>,
}

impl ParsedArgs {
    pub fn has(&self, flag: &str) -> bool {
        self.flags.contains(flag)
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.opts.get(key).map(String::as_str)
    }

    pub fn number<T: std::str::FromStr>(&self, key: &str, default: T) -> Result<T, String> {
        match self.get(key) {
            None => Ok(default),
            Some(raw) => raw
                .trim()
                .parse::<T>()
                .map_err(|_| format!("--{key}: '{raw}' is not a number")),
        }
    }
}

/// Split argv (without the program name) into a command, options and operands.
///
/// `--key value`, `--key=value`, `-o value`, clustered short flags (`-qj`) and
/// a bare `--` terminator all work. An unknown long option is a usage error
/// rather than a silently ignored typo.
pub fn parse(argv: &[String]) -> Result<ParsedArgs, String> {
    let mut out = ParsedArgs::default();
    let mut rest_are_positional = false;
    let mut i = 0;

    while i < argv.len() {
        let arg = argv[i].clone();
        i += 1;

        if rest_are_positional {
            out.positional.push(arg);
            continue;
        }
        if arg == "--" {
            rest_are_positional = true;
            continue;
        }

        if let Some(long) = arg.strip_prefix("--") {
            let (key, inline) = match long.split_once('=') {
                Some((k, v)) => (k.to_string(), Some(v.to_string())),
                None => (long.to_string(), None),
            };
            if VALUE_OPTS.contains(&key.as_str()) {
                let value = match inline {
                    Some(v) => v,
                    None => {
                        let v = argv
                            .get(i)
                            .cloned()
                            .ok_or_else(|| format!("--{key} needs a value"))?;
                        i += 1;
                        v
                    }
                };
                out.opts.insert(key, value);
            } else if inline.is_some() {
                return Err(format!("--{key} does not take a value"));
            } else if is_known_flag(&key) {
                out.flags.insert(key);
            } else {
                return Err(format!("unknown option --{key}"));
            }
            continue;
        }

        if arg.len() > 1 && arg.starts_with('-') {
            let chars: Vec<char> = arg.chars().skip(1).collect();
            for (pos, c) in chars.iter().enumerate() {
                let (_, name, takes_value) = SHORT_OPTS
                    .iter()
                    .find(|(short, _, _)| short == c)
                    .ok_or_else(|| format!("unknown option -{c}"))?;
                if *takes_value {
                    // The rest of the cluster is the value, else the next argv.
                    let tail: String = chars[pos + 1..].iter().collect();
                    let value = if tail.is_empty() {
                        let v = argv
                            .get(i)
                            .cloned()
                            .ok_or_else(|| format!("-{c} needs a value"))?;
                        i += 1;
                        v
                    } else {
                        tail
                    };
                    out.opts.insert((*name).to_string(), value);
                    break;
                }
                out.flags.insert((*name).to_string());
            }
            continue;
        }

        if out.command.is_empty() && out.positional.is_empty() {
            out.command = arg;
        } else {
            out.positional.push(arg);
        }
    }

    Ok(out)
}

/// Settings from the conversion flags, on top of the defaults every front
/// end shares, clamped to the same ranges.
pub fn settings_from_args(args: &ParsedArgs) -> Result<Settings, String> {
    let d = Settings::default();
    let float = |key: &str, default: f32| args.number::<f32>(key, default);

    let target = match (args.get("target"), args.get("data-version")) {
        (Some(_), Some(_)) => {
            return Err("--target and --data-version are alternatives; pass one".into())
        }
        (Some(t), None) => t.to_string(),
        (None, Some(dv)) => {
            let dv: i32 = dv
                .trim()
                .parse()
                .map_err(|_| format!("--data-version: '{dv}' is not a number"))?;
            dv.to_string()
        }
        (None, None) => std::env::var("SCHEMGEN_DATA_VERSION")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(d.target.clone()),
    };

    let settings = Settings {
        max_size: args.number::<u32>("max-size", d.max_size)?,
        voxel_size: match args.get("voxel-size") {
            Some(raw) => Some(
                raw.trim()
                    .parse::<f32>()
                    .map_err(|_| format!("--voxel-size: '{raw}' is not a number"))?,
            ),
            None => None,
        },
        ram_limit: float("ram-limit", d.ram_limit)?,
        dither: !args.has("no-dither"),
        color_sampling: !args.has("no-color"),
        brightness: float("brightness", d.brightness)?,
        contrast: float("contrast", d.contrast)?,
        saturation: float("saturation", d.saturation)?,
        default_block: block_id(args.get("block").unwrap_or("white"))
            .map_err(|e| format!("--block: {e}"))?,
        light_dir: match args.get("light-dir") {
            Some(raw) => parse_direction(raw)?,
            None => d.light_dir,
        },
        light_ambient: float("light-ambient", d.light_ambient)?,
        light_gloss: float("light-gloss", d.light_gloss)?,
        specular: float("specular", d.specular)?,
        highlight_rejection: float("highlight-rejection", d.highlight_rejection)?,
        highlight_recovery: float("highlight-recovery", d.highlight_recovery)?,
        delight: float("delight", d.delight)?,
        target,
        format: args.get("format").unwrap_or(&d.format).to_string(),
        schematic_name: String::new(),
    };
    settings.normalized().map_err(|e| e.to_string())
}

fn parse_direction(raw: &str) -> Result<[f32; 3], String> {
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return Err(format!("--light-dir: expected x,y,z — got '{raw}'"));
    }
    let mut out = [0.0f32; 3];
    for (slot, part) in out.iter_mut().zip(parts) {
        *slot = part
            .parse()
            .map_err(|_| format!("--light-dir: '{part}' is not a number"))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    fn settings(items: &[&str]) -> Result<Settings, String> {
        settings_from_args(&parse(&argv(items)).unwrap())
    }

    #[test]
    fn parses_command_operands_and_values() {
        let a = parse(&argv(&["convert", "a.glb", "b.glb", "--max-size", "64"])).unwrap();
        assert_eq!(a.command, "convert");
        assert_eq!(a.positional, vec!["a.glb".to_string(), "b.glb".to_string()]);
        assert_eq!(a.get("max-size"), Some("64"));
    }

    #[test]
    fn accepts_inline_values_and_short_clusters() {
        let a = parse(&argv(&[
            "convert",
            "m.glb",
            "--max-size=96",
            "-qj",
            "-o",
            "out.litematic",
        ]))
        .unwrap();
        assert_eq!(a.get("max-size"), Some("96"));
        assert!(a.has("quiet") && a.has("json"));
        assert_eq!(a.get("output"), Some("out.litematic"));
    }

    #[test]
    fn rejects_typos_instead_of_ignoring_them() {
        assert!(parse(&argv(&["convert", "--max-sixe", "64"])).is_err());
        assert!(parse(&argv(&["convert", "--max-size"])).is_err());
        assert!(parse(&argv(&["convert", "--no-dither=1"])).is_err());
        assert!(parse(&argv(&["convert", "-Z"])).is_err());
    }

    #[test]
    fn double_dash_stops_option_parsing() {
        let a = parse(&argv(&["convert", "--", "--weird-name.glb"])).unwrap();
        assert_eq!(a.positional, vec!["--weird-name.glb".to_string()]);
    }

    #[test]
    fn defaults_are_the_shared_defaults() {
        assert_eq!(
            settings(&["convert", "m.glb"]).unwrap(),
            Settings::default()
        );
    }

    #[test]
    fn flags_override_and_clamp() {
        let o = settings(&[
            "convert",
            "m.glb",
            "--no-dither",
            "--no-color",
            "--block",
            "netherrack",
            "--saturation",
            "9",
            "--delight",
            "0.5",
            "--light-dir",
            "0,1,0",
        ])
        .unwrap();
        assert!(!o.dither && !o.color_sampling);
        assert_eq!(o.default_block, "minecraft:netherrack");
        assert_eq!(o.saturation, 3.0);
        assert_eq!(o.delight, 0.5);
        assert_eq!(o.light_dir, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn targets_come_from_either_flag() {
        assert_eq!(
            settings(&["convert", "m.glb", "-t", "1.20.4"])
                .unwrap()
                .target,
            "1.20.4"
        );
        assert_eq!(
            settings(&["convert", "m.glb", "--data-version", "3700"])
                .unwrap()
                .target,
            "1.20.4"
        );
        assert_eq!(
            settings(&["convert", "m.glb", "--data-version", "4435"])
                .unwrap()
                .target,
            "4435"
        );
        assert!(settings(&["convert", "m.glb", "--target", "1.8.9"]).is_err());
        assert!(settings(&["convert", "m.glb", "-t", "1.20.4", "--data-version", "3700"]).is_err());
    }

    #[test]
    fn formats_are_checked() {
        assert_eq!(
            settings(&["convert", "m.glb", "-f", "schem"])
                .unwrap()
                .format,
            "schem"
        );
        assert_eq!(
            settings(&["convert", "m.glb", "--format", "NBT"])
                .unwrap()
                .format,
            "nbt"
        );
        assert!(settings(&["convert", "m.glb", "--format", "mcedit"]).is_err());
    }

    #[test]
    fn bad_light_dir_is_a_usage_error() {
        assert!(settings(&["convert", "m.glb", "--light-dir", "1,2"]).is_err());
        assert!(settings(&["convert", "m.glb", "--light-dir", "0,0,0"]).is_err());
    }

    #[test]
    fn non_numeric_values_are_usage_errors() {
        assert!(settings(&["convert", "m.glb", "--max-size", "big"]).is_err());
        assert!(settings(&["convert", "m.glb", "--contrast", "NaN"]).is_err());
    }
}
