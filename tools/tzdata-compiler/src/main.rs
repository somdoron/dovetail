//! tzdata-compiler — reads IANA tz source files and writes the packed binary
//! consumed by `standard-time/src/TzData.dove`.
//!
//! Usage:
//!   cargo run -p tzdata-compiler -- <iana-src-dir> <output.bin>
//!
//! `iana-src-dir` contains the IANA source files (`africa`, `europe`, …,
//! `leapseconds`) — vendored under `standard-time/tzdata/iana/`.
//!
//! Output format (little-endian):
//!   header (32 bytes):
//!     magic            : 4   "TZDB"
//!     format_version   : u32 1
//!     zone_count       : u32
//!     name_pool_offset : u32   (absolute offset)
//!     abbrev_pool_off  : u32
//!     zones_offset     : u32
//!     leap_offset      : u32   (0 if no leap section)
//!     // pad to 32
//!
//!   zone_index, sorted by name (16 bytes per entry):
//!     name_offset : u32   (into name_pool)
//!     name_length : u32
//!     data_offset : u32   (into zones_section, absolute)
//!     reserved    : u32   (0)
//!
//!   zones_section, per zone:
//!     transition_count : u32
//!     transitions      : [ (epoch_second:i64, offset_seconds:i32,
//!                            abbrev_idx:u16, flags:u8, _pad:u8) ; count ]
//!         flags bit0 = isDst
//!     posix_string_len : u16
//!     _pad             : u16
//!     posix_string     : utf8[]
//!     (padded to 4 bytes)
//!
//!   name_pool      : packed utf8, indexed by (name_offset, name_length)
//!   abbrev_pool    : NUL-terminated utf8 abbreviations
//!
//!   leap section (when present):
//!     count   : u32
//!     entries : [(epoch_second:i64, correction_seconds:i32, _pad:i32); count]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;

use parse_zoneinfo::line::{Line, LineParser};
use parse_zoneinfo::table::TableBuilder;
use parse_zoneinfo::transitions::TableTransitions;

const MAGIC: &[u8; 4] = b"TZDB";
const FORMAT_VERSION: u32 = 1;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: tzdata-compiler <iana-src-dir> <output.bin>");
        std::process::exit(2);
    }
    let iana_dir = Path::new(&args[1]);
    let output_path = Path::new(&args[2]);

    let packed = match compile(iana_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).expect("create output dir");
    }
    fs::write(output_path, &packed).expect("write output");

    // Quick header stats so updates print a sanity check.
    let zone_count = u32::from_le_bytes(packed[8..12].try_into().unwrap());
    eprintln!(
        "wrote {} bytes ({} zones) to {}",
        packed.len(),
        zone_count,
        output_path.display()
    );
}

fn compile(iana_dir: &Path) -> Result<Vec<u8>, String> {
    // The standard IANA distribution files we need. `leapseconds` is parsed
    // separately because parse-zoneinfo's Line parser doesn't know `Leap`.
    let zone_files = [
        "africa",
        "antarctica",
        "asia",
        "australasia",
        "backward",
        "etcetera",
        "europe",
        "northamerica",
        "southamerica",
    ];

    let mut builder = TableBuilder::new();
    let parser = LineParser::default();
    for fname in zone_files {
        let path = iana_dir.join(fname);
        let content =
            fs::read_to_string(&path).map_err(|e| format!("reading {}: {}", path.display(), e))?;
        for (lineno, raw) in content.lines().enumerate() {
            // parse-zoneinfo wants stripped continuation comments — its
            // parser handles `#` already; just feed each line.
            let line = match parser.parse_str(raw) {
                Ok(l) => l,
                Err(e) => {
                    return Err(format!(
                        "{}:{}: parse error: {}",
                        path.display(),
                        lineno + 1,
                        e
                    ));
                }
            };
            match line {
                Line::Zone(z) => builder.add_zone_line(z).map_err(|e| e.to_string())?,
                Line::Continuation(z) => builder
                    .add_continuation_line(z)
                    .map_err(|e| e.to_string())?,
                Line::Rule(r) => builder.add_rule_line(r).map_err(|e| e.to_string())?,
                Line::Link(l) => builder.add_link_line(l).map_err(|e| e.to_string())?,
                Line::Space => {}
            }
        }
    }
    let table = builder.build();

    // Build the zone list. Resolve every name (including Links) to its
    // canonical zone's transitions.
    let mut zone_names: BTreeSet<String> = BTreeSet::new();
    for name in table.zonesets.keys() {
        zone_names.insert(name.clone());
    }
    for name in table.links.keys() {
        zone_names.insert(name.clone());
    }

    // For each name, compute the transition table.
    // FixedTimespanSet contains: { first: FixedTimespan, rest: Vec<(i64, FixedTimespan)> }
    // FixedTimespan: { utc_offset: i64, dst_offset: i64, name: String }
    struct ZoneRecord {
        name: String,
        first_offset: i32,
        first_dst: bool,
        first_abbrev: String,
        transitions: Vec<(i64, i32, bool, String)>, // (epoch, offset, dst, abbrev)
    }

    let mut zones: Vec<ZoneRecord> = Vec::new();
    for name in &zone_names {
        let canonical = table
            .links
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.clone());
        let timespans = match table.timespans(&canonical) {
            Some(ts) => ts,
            None => continue, // shouldn't happen — name came from zonesets/links
        };
        let first = &timespans.first;
        let mut transitions = Vec::with_capacity(timespans.rest.len());
        for (epoch, span) in &timespans.rest {
            transitions.push((
                *epoch,
                (span.utc_offset + span.dst_offset) as i32,
                span.dst_offset != 0,
                span.name.clone(),
            ));
        }
        zones.push(ZoneRecord {
            name: name.clone(),
            first_offset: (first.utc_offset + first.dst_offset) as i32,
            first_dst: first.dst_offset != 0,
            first_abbrev: first.name.clone(),
            transitions,
        });
    }
    zones.sort_by(|a, b| a.name.cmp(&b.name));

    // Parse leap seconds (separate file).
    let mut leaps: Vec<(i64, i32)> = Vec::new();
    if let Ok(content) = fs::read_to_string(iana_dir.join("leapseconds")) {
        let mut total: i32 = 0;
        for raw in content.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Format: Leap YYYY Mon DD HH:MM:SS [+-] R/S
            if parts.len() >= 7 && parts[0] == "Leap" {
                let year: i32 = parts[1].parse().map_err(|_| "leap year parse")?;
                let month = parse_month(parts[2])?;
                let day: u32 = parts[3].parse().map_err(|_| "leap day parse")?;
                let time: Vec<&str> = parts[4].split(':').collect();
                if time.len() != 3 {
                    return Err(format!("bad leap time: {}", parts[4]));
                }
                let h: u32 = time[0].parse().map_err(|_| "leap hour")?;
                let m: u32 = time[1].parse().map_err(|_| "leap minute")?;
                let s: u32 = time[2].parse().map_err(|_| "leap second")?;
                let sign = parts[5];
                let delta: i32 = match sign {
                    "+" => 1,
                    "-" => -1,
                    _ => return Err(format!("bad leap sign: {sign}")),
                };
                let epoch = days_from_epoch(year, month, day) * 86400
                    + (h as i64) * 3600
                    + (m as i64) * 60
                    + (s as i64);
                total += delta;
                leaps.push((epoch, total));
            }
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // Build pools first (so we can compute offsets).
    // ──────────────────────────────────────────────────────────────────

    // Abbreviation pool: deduplicated, NUL-terminated.
    let mut abbrev_pool: Vec<u8> = Vec::new();
    let mut abbrev_indices: BTreeMap<String, u16> = BTreeMap::new();
    let intern_abbrev = |s: &str, pool: &mut Vec<u8>, indices: &mut BTreeMap<String, u16>| -> u16 {
        if let Some(&idx) = indices.get(s) {
            return idx;
        }
        let idx = u16::try_from(pool.len()).expect("abbrev pool overflow");
        pool.extend_from_slice(s.as_bytes());
        pool.push(0);
        indices.insert(s.to_string(), idx);
        idx
    };
    // Pre-intern first-offset and transition abbreviations.
    for z in &zones {
        let _ = intern_abbrev(&z.first_abbrev, &mut abbrev_pool, &mut abbrev_indices);
        for t in &z.transitions {
            let _ = intern_abbrev(&t.3, &mut abbrev_pool, &mut abbrev_indices);
        }
    }

    // Name pool — raw concatenation; lookup uses (offset, length).
    let mut name_pool: Vec<u8> = Vec::new();
    let mut name_offsets: Vec<(u32, u32)> = Vec::with_capacity(zones.len());
    for z in &zones {
        let off = u32::try_from(name_pool.len()).expect("name pool overflow");
        let len = u32::try_from(z.name.len()).expect("name length overflow");
        name_pool.extend_from_slice(z.name.as_bytes());
        name_offsets.push((off, len));
    }

    // Zones section: per-zone payload (transition table + posix string).
    // For each zone, we now build the payload with its absolute offset in
    // the zones_section, then we'll emit them sequentially.
    let mut zones_section: Vec<u8> = Vec::new();
    let mut zone_data_offsets: Vec<u32> = Vec::with_capacity(zones.len());
    for z in &zones {
        let off = u32::try_from(zones_section.len()).expect("zones section overflow");
        zone_data_offsets.push(off);
        // Treat the first segment as an implicit transition at i64::MIN+1
        // so the runtime can find an offset for *any* epoch_second.
        let count = u32::try_from(z.transitions.len() + 1).expect("transition count");
        zones_section.extend_from_slice(&count.to_le_bytes());
        // Implicit first transition at i64::MIN+1.
        write_transition(
            &mut zones_section,
            i64::MIN + 1,
            z.first_offset,
            z.first_dst,
            *abbrev_indices.get(&z.first_abbrev).unwrap(),
        );
        for t in &z.transitions {
            write_transition(
                &mut zones_section,
                t.0,
                t.1,
                t.2,
                *abbrev_indices.get(&t.3).unwrap(),
            );
        }
        // POSIX string: empty for now (extracted from the source file's
        // trailing rule in a follow-up). Most zones either have a final
        // POSIX rule or fall back to the last fixed offset; emitting an
        // empty string makes the runtime use the last transition's offset
        // for any epoch past it.
        let posix_str = "";
        let posix_len = u16::try_from(posix_str.len()).unwrap();
        zones_section.extend_from_slice(&posix_len.to_le_bytes());
        zones_section.extend_from_slice(&[0u8, 0u8]); // pad
        zones_section.extend_from_slice(posix_str.as_bytes());
        while !zones_section.len().is_multiple_of(4) {
            zones_section.push(0);
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // Now assemble final layout. Offsets are absolute (from start of file).
    // ──────────────────────────────────────────────────────────────────
    const HEADER_SIZE: usize = 32;
    let zone_count = u32::try_from(zones.len()).expect("zone count overflow");
    let index_size = (zones.len() * 16) as u32;
    let zones_off = HEADER_SIZE as u32 + index_size;
    let zones_size = u32::try_from(zones_section.len()).expect("zones size overflow");
    let name_pool_off = zones_off + zones_size;
    let name_size = u32::try_from(name_pool.len()).expect("name pool overflow");
    let abbrev_pool_off = name_pool_off + name_size;
    let abbrev_size = u32::try_from(abbrev_pool.len()).expect("abbrev pool overflow");
    let leap_off = if leaps.is_empty() {
        0
    } else {
        abbrev_pool_off + abbrev_size
    };

    let mut out = Vec::with_capacity(
        HEADER_SIZE
            + index_size as usize
            + zones_section.len()
            + name_pool.len()
            + abbrev_pool.len()
            + 4
            + 16 * leaps.len(),
    );

    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&zone_count.to_le_bytes());
    out.extend_from_slice(&name_pool_off.to_le_bytes());
    out.extend_from_slice(&abbrev_pool_off.to_le_bytes());
    out.extend_from_slice(&zones_off.to_le_bytes());
    out.extend_from_slice(&leap_off.to_le_bytes());
    // Pad header to 32 bytes.
    out.extend_from_slice(&0u32.to_le_bytes());
    debug_assert_eq!(out.len(), HEADER_SIZE);

    for (i, z) in zones.iter().enumerate() {
        let (name_off, name_len) = name_offsets[i];
        let abs_data_off = zones_off + zone_data_offsets[i];
        out.extend_from_slice(&name_off.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&abs_data_off.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        let _ = z;
    }
    out.extend_from_slice(&zones_section);
    out.extend_from_slice(&name_pool);
    out.extend_from_slice(&abbrev_pool);
    if !leaps.is_empty() {
        let leap_count = u32::try_from(leaps.len()).expect("leap count overflow");
        out.extend_from_slice(&leap_count.to_le_bytes());
        for (epoch, total) in &leaps {
            out.extend_from_slice(&epoch.to_le_bytes());
            out.extend_from_slice(&total.to_le_bytes());
            out.extend_from_slice(&0i32.to_le_bytes());
        }
    }

    Ok(out)
}

fn write_transition(buf: &mut Vec<u8>, epoch: i64, offset: i32, dst: bool, abbrev_idx: u16) {
    buf.extend_from_slice(&epoch.to_le_bytes());
    buf.extend_from_slice(&offset.to_le_bytes());
    buf.extend_from_slice(&abbrev_idx.to_le_bytes());
    buf.push(if dst { 1 } else { 0 });
    buf.push(0); // pad
}

fn parse_month(s: &str) -> Result<u32, String> {
    Ok(match s {
        "Jan" | "January" => 1,
        "Feb" | "February" => 2,
        "Mar" | "March" => 3,
        "Apr" | "April" => 4,
        "May" => 5,
        "Jun" | "June" => 6,
        "Jul" | "July" => 7,
        "Aug" | "August" => 8,
        "Sep" | "September" => 9,
        "Oct" | "October" => 10,
        "Nov" | "November" => 11,
        "Dec" | "December" => 12,
        _ => return Err(format!("bad month: {s}")),
    })
}

/// Days from 1970-01-01 (proleptic Gregorian), supports BCE via negative year.
fn days_from_epoch(y: i32, m: u32, d: u32) -> i64 {
    // Howard Hinnant's algorithm — works for any year in Gregorian.
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u32; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era as i64) * 146097 + doe as i64 - 719468
}
