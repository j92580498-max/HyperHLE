/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSTimeZone`.
use crate::frameworks::foundation::{ns_array, ns_dictionary, ns_string, NSInteger};
use crate::objc::{autorelease, id, nil, release, retain, ClassExports, HostObject, NSZonePtr};
use crate::{msg, objc_classes};

/// Minimal IANA-zone → UTC-offset-seconds + abbreviation + long-form
/// (CLDR-style) name table. DST is not modelled; offsets are standard
/// (winter) offsets. The long-form name is used by `localizedName:locale:`
/// for the long `Standard` / `DaylightSaving` / `Generic` styles.
static ZONE_TABLE: &[(&str, i32, &str, &str)] = &[
    ("UTC", 0, "UTC", "Coordinated Universal Time"),
    ("GMT", 0, "GMT", "Greenwich Mean Time"),
    ("America/New_York", -18000, "EST", "Eastern Time"),
    ("America/Chicago", -21600, "CST", "Central Time"),
    ("America/Denver", -25200, "MST", "Mountain Time"),
    ("America/Phoenix", -25200, "MST", "Mountain Standard Time"),
    ("America/Los_Angeles", -28800, "PST", "Pacific Time"),
    ("America/Anchorage", -32400, "AKST", "Alaska Time"),
    ("Pacific/Honolulu", -36000, "HST", "Hawaii-Aleutian Time"),
    ("Canada/Eastern", -18000, "EST", "Eastern Time"),
    ("Canada/Central", -21600, "CST", "Central Time"),
    ("Canada/Mountain", -25200, "MST", "Mountain Time"),
    ("Canada/Pacific", -28800, "PST", "Pacific Time"),
    ("Europe/London", 0, "GMT", "British Time"),
    ("Europe/Paris", 3600, "CET", "Central European Time"),
    ("Europe/Berlin", 3600, "CET", "Central European Time"),
    ("Europe/Warsaw", 3600, "CET", "Central European Time"),
    ("Europe/Helsinki", 7200, "EET", "Eastern European Time"),
    ("Europe/Moscow", 10800, "MSK", "Moscow Time"),
    ("Asia/Kolkata", 19800, "IST", "India Standard Time"),
    ("Asia/Shanghai", 28800, "CST", "China Standard Time"),
    ("Asia/Tokyo", 32400, "JST", "Japan Standard Time"),
    ("Asia/Seoul", 32400, "KST", "Korean Standard Time"),
    ("Australia/Sydney", 36000, "AEST", "Australian Eastern Time"),
    // Abbreviation aliases (looked up by abbreviation key)
    ("EST", -18000, "EST", "Eastern Standard Time"),
    ("CST", -21600, "CST", "Central Standard Time"),
    ("MST", -25200, "MST", "Mountain Standard Time"),
    ("PST", -28800, "PST", "Pacific Standard Time"),
    ("JST", 32400, "JST", "Japan Standard Time"),
    ("IST", 19800, "IST", "India Standard Time"),
    ("CET", 3600, "CET", "Central European Time"),
    ("EET", 7200, "EET", "Eastern European Time"),
    ("MSK", 10800, "MSK", "Moscow Time"),
];

fn offset_for_name(name: &str) -> Option<i32> {
    ZONE_TABLE.iter().find(|r| r.0 == name).map(|r| r.1)
}

fn abbr_for_name(name: &str) -> Option<&'static str> {
    ZONE_TABLE.iter().find(|r| r.0 == name).map(|r| r.2)
}

fn long_name_for_name(name: &str) -> Option<&'static str> {
    ZONE_TABLE.iter().find(|r| r.0 == name).map(|r| r.3)
}

fn name_for_abbr(abbr: &str) -> Option<&'static str> {
    // Prefer a row whose name equals the abbreviation itself, then any match.
    ZONE_TABLE
        .iter()
        .find(|r| r.2 == abbr && r.0 == abbr)
        .or_else(|| ZONE_TABLE.iter().find(|r| r.2 == abbr))
        .map(|r| r.0)
}

#[derive(Default)]
struct NSTimeZoneHostObject {
    /// NSString* — the IANA zone name (or abbreviation string for offset-only
    //zones)
    time_zone: id,
    /// Seconds east of GMT
    seconds_from_gmt: i32,
}
impl HostObject for NSTimeZoneHostObject {}

pub const CLASSES: ClassExports = objc_classes! {
(env, this, _cmd);

@implementation NSTimeZone: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSTimeZoneHostObject {
        time_zone: nil,
        seconds_from_gmt: 0,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// ── Convenience constructors ────────────────────────────────────────────────

+ (id)timeZoneWithName:(id)tz_name {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithName:tz_name];
    autorelease(env, new)
}

+ (id)timeZoneWithAbbreviation:(id)abbr { // NSString *
    let abbr_str = ns_string::to_rust_string(env, abbr);
    if let Some(canonical) = name_for_abbr(&abbr_str) {
        let name: id = ns_string::get_static_str(env, canonical);
        msg![env; this timeZoneWithName:name]
    } else {
        nil
    }
}

+ (id)timeZoneForSecondsFromGMT:(NSInteger)seconds {
    // Build a synthetic name like "GMT+05:30"
    let (sign, abs) = if seconds >= 0 { ('+', seconds) } else { ('-', -seconds) };
    let h = abs / 3600;
    let m = (abs % 3600) / 60;
    let name_str = format!("GMT{}{:02}:{:02}", sign, h, m);
    let name: id = ns_string::get_static_str(env, Box::leak(name_str.into_boxed_str()));
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithName:name];
    // Override the offset that initWithName: would have computed (it won't
    // find "GMT+05:30" in the table, so set it explicitly).
    env.objc.borrow_mut::<NSTimeZoneHostObject>(new).seconds_from_gmt = seconds as i32;
    autorelease(env, new)
}

+ (id)systemTimeZone {
    // Mirror localTimeZone for now; a host-query could be added later.
    msg![env; this localTimeZone]
}

// resetSystemTimeZone clears the cached system time zone so that the next
// call to systemTimeZone re-reads it from the OS / user preferences.
// Since touchHLE always uses UTC we just log and no-op.
+ (())resetSystemTimeZone {
    log_dbg!("NSTimeZone resetSystemTimeZone — ignored (touchHLE uses UTC)");
}

+ (id)defaultTimeZone {
    msg![env; this localTimeZone]
}

+ (id)localTimeZone {
    // As reported by the Aspen Simulator
    let tz_name: id = ns_string::get_static_str(env, "Canada/Eastern");
    msg![env; this timeZoneWithName:tz_name]
}

// Returns an NSArray<NSString*> of every IANA name in our table (deduplicated,
// abbreviation-alias rows excluded).
+ (id)knownTimeZoneNames {
    let names: Vec<id> = ZONE_TABLE
        .iter()
        // Deduplicate: only include rows where the name is not a bare
        // abbreviation
        .filter(|r| !r.0.chars().all(|c| c.is_ascii_uppercase() || c == '/'))
        // Actually: exclude rows that are pure-abbreviation aliases
        .filter(|r| r.0.contains('/') || r.0 == "UTC" || r.0 == "GMT")
        .map(|r| ns_string::get_static_str(env, r.0))
        .collect();
    let array = ns_array::from_vec(env, names);
    autorelease(env, array)
}

// Returns an NSDictionary<NSString*, NSString*> mapping abbreviation → IANA
// name.
+ (id)abbreviationDictionary {
    // ИЗМЕНЕНО: Используем один вектор пар (ключ, значение)
    let mut keys_and_vals: Vec<(id, id)> = Vec::new();

    // Collect unique abbreviations (first occurrence wins).
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for &(iana, _, abbr, _) in ZONE_TABLE {
        if seen.insert(abbr) {
            keys_and_vals.push((
                ns_string::get_static_str(env, abbr),
                ns_string::get_static_str(env, iana),
            ));
        }
    }

    let dict = ns_dictionary::dict_from_keys_and_objects(env, &keys_and_vals);
    autorelease(env, dict)
}

// ── Initializers ─────────────────────────────────────────────────────────────

- (id)initWithName:(id)tz_name { // NSString *
    assert_ne!(tz_name, nil);
    retain(env, tz_name);
    let name_str = ns_string::to_rust_string(env, tz_name);
    let offset = offset_for_name(&name_str).unwrap_or(0);
    let host = env.objc.borrow_mut::<NSTimeZoneHostObject>(this);
    host.time_zone = tz_name;
    host.seconds_from_gmt = offset;
    this
}

// ── Instance methods
// ──────────────────────────────────────────────────────────

- (())dealloc {
    let tz_name = env.objc.borrow_mut::<NSTimeZoneHostObject>(this).time_zone;
    release(env, tz_name);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)name {
    env.objc.borrow_mut::<NSTimeZoneHostObject>(this).time_zone
}

- (id)abbreviation {
    let name_str = {
        let tz = env.objc.borrow_mut::<NSTimeZoneHostObject>(this).time_zone;
        ns_string::to_rust_string(env, tz)
    };
    let abbr = abbr_for_name(&name_str).unwrap_or("GMT");
    ns_string::get_static_str(env, abbr)
}

// Returns the same abbreviation as -abbreviation, ignoring the date argument
// (DST not modelled).
- (id)abbreviationForDate:(id)date {
    msg![env; this abbreviation]
}

// `- (NSString *)localizedName:(NSTimeZoneNameStyle)style locale:(NSLocale *)locale`
//
// Returns a localized presentation name for this time zone in the given
// `style`. touchHLE does not ship CLDR data, so the `locale` argument is
// accepted for API compatibility (the requested locale is logged for debug
// purposes) and the name is derived from the static [ZONE_TABLE] entry —
// long styles return the human-friendly CLDR-style name (e.g. "Pacific
// Time"), short styles return the IANA abbreviation (e.g. "PST"). When the
// zone is not in the table, we fall back to a synthesised "GMT±HH:MM" name
// matching what real Foundation does for offset-only zones.
- (id)localizedName:(NSInteger)style locale:(id)locale {
    let name_str = {
        let tz = env.objc.borrow::<NSTimeZoneHostObject>(this).time_zone;
        ns_string::to_rust_string(env, tz).into_owned()
    };
    let seconds_from_gmt = env
        .objc
        .borrow::<NSTimeZoneHostObject>(this)
        .seconds_from_gmt;

    if locale != nil {
        log_dbg!(
            "-[NSTimeZone localizedName:{} locale:{:?}] for zone {:?} (touchHLE ignores locale)",
            style,
            locale,
            name_str,
        );
    }

    let short_fallback = |seconds: i32| -> String {
        let (sign, abs) = if seconds >= 0 {
            ('+', seconds)
        } else {
            ('-', -seconds)
        };
        let h = abs / 3600;
        let m = (abs % 3600) / 60;
        format!("GMT{}{:02}:{:02}", sign, h, m)
    };

    // NSTimeZoneNameStyle values:
    //   0 = Standard, 1 = ShortStandard, 2 = DaylightSaving,
    //   3 = ShortDaylightSaving, 4 = Generic, 5 = ShortGeneric
    let is_short = matches!(style, 1 | 3 | 5);
    let is_dst = matches!(style, 2 | 3);

    let name = if is_short {
        match abbr_for_name(&name_str) {
            Some(abbr) => abbr.to_string(),
            None => short_fallback(seconds_from_gmt),
        }
    } else {
        let base = long_name_for_name(&name_str)
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                // No CLDR entry: prefer the IANA name, otherwise a synthetic
                // offset name.
                if name_str.is_empty() {
                    short_fallback(seconds_from_gmt)
                } else {
                    name_str.clone()
                }
            });
        if is_dst && !base.to_ascii_lowercase().contains("daylight") {
            // For DST styles, swap "Standard" → "Daylight" if applicable,
            // otherwise append the suffix. This is intentionally approximate
            // since DST is not modelled.
            if base.contains("Standard Time") {
                base.replace("Standard Time", "Daylight Time")
            } else if base.ends_with(" Time") {
                base.replace(" Time", " Daylight Time")
            } else {
                format!("{} Daylight Time", base)
            }
        } else {
            base
        }
    };

    let ns = ns_string::from_rust_string(env, name);
    autorelease(env, ns)
}

- (NSInteger)secondsFromGMT {
    env.objc.borrow_mut::<NSTimeZoneHostObject>(this).seconds_from_gmt as NSInteger
}

// Returns the same offset as -secondsFromGMT, ignoring the date argument
// (DST not modelled).
- (NSInteger)secondsFromGMTForDate:(id)date {
    msg![env; this secondsFromGMT]
}

// DST is not modelled; always returns false.
- (bool)isDaylightSavingTime {
    false
}

// DST is not modelled; always returns false for any date.
- (bool)isDaylightSavingTimeForDate:(id)date {
    false
}

// DST is not modelled; returns nil (no known transition).
- (id)nextDaylightSavingTimeTransition {
    nil
}

// DST is not modelled; returns nil for any date.
- (id)nextDaylightSavingTimeTransitionAfterDate:(id)date {
    nil
}

// DST offset is always 0 since DST is not modelled.
- (NSInteger)daylightSavingTimeOffset {
    0
}

@end

};
