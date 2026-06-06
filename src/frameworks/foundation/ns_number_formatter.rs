/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 */
//!
//! `NSNumberFormatter` - formats numbers into strings and parses strings into
//! numbers.

use crate::frameworks::foundation::ns_string::{from_rust_string, to_rust_string};
use crate::frameworks::foundation::NSUInteger;
use crate::objc::{id, msg, nil, objc_classes, ClassExports, HostObject, NSZonePtr};

/// Apple's NSNumberFormatter behavior modes.
/// <https://developer.apple.com/documentation/foundation/nsnumberformatterbehavior>
///
/// * `NSNumberFormatterBehaviorDefault = 0`
/// * `NSNumberFormatterBehavior10_0    = 1000`
/// * `NSNumberFormatterBehavior10_4    = 1040`
const NS_NUMBER_FORMATTER_BEHAVIOR_10_4: NSUInteger = 1040;

/// Process-wide default formatter behavior, used by
/// `+defaultFormatterBehavior` / `+setDefaultFormatterBehavior:`. Apple
/// documents the runtime default as `NSNumberFormatterBehavior10_4` on
/// modern OS versions.
static DEFAULT_FORMATTER_BEHAVIOR: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(NS_NUMBER_FORMATTER_BEHAVIOR_10_4);

#[derive(Default)]
struct NSNumberFormatterHostObject {
    number_style: NSUInteger,
    locale: id,
    grouping_separator: id,
    uses_grouping_separator: bool,
    minimum_fraction_digits: NSUInteger,
    maximum_fraction_digits: NSUInteger,
    /// `NSNumberFormatterBehavior` value. Defaults to
    /// `NSNumberFormatterBehavior10_4` (1040) to match modern iOS.
    formatter_behavior: NSUInteger,
}
impl HostObject for NSNumberFormatterHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSNumberFormatter : NSObject

// =========================================================================
// MARK: - Class methods
// =========================================================================

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = NSNumberFormatterHostObject {
        number_style: 0,
        locale: nil,
        grouping_separator: nil,
        uses_grouping_separator: false,
        minimum_fraction_digits: 0,
        maximum_fraction_digits: 0,
        formatter_behavior: NS_NUMBER_FORMATTER_BEHAVIOR_10_4,
    };
    env.objc.alloc_object(this, Box::new(host_object), &mut env.mem)
}

// `+ (NSNumberFormatterBehavior)defaultFormatterBehavior`
// <https://developer.apple.com/documentation/foundation/nsnumberformatter/1409014-defaultformatterbehavior>
+ (NSUInteger)defaultFormatterBehavior {
    DEFAULT_FORMATTER_BEHAVIOR.load(std::sync::atomic::Ordering::Relaxed) as NSUInteger
}

// `+ (void)setDefaultFormatterBehavior:(NSNumberFormatterBehavior)behavior`
// <https://developer.apple.com/documentation/foundation/nsnumberformatter/1407959-setdefaultformatterbehavior>
+ (())setDefaultFormatterBehavior:(NSUInteger)behavior {
    DEFAULT_FORMATTER_BEHAVIOR.store(behavior as u32, std::sync::atomic::Ordering::Relaxed);
}

// =========================================================================
// MARK: - Instance methods
// =========================================================================

- (id)init {
    this
}

- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}

// `- (NSNumberFormatterBehavior)formatterBehavior`
// <https://developer.apple.com/documentation/foundation/nsnumberformatter/1411915-formatterbehavior>
- (NSUInteger)formatterBehavior {
    env.objc.borrow::<NSNumberFormatterHostObject>(this).formatter_behavior
}

// `- (void)setFormatterBehavior:(NSNumberFormatterBehavior)behavior`
// <https://developer.apple.com/documentation/foundation/nsnumberformatter/1416550-setformatterbehavior>
//
// Apple defines three valid values: `NSNumberFormatterBehaviorDefault`
// (0), `NSNumberFormatterBehavior10_0` (1000) and
// `NSNumberFormatterBehavior10_4` (1040). The "default" value is
// documented to be mapped onto the current
// `+defaultFormatterBehavior` (i.e. the 10.4 behaviour on modern
// systems), so store the resolved value to keep `-formatterBehavior`
// observable from the guest.
- (())setFormatterBehavior:(NSUInteger)behavior {
    let resolved = if behavior == 0 {
        DEFAULT_FORMATTER_BEHAVIOR.load(std::sync::atomic::Ordering::Relaxed) as NSUInteger
    } else {
        behavior
    };
    env.objc.borrow_mut::<NSNumberFormatterHostObject>(this).formatter_behavior = resolved;
}

- (NSUInteger)numberStyle {
    env.objc.borrow::<NSNumberFormatterHostObject>(this).number_style
}

- (())setNumberStyle:(NSUInteger)style {
    env.objc.borrow_mut::<NSNumberFormatterHostObject>(this).number_style = style;
}

- (id)locale {
    env.objc.borrow::<NSNumberFormatterHostObject>(this).locale
}

- (())setLocale:(id)locale {
    env.objc.borrow_mut::<NSNumberFormatterHostObject>(this).locale = locale;
}

- (id)groupingSeparator {
    env.objc.borrow::<NSNumberFormatterHostObject>(this).grouping_separator
}

- (())setGroupingSeparator:(id)separator {
    env.objc.borrow_mut::<NSNumberFormatterHostObject>(this).grouping_separator = separator;
}

- (bool)usesGroupingSeparator {
    env.objc.borrow::<NSNumberFormatterHostObject>(this).uses_grouping_separator
}

- (())setUsesGroupingSeparator:(bool)uses {
    env.objc.borrow_mut::<NSNumberFormatterHostObject>(this).uses_grouping_separator = uses;
}

- (NSUInteger)minimumFractionDigits {
    env.objc.borrow::<NSNumberFormatterHostObject>(this).minimum_fraction_digits
}

- (())setMinimumFractionDigits:(NSUInteger)digits {
    env.objc.borrow_mut::<NSNumberFormatterHostObject>(this).minimum_fraction_digits = digits;
}

- (NSUInteger)maximumFractionDigits {
    env.objc.borrow::<NSNumberFormatterHostObject>(this).maximum_fraction_digits
}

- (())setMaximumFractionDigits:(NSUInteger)digits {
    env.objc.borrow_mut::<NSNumberFormatterHostObject>(this).maximum_fraction_digits = digits;
}

- (id)stringFromNumber:(id)number {
    if number == nil {
        return nil;
    }

    let val: f64 = msg![env; number doubleValue];
    let host_obj = env.objc.borrow::<NSNumberFormatterHostObject>(this);
    let style = host_obj.number_style;

    let rust_string: String;

    // 0 = NoStyle, 1 = DecimalStyle, 2 = CurrencyStyle, 3 = PercentStyle, 4 =
    // ScientificStyle
    if style == 2 {
        rust_string = format!("${:.2}", val);
    } else if style == 3 {
        rust_string = format!("{}%", val * 100.0);
    } else if style == 4 {
        rust_string = format!("{:e}", val);
    } else {
        rust_string = format!("{}", val);
    }

    from_rust_string(env, rust_string)
}

- (id)numberFromString:(id)string {
    if string == nil {
        return nil;
    }

    let rust_str = to_rust_string(env, string);

    // Clean string from currency and percentage signs
    let clean_str = rust_str.replace(['$', ',', '%'], "");
    let trimmed = clean_str.trim();

    if let Ok(val) = trimmed.parse::<f64>() {
        let ns_number_class = env.objc.get_known_class("NSNumber", &mut env.mem);
        msg![env; ns_number_class numberWithDouble:val]
    } else {
        log!("Warning: NSNumberFormatter failed to parse string '{}'", rust_str);
        nil
    }
}

@end

};
