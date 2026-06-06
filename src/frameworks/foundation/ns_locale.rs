/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSLocale`.

use super::{ns_array, ns_string};
use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::core_foundation::cf_locale::{
    kCFLocaleCountryCode, kCFLocaleIdentifier, kCFLocaleLanguageCode,
};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};
use crate::window::{get_preferred_country_codes, get_preferred_language_codes};
use crate::Environment;

// MARK: - NSLocale key constants

const NSLocaleCountryCode: &str = "NSLocaleCountryCode";
const NSLocaleLanguageCode: &str = "NSLocaleLanguageCode";
const NSLocaleScriptCode: &str = "NSLocaleScriptCode";
const NSLocaleVariantCode: &str = "NSLocaleVariantCode";
const NSLocaleIdentifier: &str = "kCFLocaleIdentifierKey";
const NSLocaleCalendar: &str = "NSLocaleCalendar";
const NSLocaleCollationIdentifier: &str = "NSLocaleCollationIdentifier";
const NSLocaleUsesMetricSystem: &str = "NSLocaleUsesMetricSystem";
const NSLocaleMeasurementSystem: &str = "NSLocaleMeasurementSystem";
const NSLocaleDecimalSeparator: &str = "NSLocaleDecimalSeparator";
const NSLocaleGroupingSeparator: &str = "NSLocaleGroupingSeparator";
const NSLocaleCurrencySymbol: &str = "NSLocaleCurrencySymbol";
const NSLocaleCurrencyCode: &str = "NSLocaleCurrencyCode";
const NSLocaleCollatorIdentifier: &str = "NSLocaleCollatorIdentifier";
const NSLocaleQuotationBeginDelimiterKey: &str = "NSLocaleQuotationBeginDelimiterKey";
const NSLocaleQuotationEndDelimiterKey: &str = "NSLocaleQuotationEndDelimiterKey";

pub const CONSTANTS: ConstantExports = &[
    (
        "_NSLocaleCountryCode",
        HostConstant::NSString(NSLocaleCountryCode),
    ),
    (
        "_NSLocaleLanguageCode",
        HostConstant::NSString(NSLocaleLanguageCode),
    ),
    (
        "_NSLocaleScriptCode",
        HostConstant::NSString(NSLocaleScriptCode),
    ),
    (
        "_NSLocaleVariantCode",
        HostConstant::NSString(NSLocaleVariantCode),
    ),
    (
        "_NSLocaleIdentifier",
        HostConstant::NSString(NSLocaleIdentifier),
    ),
    (
        "_NSLocaleCalendar",
        HostConstant::NSString(NSLocaleCalendar),
    ),
    (
        "_NSLocaleCollationIdentifier",
        HostConstant::NSString(NSLocaleCollationIdentifier),
    ),
    (
        "_NSLocaleUsesMetricSystem",
        HostConstant::NSString(NSLocaleUsesMetricSystem),
    ),
    (
        "_NSLocaleMeasurementSystem",
        HostConstant::NSString(NSLocaleMeasurementSystem),
    ),
    (
        "_NSLocaleDecimalSeparator",
        HostConstant::NSString(NSLocaleDecimalSeparator),
    ),
    (
        "_NSLocaleGroupingSeparator",
        HostConstant::NSString(NSLocaleGroupingSeparator),
    ),
    (
        "_NSLocaleCurrencySymbol",
        HostConstant::NSString(NSLocaleCurrencySymbol),
    ),
    (
        "_NSLocaleCurrencyCode",
        HostConstant::NSString(NSLocaleCurrencyCode),
    ),
    (
        "_NSLocaleCollatorIdentifier",
        HostConstant::NSString(NSLocaleCollatorIdentifier),
    ),
    (
        "_NSLocaleQuotationBeginDelimiterKey",
        HostConstant::NSString(NSLocaleQuotationBeginDelimiterKey),
    ),
    (
        "_NSLocaleQuotationEndDelimiterKey",
        HostConstant::NSString(NSLocaleQuotationEndDelimiterKey),
    ),
];

#[derive(Default)]
pub struct State {
    current_locale: Option<id>,
    system_locale: Option<id>,
    preferred_languages: Option<id>,
}
impl State {
    fn get(env: &mut Environment) -> &mut State {
        &mut env.framework_state.foundation.ns_locale
    }
}

// MARK: - Internal helpers

fn get_preferred_languages(env: &mut Environment) -> Vec<String> {
    let options = env.options.as_ref();
    if let Some(ref preferred_languages) = options.preferred_languages {
        log!("The app requested your preferred languages. {:?} will reported based on your --preferred-languages= option.", preferred_languages);
        return preferred_languages.clone();
    }

    let languages = get_preferred_language_codes(env);
    if languages.is_empty() {
        let lang = "en".to_string();
        log!("The app requested your preferred languages. No information could be retrieved, so {:?} (English) will be reported.", lang);
        vec![lang]
    } else {
        log!("The app requested your preferred languages. {:?} will be reported based on your system language preferences.", languages);
        languages
    }
}

fn get_preferred_countries(env: &mut Environment) -> Vec<String> {
    let countries = get_preferred_country_codes(env);
    if countries.is_empty() {
        let country = "US".to_string();
        log!("The app requested your current locale. No country information could be retrieved, so {:?} will be reported.", country);
        vec![country]
    } else {
        log!("The app requested your current locale. {:?} will be reported based on your system region settings.", countries);
        countries
    }
}

fn language_from_locale_identifier(identifier: &str) -> &str {
    let sep = identifier.find('_').or_else(|| identifier.find('-'));
    match sep {
        Some(idx) => &identifier[..idx],
        None => identifier,
    }
}

fn country_from_locale_identifier(identifier: &str) -> Option<&str> {
    let sep = identifier.find('_').or_else(|| identifier.find('-'))?;
    let rest = &identifier[sep + 1..];
    // Strip script code if present (e.g. "zh_Hans_CN" -> "CN")
    if let Some(second) = rest.find('_').or_else(|| rest.find('-')) {
        Some(&rest[second + 1..])
    } else {
        Some(rest)
    }
}

/// Build a locale identifier string like "en_US".
fn locale_identifier(language: &str, country: &str) -> String {
    if country.is_empty() {
        language.to_string()
    } else {
        format!("{}_{}", language, country)
    }
}

// MARK: - Host object

#[derive(Default)]
struct NSLocaleHostObject {
    country_code: id,  // NSString* — retained
    language_code: id, // NSString* — retained
}
impl HostObject for NSLocaleHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSLocale: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSLocaleHostObject {
        country_code:  nil,
        language_code: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// MARK: - Singletons

+ (id)currentLocale {
    if let Some(locale) = State::get(env).current_locale {
        return locale;
    }
    let countries = get_preferred_countries(env);
    let languages  = get_preferred_languages(env);
    let country_code  = ns_string::from_rust_string(env, countries[0].clone());
    let language_code = ns_string::from_rust_string(env, languages[0].clone());
    let new = env.objc.alloc_object(
        this,
        Box::new(NSLocaleHostObject { country_code, language_code }),
        &mut env.mem,
    );
    // Retain so the singleton lives beyond any autorelease pool drain.
    retain(env, new);
    State::get(env).current_locale = Some(new);
    new
}

+ (id)systemLocale {
    if let Some(locale) = State::get(env).system_locale {
        return locale;
    }
    let new = env.objc.alloc_object(
        this,
        Box::new(NSLocaleHostObject { country_code: nil, language_code: nil }),
        &mut env.mem,
    );
    // Retain so the singleton lives beyond any autorelease pool drain.
    retain(env, new);
    State::get(env).system_locale = Some(new);
    new
}

+ (id)autoupdatingCurrentLocale {
    // We don't auto-update; return currentLocale.
    msg_class![env; NSLocale currentLocale]
}

// MARK: - Preferred languages / locales

+ (id)preferredLanguages {
    if let Some(existing) = State::get(env).preferred_languages {
        return existing;
    }
    let langs = get_preferred_languages(env);
    let ns_strings: Vec<id> = langs
        .into_iter()
        .map(|l| ns_string::from_rust_string(env, l))
        .collect();
    let new = ns_array::from_vec(env, ns_strings);
    // Retain so the singleton lives beyond any autorelease pool drain.
    retain(env, new);
    State::get(env).preferred_languages = Some(new);
    new
}

+ (id)availableLocaleIdentifiers {
    // Return a minimal list — enough to not return nil.
    let ids = ["en_US", "en_GB", "fr_FR", "de_DE", "ja_JP", "zh_CN", "es_ES"];
    let ns_strings: Vec<id> = ids
        .iter()
        .map(|s| ns_string::from_rust_string(env, s.to_string()))
        .collect();
    let arr = ns_array::from_vec(env, ns_strings);
    autorelease(env, arr)
}

+ (id)ISOLanguageCodes {
    let codes = [
        "en","fr","de","ja","zh","es","it","pt","ru","ko","ar","nl","sv","pl","tr",
    ];
    let ns_strings: Vec<id> = codes
        .iter()
        .map(|s| ns_string::from_rust_string(env, s.to_string()))
        .collect();
    let arr = ns_array::from_vec(env, ns_strings);
    autorelease(env, arr)
}

+ (id)ISOCountryCodes {
    let codes = [
        "US","GB","FR","DE","JP","CN","ES","IT","PT","RU","KR","SA","NL","SE","PL",
    ];
    let ns_strings: Vec<id> = codes
        .iter()
        .map(|s| ns_string::from_rust_string(env, s.to_string()))
        .collect();
    let arr = ns_array::from_vec(env, ns_strings);
    autorelease(env, arr)
}

+ (id)ISOCurrencyCodes {
    let codes = ["USD","EUR","GBP","JPY","CNY","KRW","RUB","AUD","CAD","CHF"];
    let ns_strings: Vec<id> = codes
        .iter()
        .map(|s| ns_string::from_rust_string(env, s.to_string()))
        .collect();
    let arr = ns_array::from_vec(env, ns_strings);
    autorelease(env, arr)
}

+ (id)localeWithLocaleIdentifier:(id)identifier { // NSString*
    let new: id = msg_class![env; NSLocale alloc];
    let new: id = msg![env; new initWithLocaleIdentifier:identifier];
    autorelease(env, new)
}

+ (id)canonicalLocaleIdentifierFromString:(id)string { // NSString*
    // Return the string unchanged — canonicalisation is locale-library work.
    string
}

+ (id)canonicalLanguageIdentifierFromString:(id)string { // NSString*
    string
}

+ (id)localeIdentifierFromComponents:(id)_components { // NSDictionary*
    // Stub — return "en_US".
    let s = ns_string::from_rust_string(env, "en_US".to_string());
    autorelease(env, s)
}

+ (id)componentsFromLocaleIdentifier:(id)identifier { // NSString*
    let id_str = ns_string::to_rust_string(env, identifier).into_owned();
    let lang    = language_from_locale_identifier(&id_str).to_string();
    let country = country_from_locale_identifier(&id_str)
        .unwrap_or("")
        .to_string();
    let dict: id = msg_class![env; NSMutableDictionary new];
    let lang_key   = ns_string::from_rust_string(env, NSLocaleLanguageCode.to_string());
    let lang_val   = ns_string::from_rust_string(env, lang);
    () = msg![env; dict setObject:lang_val forKey:lang_key];
    release(env, lang_key);
    release(env, lang_val);
    if !country.is_empty() {
        let cc_key = ns_string::from_rust_string(env, NSLocaleCountryCode.to_string());
        let cc_val = ns_string::from_rust_string(env, country);
        () = msg![env; dict setObject:cc_val forKey:cc_key];
        release(env, cc_key);
        release(env, cc_val);
    }
    autorelease(env, dict)
}

// MARK: - Init / dealloc

- (id)initWithLocaleIdentifier:(id)string { // NSString*
    let str = ns_string::to_rust_string(env, string).into_owned();
    log_dbg!("[(NSLocale *){:?} initWithLocaleIdentifier:'{}']", this, str);
    let lang    = language_from_locale_identifier(&str).to_string();
    let country = country_from_locale_identifier(&str).unwrap_or("").to_string();
    let lang_ns    = ns_string::from_rust_string(env, lang);
    let country_ns = ns_string::from_rust_string(env, country);
    let host = env.objc.borrow_mut::<NSLocaleHostObject>(this);
    host.language_code = lang_ns;
    host.country_code  = country_ns;
    this
}

- (id)init {
    this
}

- (())dealloc {
    let &NSLocaleHostObject { country_code, language_code } =
        env.objc.borrow::<NSLocaleHostObject>(this);
    release(env, country_code);
    release(env, language_code);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

// MARK: - Identifier

- (id)localeIdentifier {
    let host = env.objc.borrow::<NSLocaleHostObject>(this);
    let (language_code, country_code) = (host.language_code, host.country_code);
    let lang    = ns_string::to_rust_string(env, language_code).into_owned();
    let country = ns_string::to_rust_string(env, country_code).into_owned();
    let id_str  = locale_identifier(&lang, &country);
    let ns = ns_string::from_rust_string(env, id_str);
    autorelease(env, ns)
}

- (id)description {
    msg![env; this localeIdentifier]
}

// MARK: - objectForKey:

- (id)objectForKey:(id)key {
    let key_str = ns_string::to_rust_string(env, key).into_owned();
    match key_str.as_str() {
        // Simple id-valued fields: copy the id out, drop borrow, return.
        NSLocaleCountryCode | kCFLocaleCountryCode => {
            env.objc.borrow::<NSLocaleHostObject>(this).country_code
        }
        NSLocaleLanguageCode | kCFLocaleLanguageCode => {
            env.objc.borrow::<NSLocaleHostObject>(this).language_code
        }
        NSLocaleIdentifier | kCFLocaleIdentifier => {
            let host = env.objc.borrow::<NSLocaleHostObject>(this);
            let (language_code, country_code) = (host.language_code, host.country_code);
            let lang    = ns_string::to_rust_string(env, language_code).into_owned();
            let country = ns_string::to_rust_string(env, country_code).into_owned();
            let id_str  = locale_identifier(&lang, &country);
            let ns = ns_string::from_rust_string(env, id_str);
            autorelease(env, ns)
        }
        NSLocaleDecimalSeparator => {
            let ns = ns_string::from_rust_string(env, ".".to_string());
            autorelease(env, ns)
        }
        NSLocaleGroupingSeparator => {
            let ns = ns_string::from_rust_string(env, ",".to_string());
            autorelease(env, ns)
        }
        NSLocaleCurrencyCode => {
            let ns = ns_string::from_rust_string(env, "USD".to_string());
            autorelease(env, ns)
        }
        NSLocaleCurrencySymbol => {
            let ns = ns_string::from_rust_string(env, "$".to_string());
            autorelease(env, ns)
        }
        NSLocaleUsesMetricSystem => {
            msg_class![env; NSNumber numberWithBool:false]
        }
        NSLocaleCalendar => {
            msg_class![env; NSCalendar currentCalendar]
        }
        NSLocaleQuotationBeginDelimiterKey => {
            // Left double quotation mark U+201C
            let ns = ns_string::from_rust_string(env, "\u{201C}".to_string());
            autorelease(env, ns)
        }
        NSLocaleQuotationEndDelimiterKey => {
            // Right double quotation mark U+201D
            let ns = ns_string::from_rust_string(env, "\u{201D}".to_string());
            autorelease(env, ns)
        }
        _ => {
            log_dbg!(
                "NSLocale objectForKey:{} - unimplemented, returning nil",
                key_str
            );
            nil
        }
    }
}

// MARK: - displayNameForKey:value:

- (id)displayNameForKey:(id)key value:(id)value {
    log_dbg!("NSLocale displayNameForKey:value: - returning value as-is");
    value
}

// MARK: - Convenience accessors (iOS 4+)

- (id)languageCode {
    env.objc.borrow::<NSLocaleHostObject>(this).language_code
}

- (id)countryCode {
    env.objc.borrow::<NSLocaleHostObject>(this).country_code
}

- (id)scriptCode {
    nil
}

- (id)variantCode {
    nil
}

- (id)decimalSeparator {
    let ns = ns_string::from_rust_string(env, ".".to_string());
    autorelease(env, ns)
}

- (id)groupingSeparator {
    let ns = ns_string::from_rust_string(env, ",".to_string());
    autorelease(env, ns)
}

- (id)currencyCode {
    let ns = ns_string::from_rust_string(env, "USD".to_string());
    autorelease(env, ns)
}

- (id)currencySymbol {
    let ns = ns_string::from_rust_string(env, "$".to_string());
    autorelease(env, ns)
}

- (bool)usesMetricSystem {
    false
}

- (id)collationIdentifier {
    nil
}

- (id)collatorIdentifier {
    nil
}

- (id)quotationBeginDelimiter {
    // Left double quotation mark U+201C
    let ns = ns_string::from_rust_string(env, "\u{201C}".to_string());
    autorelease(env, ns)
}

- (id)quotationEndDelimiter {
    // Right double quotation mark U+201D
    let ns = ns_string::from_rust_string(env, "\u{201D}".to_string());
    autorelease(env, ns)
}

- (id)calendar {
    msg_class![env; NSCalendar currentCalendar]
}

// MARK: - Equality

- (bool)isEqual:(id)other {
    if other == nil { return false; }
    if this == other { return true; }
    let a: id = msg![env; this localeIdentifier];
    let b: id = msg![env; other localeIdentifier];
    msg![env; a isEqualToString:b]
}

@end

};
