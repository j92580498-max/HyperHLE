/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//!
//! The Foundation framework.
//!
//! A concept that Foundation really likes is "class clusters": abstract classes
//! with private concrete implementations.
//! Apple has their own explanation of it
//! in [Cocoa Core Competencies](https://developer.apple.com/library/archive/documentation/General/Conceptual/DevPedia-CocoaCore/ClassCluster.html).
//!
//! Being aware of this concept will make common types like `NSArray` and
//! `NSString` easier to understand.

use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::foundation::ns_string::CFStringGetCharactersPtr;
use crate::mem::{ConstPtr, ConstVoidPtr, MutPtr};
use crate::objc::{id, msg_class, retain};
use crate::Environment;

pub mod _nib_archive_decoder;
pub mod ab_people_picker_navigation_controller;
pub mod asidentifier_manager;
pub mod chipmunk_space;
pub mod ns_array;
pub mod ns_assertion_handler;
pub mod ns_autorelease_pool;
pub mod ns_bundle;
pub mod ns_calendar;
pub mod ns_character_set;
pub mod ns_coder;
pub mod ns_condition;
pub mod ns_data;
pub mod ns_date;
pub mod ns_date_components;
pub mod ns_date_formatter;
pub mod ns_decimal_number;
pub mod ns_dictionary;
pub mod ns_enumerator;
pub mod ns_error;
pub mod ns_exception;
pub mod ns_file_handle;
pub mod ns_file_manager;
pub mod ns_host;
pub mod ns_http_cookie_storage;
pub mod ns_index_path;
pub mod ns_index_set;
pub mod ns_json_serialization;
pub mod ns_input_stream;
pub mod ns_invocation;
pub mod ns_keyed_archiver;
pub mod ns_keyed_unarchiver;
pub mod ns_locale;
pub mod ns_lock;
pub mod ns_log;
pub mod ns_metadata_query;
pub mod ns_notification;
pub mod ns_notification_center;
pub mod ns_null;
pub mod ns_number_formatter;
pub mod ns_objc_runtime;
pub mod ns_object;
pub mod ns_operation;
pub mod ns_persistent_store_coordinator;
pub mod ns_predicate;
pub mod ns_process_info;
pub mod ns_property_list_serialization;
pub mod ns_regular_expression;
pub mod ns_run_loop;
pub mod ns_scanner;
pub mod ns_hash_table;
pub mod ns_set;
pub mod ns_sort_descriptor;
pub mod ns_string;
pub mod ns_text_checking_result;
pub mod ns_thread;
pub mod ns_time_zone;
pub mod ns_timer;
pub mod ns_ubiquitous_key_value_store;
pub mod ns_undo_manager;
pub mod ns_url;
pub mod ns_url_connection;
pub mod ns_url_request;
pub mod ns_url_response;
pub mod ns_user_defaults;
pub mod ns_uuid;
pub mod ns_value;
pub mod ns_xml_parser;

pub fn NSGetSizeAndAlignment(
    env: &mut Environment,
    type_ptr: ConstPtr<u8>,
    size_out: MutPtr<NSUInteger>,
    align_out: MutPtr<NSUInteger>,
) -> ConstPtr<u8> {
    let (next_ptr, size, align) = parse_objc_type(env, type_ptr);

    if !size_out.is_null() {
        env.mem.write(size_out, size as NSUInteger);
    }
    if !align_out.is_null() {
        env.mem.write(align_out, align as NSUInteger);
    }

    next_ptr
}

fn parse_objc_type(env: &mut Environment, mut ptr: ConstPtr<u8>) -> (ConstPtr<u8>, u32, u32) {
    // Пропускаем модификаторы типа (const, in, out, inout, bycopy, byref,
    // oneway)
    loop {
        let c = env.mem.read(ptr) as char;

        match c {
            'r' | 'n' | 'N' | 'o' | 'O' | 'R' | 'V' => {
                ptr += 1;
            }
            _ => break,
        }
    }

    let c = env.mem.read(ptr) as char;

    ptr += 1;

    match c {
        // Базовые типы
        'c' | 'C' | 'B' => (ptr, 1, 1),
        's' | 'S' => (ptr, 2, 2),
        'i' | 'I' | 'l' | 'L' | 'f' | 'W' => (ptr, 4, 4),
        'q' | 'Q' | 'd' => (ptr, 8, 8),
        'v' => (ptr, 0, 1), // void

        // Указатели, объекты (id), классы (Class), селекторы (SEL), неизвестные
        // указатели (?)
        '*' | '@' | '#' | ':' | '?' => (ptr, 4, 4),

        // Указатель на другой тип: размер всегда 4, но нужно "проглотить" тип,
        // на который он указывает
        '^' => {
            let (next_ptr, _, _) = parse_objc_type(env, ptr);

            (next_ptr, 4, 4)
        }

        // Массивы: [len+type]
        '[' => {
            let mut len = 0;

            loop {
                let c = env.mem.read(ptr) as char;

                if c.is_ascii_digit() {
                    len = len * 10 + c.to_digit(10).unwrap();

                    ptr += 1;
                } else {
                    break;
                }
            }
            let (mut next_ptr, elem_size, elem_align) = parse_objc_type(env, ptr);

            if env.mem.read(next_ptr) as char == ']' {
                next_ptr += 1;
            }
            (next_ptr, len * elem_size, elem_align)
        }

        // Структуры: {name=types}
        '{' => {
            loop {
                let c = env.mem.read(ptr) as char;

                ptr += 1;
                if c == '=' || c == '}' {
                    if c == '}' {
                        return (ptr, 0, 1);
                    } // Opaque
                    break;
                }
            }
            let mut total_size = 0;

            let mut max_align = 1;
            loop {
                let c = env.mem.read(ptr) as char;

                if c == '}' {
                    ptr += 1;

                    break;
                }
                if c == '\0' {
                    break;
                }

                // Пропускаем имена полей (например: "x"f)
                if c == '"' {
                    ptr += 1;

                    loop {
                        let nc = env.mem.read(ptr) as char;
                        ptr += 1;
                        if nc == '"' {
                            break;
                        }
                    }
                } else {
                    let (next_ptr, elem_size, elem_align) = parse_objc_type(env, ptr);

                    ptr = next_ptr;

                    if elem_align > 0 {
                        let rem = total_size % elem_align;

                        if rem != 0 {
                            total_size += elem_align - rem;
                        }
                        if elem_align > max_align {
                            max_align = elem_align;
                        }
                    }
                    total_size += elem_size;
                }
            }
            if max_align > 0 {
                let rem = total_size % max_align;

                if rem != 0 {
                    total_size += max_align - rem;
                }
            }
            (ptr, total_size, max_align)
        }

        // Объединения: (name=types)
        '(' => {
            loop {
                let c = env.mem.read(ptr) as char;

                ptr += 1;
                if c == '=' || c == ')' {
                    if c == ')' {
                        return (ptr, 0, 1);
                    }
                    break;
                }
            }
            let mut max_size = 0;

            let mut max_align = 1;
            loop {
                let c = env.mem.read(ptr) as char;

                if c == ')' {
                    ptr += 1;

                    break;
                }
                if c == '\0' {
                    break;
                }

                if c == '"' {
                    ptr += 1;
                    loop {
                        let nc = env.mem.read(ptr) as char;
                        ptr += 1;
                        if nc == '"' {
                            break;
                        }
                    }
                } else {
                    let (next_ptr, elem_size, elem_align) = parse_objc_type(env, ptr);

                    ptr = next_ptr;
                    if elem_size > max_size {
                        max_size = elem_size;
                    }
                    if elem_align > max_align {
                        max_align = elem_align;
                    }
                }
            }
            (ptr, max_size, max_align)
        }

        // Битовые поля: bNUM
        'b' => {
            let mut bits = 0;

            loop {
                let c = env.mem.read(ptr) as char;

                if c.is_ascii_digit() {
                    bits = bits * 10 + c.to_digit(10).unwrap();

                    ptr += 1;
                } else {
                    break;
                }
            }
            let bytes = bits.div_ceil(8);

            (ptr, bytes, 1)
        }

        _ => (ptr, 0, 1),
    }
}

/// `NSFoundationVersionNumber` is a global `double` exported by Foundation
/// that apps use as a runtime OS-version probe (`if (NSFoundationVersionNumber
/// >= NSFoundationVersionNumber_iPhoneOS_4_0)`). We expose touchHLE's
/// nominal "iOS 4.0" identity (build 8A293, see `libc/sys/utsname`) — the
/// constant `751.32` is the documented Foundation version for iOS 4.0.
fn ns_foundation_version_number(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u64> = env.mem.alloc(8).cast();
    env.mem.write(ptr, 751.32f64.to_bits());
    ptr.cast().cast_const()
}

/// CoreFoundation's twin of `NSFoundationVersionNumber`. iOS 4.0 reports
/// 550.32 (`kCFCoreFoundationVersionNumber_iPhoneOS_4_0`), which keeps
/// `if (kCFCoreFoundationVersionNumber >= kCFCoreFoundationVersionNumber_iPhoneOS_4_0)`
/// gates on the iOS-4 branch — matches the rest of touchHLE's identity.
fn cf_core_foundation_version_number(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u64> = env.mem.alloc(8).cast();
    env.mem.write(ptr, 550.32f64.to_bits());
    ptr.cast().cast_const()
}

/// `UIViewNoIntrinsicMetric` is `(CGFloat)-1.0`, used as a sentinel value
/// for "this view has no intrinsic content size on this axis" by AutoLayout.
/// On 32-bit iOS, `CGFloat` is `float`; the symbol is a 4-byte little-endian
/// `-1.0f`.
fn ui_view_no_intrinsic_metric(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u32> = env.mem.alloc(4).cast();
    env.mem.write(ptr, (-1.0f32).to_bits());
    ptr.cast().cast_const()
}

/// `NSURLSessionTransferSizeUnknown` is declared as
/// `FOUNDATION_EXPORT const int64_t NSURLSessionTransferSizeUnknown`, with
/// the literal value `-1LL`. Apps test
/// `task.countOfBytesExpectedToReceive == NSURLSessionTransferSizeUnknown`
/// to detect when the server didn't send a `Content-Length`. See
/// <https://developer.apple.com/documentation/foundation/nsurlsessiontransfersizeunknown>.
/// The symbol is an 8-byte little-endian `-1`.
fn ns_url_session_transfer_size_unknown(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u64> = env.mem.alloc(8).cast();
    env.mem.write(ptr, (-1i64) as u64);
    ptr.cast().cast_const()
}

/// `kCMTimeRangeZero` — `CMTimeRange` (48 bytes) of all-zero bytes.
///
/// From `<CoreMedia/CMTime.h>`: `CMTimeRange = { CMTime start; CMTime
/// duration; }`. `CMTime` is `{ int64 value; int32 timescale; uint32 flags;
/// int64 epoch }` (=24 bytes), so the
fn kcm_timetime_range_zero(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u8> = env.mem.alloc(48).cast();
    env.mem.bytes_at_mut(ptr, 48).fill(0);
    ptr.cast().cast_const()
}

/// `matrix_identity_float4x4` — 4×4 identity matrix in column-major f32 order.
///
/// From `<simd/matrix.h>`: 16× f32 in column-major order per Apple's simd.h
/// convention, with 1.0f at the four diagonal positions (offsets 0, 5, 10,
/// 15 in the 16-element matrix, i.e. byte offsets 0, 20, 40, 60).
fn matrix_identity_float4x4(env: &mut Environment) -> ConstVoidPtr {
    let base: MutPtr<u8> = env.mem.alloc(64).cast();
    env.mem.bytes_at_mut(base, 64).fill(0);
    // Write 1.0f32 at column-major diagonal positions: byte offsets 0, 20, 40, 60.
    for &offset in &[0u32, 20, 40, 60] {
        let p: MutPtr<f32> = MutPtr::from_bits(base.to_bits() + offset);
        env.mem.write(p, 1.0f32);
    }
    base.cast().cast_const()
}

/// `MKMapRectNull` — the "null" map rect from `<MapKit/MKGeometry.h>`.
///
/// `MKMapRect` is `{ MKMapPoint origin; MKMapSize size; }` where both
/// `MKMapPoint` and `MKMapSize` are pairs of `double`, i.e. 32 bytes total.
/// Apple defines `MKMapRectNull` as `(MKMapRect){ { INFINITY, INFINITY },
/// { 0, 0 } }`, which `MKMapRectIsNull()` recognises by the infinite origin.
fn mk_map_rect_null(env: &mut Environment) -> ConstVoidPtr {
    let base: MutPtr<u8> = env.mem.alloc(32).cast();
    env.mem.bytes_at_mut(base, 32).fill(0);
    // origin.x and origin.y = INFINITY (f64) at byte offsets 0 and 8.
    for &offset in &[0u32, 8] {
        let p: MutPtr<f64> = MutPtr::from_bits(base.to_bits() + offset);
        env.mem.write(p, f64::INFINITY);
    }
    base.cast().cast_const()
}

/// `UILayoutFittingCompressedSize` — `CGSize{0,0}` (8 bytes of zero) per
/// Apple's `<UIKit/UIGeometry.h>`. This is the value AutoLayout uses
/// when asking a view for its minimum size.
fn ui_layout_fitting_compressed_size(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u8> = env.mem.alloc(8).cast();
    env.mem.bytes_at_mut(ptr, 8).fill(0);
    ptr.cast().cast_const()
}

/// `UILayoutFittingExpandedSize` — CGSize with two `CGFLOAT_MAX` values.
///
/// From `<UIKit/UIGeometry.h>`: `UILayoutFittingExpandedSize` uses
/// `CGFLOAT_MAX` (i.e. `f32::MAX`) for both width and height.
fn ui_layout_fitting_expanded_size(env: &mut Environment) -> ConstVoidPtr {
    let base: MutPtr<u8> = env.mem.alloc(8).cast();
    env.mem.bytes_at_mut(base, 8).fill(0);
    for &offset in &[0u32, 4] {
        let p: MutPtr<f32> = MutPtr::from_bits(base.to_bits() + offset);
        env.mem.write(p, f32::MAX);
    }
    base.cast().cast_const()
}

/// `__NSArray0__` — the Apple Objective-C runtime's shared immutable empty
/// `NSArray` singleton. Clang emits a reference to this symbol for an empty
/// array literal `@[]`. We back it with a real, retained empty NSArray so
/// guest code that messages it (e.g. `-count`, `-objectEnumerator`) behaves
/// like a genuine empty array instead of crashing on a NULL isa.
fn ns_array0(env: &mut Environment) -> ConstVoidPtr {
    let arr: id = msg_class![env; NSArray array];
    retain(env, arr);
    arr.cast().cast_const()
}

/// `__NSDictionary0__` — the runtime's shared immutable empty `NSDictionary`
/// singleton, referenced by an empty dictionary literal `@{}`. Backed by a
/// real, retained empty NSDictionary for the same reason as `ns_array0`.
fn ns_dictionary0(env: &mut Environment) -> ConstVoidPtr {
    let dict: id = msg_class![env; NSDictionary dictionary];
    retain(env, dict);
    dict.cast().cast_const()
}

/// `AVCaptureExposureDurationCurrent` — a sentinel `CMTime` (24 bytes) meaning
/// "leave the exposure duration unchanged". Apple defines it as
/// `kCMTimeInvalid`, whose `flags` field has the `kCMTimeFlags_Valid` bit
/// clear, so an all-zero struct is the correct representation.
fn av_capture_exposure_duration_current(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u8> = env.mem.alloc(24).cast();
    env.mem.bytes_at_mut(ptr, 24).fill(0);
    ptr.cast().cast_const()
}

/// `AVCaptureISOCurrent` — a sentinel `float` (-1.0) meaning "leave the ISO
/// unchanged", per `<AVFoundation/AVCaptureDevice.h>`.
fn av_capture_iso_current(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<f32> = env.mem.alloc(4).cast();
    env.mem.write(ptr, -1.0f32);
    ptr.cast().cast_const()
}

pub const STUB_CONSTANTS: ConstantExports = &[
    // _NSLocalizedFailureReasonErrorKey and _NSURLErrorDomain are exported
    // from foundation::ns_error::CONSTANTS; not duplicated here.
    (
        "_NSFoundationVersionNumber",
        HostConstant::Custom(ns_foundation_version_number),
    ),
    (
        "_kCFCoreFoundationVersionNumber",
        HostConstant::Custom(cf_core_foundation_version_number),
    ),
    (
        "_UIViewNoIntrinsicMetric",
        HostConstant::Custom(ui_view_no_intrinsic_metric),
    ),
    // -----------------------------------------------------------------
    // NSError userInfo keys not in ns_error::CONSTANTS.
    // -----------------------------------------------------------------
    (
        "_NSRecoveryAttempterErrorKey",
        HostConstant::NSString("NSRecoveryAttempterErrorKey"),
    ),
    // -----------------------------------------------------------------
    // Attributed-string attribute name keys (UIKit re-exports them as
    // NSString constants on iOS 6+; the canonical literal values are the
    // names themselves, which is what real iOS uses internally and what
    // any sane comparison (CFEqual / isEqualToString:) relies on).
    // -----------------------------------------------------------------
    ("_NSFontAttributeName", HostConstant::NSString("NSFont")),
    (
        "_NSForegroundColorAttributeName",
        HostConstant::NSString("NSColor"),
    ),
    (
        "_NSBackgroundColorAttributeName",
        HostConstant::NSString("NSBackgroundColor"),
    ),
    (
        "_NSParagraphStyleAttributeName",
        HostConstant::NSString("NSParagraphStyle"),
    ),
    (
        "_NSStrokeColorAttributeName",
        HostConstant::NSString("NSStrokeColor"),
    ),
    (
        "_NSStrokeWidthAttributeName",
        HostConstant::NSString("NSStrokeWidth"),
    ),
    ("_NSShadowAttributeName", HostConstant::NSString("NSShadow")),
    ("_NSKernAttributeName", HostConstant::NSString("NSKern")),
    (
        "_NSLigatureAttributeName",
        HostConstant::NSString("NSLigature"),
    ),
    (
        "_NSUnderlineStyleAttributeName",
        HostConstant::NSString("NSUnderline"),
    ),
    (
        "_NSUnderlineColorAttributeName",
        HostConstant::NSString("NSUnderlineColor"),
    ),
    (
        "_NSStrikethroughStyleAttributeName",
        HostConstant::NSString("NSStrikethrough"),
    ),
    (
        "_NSStrikethroughColorAttributeName",
        HostConstant::NSString("NSStrikethroughColor"),
    ),
    (
        "_NSObliquenessAttributeName",
        HostConstant::NSString("NSObliqueness"),
    ),
    (
        "_NSExpansionAttributeName",
        HostConstant::NSString("NSExpansion"),
    ),
    (
        "_NSBaselineOffsetAttributeName",
        HostConstant::NSString("NSBaselineOffset"),
    ),
    (
        "_NSWritingDirectionAttributeName",
        HostConstant::NSString("NSWritingDirection"),
    ),
    (
        "_NSVerticalGlyphFormAttributeName",
        HostConstant::NSString("NSVerticalGlyphForm"),
    ),
    (
        "_NSTextEffectAttributeName",
        HostConstant::NSString("NSTextEffect"),
    ),
    (
        "_NSAttachmentAttributeName",
        HostConstant::NSString("NSAttachment"),
    ),
    ("_NSLinkAttributeName", HostConstant::NSString("NSLink")),
    // -----------------------------------------------------------------
    // KVO change-dictionary keys.
    // _NSKeyValueChangeNewKey is exported from objc::CONSTANTS already.
    // -----------------------------------------------------------------
    ("_NSKeyValueChangeKindKey", HostConstant::NSString("kind")),
    ("_NSKeyValueChangeOldKey", HostConstant::NSString("old")),
    (
        "_NSKeyValueChangeIndexesKey",
        HostConstant::NSString("indexes"),
    ),
    (
        "_NSKeyValueChangeNotificationIsPriorKey",
        HostConstant::NSString("notificationIsPrior"),
    ),
    // -----------------------------------------------------------------
    // NSError / NSURL extra userInfo keys not in ns_error::CONSTANTS.
    // -----------------------------------------------------------------
    (
        "_NSURLErrorFailingURLErrorKey",
        HostConstant::NSString("NSErrorFailingURLKey"),
    ),
    (
        "_NSURLErrorFailingURLStringErrorKey",
        HostConstant::NSString("NSErrorFailingURLStringKey"),
    ),
    (
        "_NSURLErrorFailingURLPeerTrustErrorKey",
        HostConstant::NSString("NSURLErrorFailingURLPeerTrustErrorKey"),
    ),
    // -----------------------------------------------------------------
    // CFDateFormatter property key constants (CFStringRef).
    // These are string keys used with CFDateFormatterSetProperty /
    // CFDateFormatterCopyProperty. The literal value of each constant
    // is its own symbol name, matching Apple's CoreFoundation behaviour.
    // -----------------------------------------------------------------
    (
        "_kCFDateFormatterCalendar",
        HostConstant::NSString("kCFDateFormatterCalendar"),
    ),
    (
        "_kCFDateFormatterMonthSymbols",
        HostConstant::NSString("kCFDateFormatterMonthSymbols"),
    ),
    (
        "_kCFDateFormatterShortMonthSymbols",
        HostConstant::NSString("kCFDateFormatterShortMonthSymbols"),
    ),
    (
        "_kCFDateFormatterShortWeekdaySymbols",
        HostConstant::NSString("kCFDateFormatterShortWeekdaySymbols"),
    ),
    (
        "_kCFDateFormatterStandaloneMonthSymbols",
        HostConstant::NSString("kCFDateFormatterStandaloneMonthSymbols"),
    ),
    (
        "_kCFDateFormatterTimeZone",
        HostConstant::NSString("kCFDateFormatterTimeZone"),
    ),
    (
        "_kCFDateFormatterVeryShortStandaloneWeekdaySymbols",
        HostConstant::NSString("kCFDateFormatterVeryShortStandaloneWeekdaySymbols"),
    ),
    (
        "_kCFDateFormatterWeekdaySymbols",
        HostConstant::NSString("kCFDateFormatterWeekdaySymbols"),
    ),
    // -----------------------------------------------------------------
    // NSThread notification constants.
    // -----------------------------------------------------------------
    (
        "_NSThreadWillExitNotification",
        HostConstant::NSString("NSThreadWillExitNotification"),
    ),
    (
        "_NSWillBecomeMultiThreadedNotification",
        HostConstant::NSString("NSWillBecomeMultiThreadedNotification"),
    ),
    (
        "_NSDidBecomeSingleThreadedNotification",
        HostConstant::NSString("NSDidBecomeSingleThreadedNotification"),
    ),
    // -----------------------------------------------------------------
    // NSStream property / userInfo keys.
    // -----------------------------------------------------------------
    (
        "_NSStreamFileCurrentOffsetKey",
        HostConstant::NSString("kCFStreamPropertyFileCurrentOffset"),
    ),
    (
        "_NSStreamDataWrittenToMemoryStreamKey",
        HostConstant::NSString("kCFStreamPropertyDataWritten"),
    ),
    (
        "_NSStreamSocketSecurityLevelKey",
        HostConstant::NSString("kCFStreamPropertySocketSecurityLevel"),
    ),
    (
        "_NSStreamSocketSecurityLevelNone",
        HostConstant::NSString("kCFStreamSocketSecurityLevelNone"),
    ),
    (
        "_NSStreamSocketSecurityLevelSSLv2",
        HostConstant::NSString("kCFStreamSocketSecurityLevelSSLv2"),
    ),
    (
        "_NSStreamSocketSecurityLevelSSLv3",
        HostConstant::NSString("kCFStreamSocketSecurityLevelSSLv3"),
    ),
    (
        "_NSStreamSocketSecurityLevelTLSv1",
        HostConstant::NSString("kCFStreamSocketSecurityLevelTLSv1"),
    ),
    (
        "_NSStreamSocketSecurityLevelNegotiatedSSL",
        HostConstant::NSString("kCFStreamSocketSecurityLevelNegotiatedSSL"),
    ),
    // -----------------------------------------------------------------
    // NSStream SOCKS-proxy configuration keys (CFNetwork bridge).
    // The values are the CFNetwork string names Apple's bridge layer
    // uses internally (see <CFNetwork/CFSocketStream.h> and
    // NSStream.h).
    // -----------------------------------------------------------------
    (
        "_NSStreamSOCKSProxyConfigurationKey",
        HostConstant::NSString("kCFStreamPropertySOCKSProxy"),
    ),
    (
        "_NSStreamSOCKSProxyHostKey",
        HostConstant::NSString("SOCKSProxy"),
    ),
    (
        "_NSStreamSOCKSProxyPortKey",
        HostConstant::NSString("SOCKSPort"),
    ),
    (
        "_NSStreamSOCKSProxyVersionKey",
        HostConstant::NSString("kCFStreamPropertySOCKSVersion"),
    ),
    (
        "_NSStreamSOCKSProxyUserKey",
        HostConstant::NSString("SOCKSUser"),
    ),
    (
        "_NSStreamSOCKSProxyPasswordKey",
        HostConstant::NSString("SOCKSPassword"),
    ),
    // SOCKS version sentinels.
    (
        "_NSStreamSOCKSProxyVersion4",
        HostConstant::NSString("kCFStreamSocketSOCKSVersion4"),
    ),
    (
        "_NSStreamSOCKSProxyVersion5",
        HostConstant::NSString("kCFStreamSocketSOCKSVersion5"),
    ),
    // -----------------------------------------------------------------
    // NSStream network service type keys (telephony hints).
    // -----------------------------------------------------------------
    (
        "_NSStreamNetworkServiceType",
        HostConstant::NSString("kCFStreamNetworkServiceType"),
    ),
    (
        "_NSStreamNetworkServiceTypeVoIP",
        HostConstant::NSString("kCFStreamNetworkServiceTypeVoIP"),
    ),
    (
        "_NSStreamNetworkServiceTypeVideo",
        HostConstant::NSString("kCFStreamNetworkServiceTypeVideo"),
    ),
    (
        "_NSStreamNetworkServiceTypeBackground",
        HostConstant::NSString("kCFStreamNetworkServiceTypeBackground"),
    ),
    (
        "_NSStreamNetworkServiceTypeVoice",
        HostConstant::NSString("kCFStreamNetworkServiceTypeVoice"),
    ),
    (
        "_NSStreamNetworkServiceTypeCallSignaling",
        HostConstant::NSString("kCFStreamNetworkServiceTypeCallSignaling"),
    ),
    // -----------------------------------------------------------------
    // NSAttributedString HTML/RTF import attribute keys
    // (see UIKit's NSAttributedString.h "loading" category).
    // -----------------------------------------------------------------
    (
        "_NSDocumentTypeDocumentAttribute",
        HostConstant::NSString("DocumentType"),
    ),
    (
        "_NSCharacterEncodingDocumentAttribute",
        HostConstant::NSString("CharacterEncoding"),
    ),
    (
        "_NSDefaultAttributesDocumentAttribute",
        HostConstant::NSString("DefaultAttributes"),
    ),
    ("_NSHTMLTextDocumentType", HostConstant::NSString("NSHTML")),
    (
        "_NSPlainTextDocumentType",
        HostConstant::NSString("NSPlainText"),
    ),
    ("_NSRTFTextDocumentType", HostConstant::NSString("NSRTF")),
    (
        "_NSRTFDTextDocumentType",
        HostConstant::NSString("NSRTFD"),
    ),
    // -----------------------------------------------------------------
    // NSProgress kind constants (NSProgress.h).
    // -----------------------------------------------------------------
    (
        "_NSProgressKindFile",
        HostConstant::NSString("NSProgressKindFile"),
    ),
    // -----------------------------------------------------------------
    // CFProxySupport.h: proxy dictionary key constants.
    // -----------------------------------------------------------------
    (
        "_kCFProxyTypeAutoConfigurationJavaScript",
        HostConstant::NSString("kCFProxyTypeAutoConfigurationJavaScript"),
    ),
    (
        "_kCFProxyAutoConfigurationJavaScriptKey",
        HostConstant::NSString("kCFProxyAutoConfigurationJavaScriptKey"),
    ),
    (
        "_kCFProxyUsernameKey",
        HostConstant::NSString("kCFProxyUsernameKey"),
    ),
    (
        "_kCFProxyPasswordKey",
        HostConstant::NSString("kCFProxyPasswordKey"),
    ),
    // -----------------------------------------------------------------
    // NSFileHandle notification names / userInfo keys.
    // -----------------------------------------------------------------
    (
        "_NSFileHandleDataAvailableNotification",
        HostConstant::NSString("NSFileHandleDataAvailableNotification"),
    ),
    (
        "_NSFileHandleReadCompletionNotification",
        HostConstant::NSString("NSFileHandleReadCompletionNotification"),
    ),
    (
        "_NSFileHandleReadToEndOfFileCompletionNotification",
        HostConstant::NSString("NSFileHandleReadToEndOfFileCompletionNotification"),
    ),
    (
        "_NSFileHandleConnectionAcceptedNotification",
        HostConstant::NSString("NSFileHandleConnectionAcceptedNotification"),
    ),
    (
        "_NSFileHandleNotificationDataItem",
        HostConstant::NSString("NSFileHandleNotificationDataItem"),
    ),
    (
        "_NSFileHandleNotificationFileHandleItem",
        HostConstant::NSString("NSFileHandleNotificationFileHandleItem"),
    ),
    // -----------------------------------------------------------------
    // NSMetadataQuery item / scope / notification keys.
    // <https://developer.apple.com/documentation/foundation/nsmetadataquery>
    // -----------------------------------------------------------------
    (
        "_NSMetadataQueryDidFinishGatheringNotification",
        HostConstant::NSString("NSMetadataQueryDidFinishGatheringNotification"),
    ),
    (
        "_NSMetadataQueryDidUpdateNotification",
        HostConstant::NSString("NSMetadataQueryDidUpdateNotification"),
    ),
    (
        "_NSMetadataQueryDidStartGatheringNotification",
        HostConstant::NSString("NSMetadataQueryDidStartGatheringNotification"),
    ),
    (
        "_NSMetadataQueryGatheringProgressNotification",
        HostConstant::NSString("NSMetadataQueryGatheringProgressNotification"),
    ),
    (
        "_NSMetadataQueryUbiquitousDocumentsScope",
        HostConstant::NSString("NSMetadataQueryUbiquitousDocumentsScope"),
    ),
    (
        "_NSMetadataQueryUbiquitousDataScope",
        HostConstant::NSString("NSMetadataQueryUbiquitousDataScope"),
    ),
    (
        "_NSMetadataItemURLKey",
        HostConstant::NSString("kMDItemURL"),
    ),
    (
        "_NSMetadataItemFSNameKey",
        HostConstant::NSString("kMDItemFSName"),
    ),
    (
        "_NSMetadataItemFSContentChangeDateKey",
        HostConstant::NSString("kMDItemFSContentChangeDate"),
    ),
    (
        "_NSMetadataUbiquitousItemIsDownloadedKey",
        HostConstant::NSString("NSMetadataUbiquitousItemIsDownloadedKey"),
    ),
    // -----------------------------------------------------------------
    // NSURL ubiquity / iCloud item attribute keys, per
    // <https://developer.apple.com/documentation/foundation/nsurl>
    // -----------------------------------------------------------------
    (
        "_NSURLIsUbiquitousItemKey",
        HostConstant::NSString("NSURLIsUbiquitousItemKey"),
    ),
    (
        "_NSURLUbiquitousItemIsDownloadedKey",
        HostConstant::NSString("NSURLUbiquitousItemIsDownloadedKey"),
    ),
    (
        "_NSURLUbiquitousItemIsDownloadingKey",
        HostConstant::NSString("NSURLUbiquitousItemIsDownloadingKey"),
    ),
    (
        "_NSURLUbiquitousItemIsUploadedKey",
        HostConstant::NSString("NSURLUbiquitousItemIsUploadedKey"),
    ),
    (
        "_NSURLUbiquitousItemIsUploadingKey",
        HostConstant::NSString("NSURLUbiquitousItemIsUploadingKey"),
    ),
    (
        "_NSURLUbiquitousItemHasUnresolvedConflictsKey",
        HostConstant::NSString("NSURLUbiquitousItemHasUnresolvedConflictsKey"),
    ),
    // -----------------------------------------------------------------
    // NSUbiquitousKeyValueStore notification keys.
    // <https://developer.apple.com/documentation/foundation/nsubiquitouskeyvaluestore>
    // -----------------------------------------------------------------
    (
        "_NSUbiquitousKeyValueStoreChangeReasonKey",
        HostConstant::NSString("NSUbiquitousKeyValueStoreChangeReasonKey"),
    ),
    (
        "_NSUbiquitousKeyValueStoreChangedKeysKey",
        HostConstant::NSString("NSUbiquitousKeyValueStoreChangedKeysKey"),
    ),
    (
        "_NSUbiquitousKeyValueStoreDidChangeExternallyNotification",
        HostConstant::NSString("NSUbiquitousKeyValueStoreDidChangeExternallyNotification"),
    ),
    // -----------------------------------------------------------------
    // NSProcessInfo / NSURLError additional userInfo keys.
    // <https://developer.apple.com/documentation/foundation/nsprocessinfo>
    // -----------------------------------------------------------------
    (
        "_NSProcessInfoPowerStateDidChangeNotification",
        HostConstant::NSString("NSProcessInfoPowerStateDidChangeNotification"),
    ),
    (
        "_NSURLErrorBackgroundTaskCancelledReasonKey",
        HostConstant::NSString("NSURLErrorBackgroundTaskCancelledReasonKey"),
    ),
    // -----------------------------------------------------------------
    // NSUserDefaults / NSUndoManager / NSHTTPCookie / NSUbiquity
    // notification names. Apple declares these as
    // `FOUNDATION_EXPORT NSNotificationName const ...` in their public
    // headers. The literal value of each constant is its own symbol
    // name — that is the value posted to `NSNotificationCenter`.
    //
    // References:
    // * Apple [NSUserDefaultsDidChangeNotification](https://developer.apple.com/documentation/foundation/userdefaults/didchangenotification)
    // * Apple [NSUndoManager notifications](https://developer.apple.com/documentation/foundation/undomanager)
    // * Apple [NSHTTPCookieVersion](https://developer.apple.com/documentation/foundation/nshttpcookie)
    // * Apple [NSUbiquityIdentityDidChangeNotification](https://developer.apple.com/documentation/foundation/nsubiquityidentitydidchangenotification)
    // -----------------------------------------------------------------
    (
        "_NSUserDefaultsDidChangeNotification",
        HostConstant::NSString("NSUserDefaultsDidChangeNotification"),
    ),
    (
        "_NSUndoManagerCheckpointNotification",
        HostConstant::NSString("NSUndoManagerCheckpointNotification"),
    ),
    (
        "_NSUndoManagerDidOpenUndoGroupNotification",
        HostConstant::NSString("NSUndoManagerDidOpenUndoGroupNotification"),
    ),
    (
        "_NSUndoManagerDidCloseUndoGroupNotification",
        HostConstant::NSString("NSUndoManagerDidCloseUndoGroupNotification"),
    ),
    (
        "_NSUndoManagerDidUndoChangeNotification",
        HostConstant::NSString("NSUndoManagerDidUndoChangeNotification"),
    ),
    (
        "_NSUndoManagerDidRedoChangeNotification",
        HostConstant::NSString("NSUndoManagerDidRedoChangeNotification"),
    ),
    (
        "_NSUndoManagerWillUndoChangeNotification",
        HostConstant::NSString("NSUndoManagerWillUndoChangeNotification"),
    ),
    (
        "_NSUndoManagerWillRedoChangeNotification",
        HostConstant::NSString("NSUndoManagerWillRedoChangeNotification"),
    ),
    (
        "_NSUndoManagerWillCloseUndoGroupNotification",
        HostConstant::NSString("NSUndoManagerWillCloseUndoGroupNotification"),
    ),
    // `NSHTTPCookieVersion` is declared in `<Foundation/NSHTTPCookie.h>`
    // as a cookie-property dictionary key whose literal value is
    // `"Version"`. Apple's CFNetwork uses it to read the cookie's
    // RFC-2965 version field.
    (
        "_NSHTTPCookieVersion",
        HostConstant::NSString("Version"),
    ),
    (
        "_NSUbiquityIdentityDidChangeNotification",
        HostConstant::NSString("NSUbiquityIdentityDidChangeNotification"),
    ),
    // -----------------------------------------------------------------
    // NSURLSession constants. iOS 7+ networking. Apps occasionally link
    // these on iOS 4-targeted bundles when they ship "legacy + modern"
    // dual-stack networking code. Per
    // <https://developer.apple.com/documentation/foundation/nsurlsession>
    // and <https://developer.apple.com/documentation/foundation/url_loading_system>,
    // the *Identifier* / *Notification* strings are exported as
    // `NSString *const` whose literal value matches the symbol name; the
    // sentinel `NSURLSessionTransferSizeUnknown` is an `int64_t` = -1.
    // -----------------------------------------------------------------
    (
        "_NSURLSessionTransferSizeUnknown",
        HostConstant::Custom(ns_url_session_transfer_size_unknown),
    ),
    (
        "_NSURLSessionDownloadTaskResumeData",
        HostConstant::NSString("NSURLSessionDownloadTaskResumeData"),
    ),
    (
        "_NSURLSessionUploadTaskResponseBodyKey",
        HostConstant::NSString("NSURLSessionUploadTaskResponseBodyKey"),
    ),
    (
        "_NSURLSessionTaskPriorityLow",
        HostConstant::NSString("NSURLSessionTaskPriorityLow"),
    ),
    (
        "_NSURLSessionTaskPriorityDefault",
        HostConstant::NSString("NSURLSessionTaskPriorityDefault"),
    ),
    (
        "_NSURLSessionTaskPriorityHigh",
        HostConstant::NSString("NSURLSessionTaskPriorityHigh"),
    ),
    // -----------------------------------------------------------------
    // NSUserActivity constants. iOS 8+ Handoff API. Like the
    // NSURLSession constants above, some bundles reference these even
    // when targeting an older OS version, so dyld needs them to
    // resolve. Per
    // <https://developer.apple.com/documentation/foundation/nsuseractivity>,
    // `NSUserActivityTypeBrowsingWeb` is a special activity type that the
    // OS uses to indicate the user is browsing a URL in Safari, and the
    // literal string value is the symbol name itself.
    // -----------------------------------------------------------------
    (
        "_NSUserActivityTypeBrowsingWeb",
        HostConstant::NSString("NSUserActivityTypeBrowsingWeb"),
    ),
    // -----------------------------------------------------------------
    // `NSHTTPCookie` property-dictionary keys. Apple declares them in
    // `<Foundation/NSHTTPCookie.h>` as `NSString *const`; the literal
    // values match Apple's headers (some are short legacy strings, see
    // `<https://developer.apple.com/documentation/foundation/nshttpcookie>`).
    // -----------------------------------------------------------------
    (
        "_NSHTTPCookieExpires",
        HostConstant::NSString("Expires"),
    ),
    (
        "_NSHTTPCookieName",
        HostConstant::NSString("Name"),
    ),
    (
        "_NSHTTPCookieValue",
        HostConstant::NSString("Value"),
    ),
    (
        "_NSHTTPCookieDomain",
        HostConstant::NSString("Domain"),
    ),
    (
        "_NSHTTPCookiePath",
        HostConstant::NSString("Path"),
    ),
    (
        "_NSHTTPCookieSecure",
        HostConstant::NSString("Secure"),
    ),
    (
        "_NSHTTPCookieDiscard",
        HostConstant::NSString("Discard"),
    ),
    (
        "_NSHTTPCookieMaximumAge",
        HostConstant::NSString("Max-Age"),
    ),
    (
        "_NSHTTPCookieComment",
        HostConstant::NSString("Comment"),
    ),
    (
        "_NSHTTPCookieCommentURL",
        HostConstant::NSString("CommentURL"),
    ),
    (
        "_NSHTTPCookieOriginURL",
        HostConstant::NSString("OriginURL"),
    ),
    (
        "_NSHTTPCookiePort",
        HostConstant::NSString("Port"),
    ),
    (
        "_NSHTTPCookieManagerCookiesChangedNotification",
        HostConstant::NSString("NSHTTPCookieManagerCookiesChangedNotification"),
    ),
    (
        "_NSHTTPCookieManagerAcceptPolicyChangedNotification",
        HostConstant::NSString("NSHTTPCookieManagerAcceptPolicyChangedNotification"),
    ),
    // -----------------------------------------------------------------
    // `NSXMLParserErrorDomain` — the error domain used by NSError
    // userInfo from `<Foundation/NSXMLParser.h>`. Apple ships this as
    // `FOUNDATION_EXPORT NSString * const`; the literal value is the
    // constant name itself.
    // <https://developer.apple.com/documentation/foundation/nsxmlparsererrordomain>
    // -----------------------------------------------------------------
    (
        "_NSXMLParserErrorDomain",
        HostConstant::NSString("NSXMLParserErrorDomain"),
    ),
    // -----------------------------------------------------------------
    // AVFoundation notification constants.
    // From <AVFoundation/AVPlayerItem.h>: the value of these constants
    // is exactly the constant name.
    // -----------------------------------------------------------------
    (
        "_AVPlayerItemTimeJumpedNotification",
        HostConstant::NSString("AVPlayerItemTimeJumpedNotification"),
    ),
    // -----------------------------------------------------------------
    // Photos framework constants.
    // -----------------------------------------------------------------
    (
        "_PHImageErrorKey",
        HostConstant::NSString("PHImageErrorKey"),
    ),
    // -----------------------------------------------------------------
    // StoreKit framework constants.
    // -----------------------------------------------------------------
    (
        "_SKReceiptPropertyIsExpired",
        HostConstant::NSString("SKReceiptPropertyIsExpired"),
    ),
    (
        "_SKReceiptPropertyIsRevoked",
        HostConstant::NSString("SKReceiptPropertyIsRevoked"),
    ),
    (
        "_SKReceiptPropertyIsVolumePurchase",
        HostConstant::NSString("SKReceiptPropertyIsVolumePurchase"),
    ),
    (
        "_SKStoreProductParameterProductIdentifier",
        HostConstant::NSString("SKStoreProductParameterProductIdentifier"),
    ),
    // -----------------------------------------------------------------
    // UIKit accessibility/input constants.
    // -----------------------------------------------------------------
    (
        "_UIAccessibilityScreenChangedNotification",
        HostConstant::NSString("UIAccessibilityScreenChangedNotification"),
    ),
    (
        "_UITextInputCurrentInputModeDidChangeNotification",
        HostConstant::NSString("UITextInputCurrentInputModeDidChangeNotification"),
    ),
    // -----------------------------------------------------------------
    // Security constants.
    // -----------------------------------------------------------------
    (
        "_kSecAttrIsPermanent",
        HostConstant::NSString("isper"),
    ),
    (
        "_kSecUseAuthenticationUI",
        HostConstant::NSString("u_AuthUI"),
    ),
    (
        "_kSecUseAuthenticationUIFail",
        HostConstant::NSString("u_AuthUIF"),
    ),
    // -----------------------------------------------------------------
    // CMTime constants.
    // -----------------------------------------------------------------
    (
        "_kCMTimeRangeZero",
        HostConstant::Custom(kcm_timetime_range_zero),
    ),
    // -----------------------------------------------------------------
    // simd constants.
    // -----------------------------------------------------------------
    (
        "_matrix_identity_float4x4",
        HostConstant::Custom(matrix_identity_float4x4),
    ),
    // -----------------------------------------------------------------
    // UIKit hardware-keyboard input strings (<UIKit/UIKeyCommand.h>).
    // Apps register UIKeyCommands against these; touchHLE does not deliver
    // hardware-keyboard events, but the symbols must resolve to a stable
    // non-nil NSString so the app's +keyCommands setup does not crash.
    // -----------------------------------------------------------------
    ("_UIKeyInputUpArrow", HostConstant::NSString("UIKeyInputUpArrow")),
    (
        "_UIKeyInputDownArrow",
        HostConstant::NSString("UIKeyInputDownArrow"),
    ),
    (
        "_UIKeyInputLeftArrow",
        HostConstant::NSString("UIKeyInputLeftArrow"),
    ),
    (
        "_UIKeyInputRightArrow",
        HostConstant::NSString("UIKeyInputRightArrow"),
    ),
    ("_UIKeyInputEscape", HostConstant::NSString("UIKeyInputEscape")),
    (
        "_UIUserNotificationActionResponseTypedTextKey",
        HostConstant::NSString("UIUserNotificationActionResponseTypedTextKey"),
    ),
    // -----------------------------------------------------------------
    // CoreAnimation CATextLayer alignment modes (<QuartzCore/CATextLayer.h>).
    // These are documented kCAAlignment* string values.
    // -----------------------------------------------------------------
    ("_kCAAlignmentLeft", HostConstant::NSString("left")),
    ("_kCAAlignmentRight", HostConstant::NSString("right")),
    ("_kCAAlignmentCenter", HostConstant::NSString("center")),
    ("_kCAAlignmentJustified", HostConstant::NSString("justified")),
    ("_kCAAlignmentNatural", HostConstant::NSString("natural")),
    // -----------------------------------------------------------------
    // AVFoundation time-pitch algorithm identifiers
    // (<AVFoundation/AVAudioProcessingSettings.h>). Documented values.
    // -----------------------------------------------------------------
    (
        "_AVAudioTimePitchAlgorithmTimeDomain",
        HostConstant::NSString("TimeDomain"),
    ),
    (
        "_AVAudioTimePitchAlgorithmVarispeed",
        HostConstant::NSString("Varispeed"),
    ),
    (
        "_AVAudioTimePitchAlgorithmSpectral",
        HostConstant::NSString("Spectral"),
    ),
    (
        "_AVAudioTimePitchAlgorithmLowQualityZeroLatency",
        HostConstant::NSString("LowQualityZeroLatency"),
    ),
    (
        "_AVCaptureDeviceTypeBuiltInWideAngleCamera",
        HostConstant::NSString("AVCaptureDeviceTypeBuiltInWideAngleCamera"),
    ),
    // -----------------------------------------------------------------
    // CoreMedia / CoreVideo / VideoToolbox format & pixel-buffer keys.
    // -----------------------------------------------------------------
    (
        "_kCMFormatDescriptionExtension_Depth",
        HostConstant::NSString("Depth"),
    ),
    (
        "_kCVPixelBufferPoolMinimumBufferCountKey",
        HostConstant::NSString("MinimumBufferCount"),
    ),
    (
        "_kCVPixelBufferPoolMaximumBufferAgeKey",
        HostConstant::NSString("MaximumBufferAge"),
    ),
    (
        "_kCVPixelBufferPoolAllocationThresholdKey",
        HostConstant::NSString("AllocationThreshold"),
    ),
    (
        "_kVTDecompressionPropertyKey_PixelBufferPool",
        HostConstant::NSString("PixelBufferPool"),
    ),
    (
        "_kVTDecompressionPropertyKey_PixelBufferPoolIsShared",
        HostConstant::NSString("PixelBufferPoolIsShared"),
    ),
    (
        "_kVTDecompressionPropertyKey_OutputPoolRequestedMinimumBufferCount",
        HostConstant::NSString("OutputPoolRequestedMinimumBufferCount"),
    ),
    // -----------------------------------------------------------------
    // CoreText font attribute key (<CoreText/CTFont.h>).
    // -----------------------------------------------------------------
    (
        "_kCTFontPostScriptNameKey",
        HostConstant::NSString("NSCTFontPostScriptNameAttribute"),
    ),
    // -----------------------------------------------------------------
    // CoreGraphics PDF context info keys (<CoreGraphics/CGPDFContext.h>).
    // -----------------------------------------------------------------
    ("_kCGPDFContextTitle", HostConstant::NSString("Title")),
    ("_kCGPDFContextCreator", HostConstant::NSString("Creator")),
    // -----------------------------------------------------------------
    // GameKit error domain (<GameKit/GKSession.h>).
    // -----------------------------------------------------------------
    (
        "_GKSessionErrorDomain",
        HostConstant::NSString("com.apple.gamekit.GKSessionErrorDomain"),
    ),
    // (`_AVPlayerItemTimeJumpedNotification` is defined once in the
    // AVFoundation notification section above.)
    // -----------------------------------------------------------------
    // PassKit payment network identifiers (<PassKit/PKPaymentRequest.h>).
    // Documented values.
    // -----------------------------------------------------------------
    ("_PKPaymentNetworkVisa", HostConstant::NSString("Visa")),
    (
        "_PKPaymentNetworkMasterCard",
        HostConstant::NSString("MasterCard"),
    ),
    ("_PKPaymentNetworkAmex", HostConstant::NSString("Amex")),
    ("_PKPaymentNetworkDiscover", HostConstant::NSString("Discover")),
    // -----------------------------------------------------------------
    // MapKit launch-options keys (<MapKit/MKTypes.h>).
    // -----------------------------------------------------------------
    (
        "_MKLaunchOptionsDirectionsModeKey",
        HostConstant::NSString("MKLaunchOptionsDirectionsModeKey"),
    ),
    (
        "_MKLaunchOptionsDirectionsModeDriving",
        HostConstant::NSString("Driving"),
    ),
    ("_MKMapRectNull", HostConstant::Custom(mk_map_rect_null)),
    // -----------------------------------------------------------------
    // UIKit activity type (<UIKit/UIActivity.h>). Documented value.
    // -----------------------------------------------------------------
    (
        "_UIActivityTypePostToTencentWeibo",
        HostConstant::NSString("com.apple.UIKit.activity.PostToTencentWeibo"),
    ),
    // -----------------------------------------------------------------
    // Notification names referenced by apps but never posted by touchHLE.
    // Registering them as stable NSStrings avoids a NULL relocation crash
    // when the app subscribes via NSNotificationCenter.
    // -----------------------------------------------------------------
    (
        "_NSCurrentLocaleDidChangeNotification",
        HostConstant::NSString("NSCurrentLocaleDidChangeNotification"),
    ),
    (
        "_NSExtensionHostDidBecomeActiveNotification",
        HostConstant::NSString("NSExtensionHostDidBecomeActiveNotification"),
    ),
    (
        "_NSExtensionHostDidEnterBackgroundNotification",
        HostConstant::NSString("NSExtensionHostDidEnterBackgroundNotification"),
    ),
    (
        "_NSExtensionHostWillEnterForegroundNotification",
        HostConstant::NSString("NSExtensionHostWillEnterForegroundNotification"),
    ),
    (
        "_NSExtensionHostWillResignActiveNotification",
        HostConstant::NSString("NSExtensionHostWillResignActiveNotification"),
    ),
    (
        "_UIScreenCapturedDidChangeNotification",
        HostConstant::NSString("UIScreenCapturedDidChangeNotification"),
    ),
    (
        "_UISceneWillConnectNotification",
        HostConstant::NSString("UISceneWillConnectNotification"),
    ),
    (
        "_UISceneDidDisconnectNotification",
        HostConstant::NSString("UISceneDidDisconnectNotification"),
    ),
    // -----------------------------------------------------------------
    // NSMetadata / iCloud ubiquitous-item keys referenced by apps that
    // probe for iCloud document state.
    // -----------------------------------------------------------------
    (
        "_NSMetadataItemPathKey",
        HostConstant::NSString("kMDItemPath"),
    ),
    (
        "_NSMetadataUbiquitousItemDownloadingStatusKey",
        HostConstant::NSString("NSMetadataUbiquitousItemDownloadingStatusKey"),
    ),
    (
        "_NSURLUbiquitousItemDownloadingStatusKey",
        HostConstant::NSString("NSURLUbiquitousItemDownloadingStatusKey"),
    ),
    (
        "_NSURLUbiquitousItemDownloadingStatusCurrent",
        HostConstant::NSString("NSURLUbiquitousItemDownloadingStatusCurrent"),
    ),
    // -----------------------------------------------------------------
    // Empty immutable collection singletons. The Objective-C runtime and
    // some apps reference `__NSArray0__` / `__NSDictionary0__` directly via
    // relocations (e.g. for `@[]` / `@{}` literals); they must resolve to a
    // real, retained, empty NSArray/NSDictionary object.
    // -----------------------------------------------------------------
    ("___NSArray0__", HostConstant::Custom(ns_array0)),
    ("___NSDictionary0__", HostConstant::Custom(ns_dictionary0)),
    // -----------------------------------------------------------------
    // MediaPlayer now-playing-info dictionary keys
    // (<MediaPlayer/MPNowPlayingInfoCenter.h>). Documented values.
    // -----------------------------------------------------------------
    (
        "_MPNowPlayingInfoPropertyElapsedPlaybackTime",
        HostConstant::NSString("MPNowPlayingInfoPropertyElapsedPlaybackTime"),
    ),
    (
        "_MPNowPlayingInfoPropertyPlaybackRate",
        HostConstant::NSString("MPNowPlayingInfoPropertyPlaybackRate"),
    ),
    // (`_kCGImagePropertyExifDictionary` is defined in
    // core_graphics::cg_image's constant list.)
    // -----------------------------------------------------------------
    // AVFoundation metadata key spaces / common keys
    // (<AVFoundation/AVMetadataIdentifiers.h>). Documented values.
    // -----------------------------------------------------------------
    ("_AVMetadataKeySpaceCommon", HostConstant::NSString("comn")),
    ("_AVMetadataCommonKeyTitle", HostConstant::NSString("title")),
    // -----------------------------------------------------------------
    // CoreGraphics extended-range color space names
    // (<CoreGraphics/CGColorSpace.h>).
    // -----------------------------------------------------------------
    (
        "_kCGColorSpaceExtendedSRGB",
        HostConstant::NSString("kCGColorSpaceExtendedSRGB"),
    ),
    (
        "_kCGColorSpaceExtendedLinearSRGB",
        HostConstant::NSString("kCGColorSpaceExtendedLinearSRGB"),
    ),
    // -----------------------------------------------------------------
    // AVCaptureDevice "current value" sentinels
    // (<AVFoundation/AVCaptureDevice.h>).
    // -----------------------------------------------------------------
    (
        "_AVCaptureExposureDurationCurrent",
        HostConstant::Custom(av_capture_exposure_duration_current),
    ),
    (
        "_AVCaptureISOCurrent",
        HostConstant::Custom(av_capture_iso_current),
    ),
    // -----------------------------------------------------------------
    // CoreAnimation CAEmitterLayer emitter shapes & render modes
    // (<QuartzCore/CAEmitterLayer.h>). Documented string values.
    // -----------------------------------------------------------------
    ("_kCAEmitterLayerPoint", HostConstant::NSString("point")),
    ("_kCAEmitterLayerLine", HostConstant::NSString("line")),
    ("_kCAEmitterLayerRectangle", HostConstant::NSString("rectangle")),
    ("_kCAEmitterLayerCuboid", HostConstant::NSString("cuboid")),
    ("_kCAEmitterLayerCircle", HostConstant::NSString("circle")),
    ("_kCAEmitterLayerSphere", HostConstant::NSString("sphere")),
    ("_kCAEmitterLayerSurface", HostConstant::NSString("surface")),
    ("_kCAEmitterLayerUnordered", HostConstant::NSString("unordered")),
    (
        "_kCAEmitterLayerOldestFirst",
        HostConstant::NSString("oldestFirst"),
    ),
    ("_kCAEmitterLayerOldestLast", HostConstant::NSString("oldestLast")),
    (
        "_kCAEmitterLayerBackToFront",
        HostConstant::NSString("backToFront"),
    ),
    ("_kCAEmitterLayerAdditive", HostConstant::NSString("additive")),
    // -----------------------------------------------------------------
    // UIKit table-view section-index search magic string
    // (<UIKit/UITableView.h>). Documented value is "{search}".
    // -----------------------------------------------------------------
    ("_UITableViewIndexSearch", HostConstant::NSString("{search}")),
    // -----------------------------------------------------------------
    // AVFoundation audio-encoder setting key
    // (<AVFoundation/AVAudioSettings.h>).
    // -----------------------------------------------------------------
    (
        "_AVEncoderBitRatePerChannelKey",
        HostConstant::NSString("AVEncoderBitRatePerChannelKey"),
    ),
    // -----------------------------------------------------------------
    // sqlite3 constants.
    // -----------------------------------------------------------------
    (
        "_sqlite3_temp_directory",
        HostConstant::NullPtr,
    ),
];

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/System/Library/Frameworks/Foundation.framework/Foundation",
    aliases: &[],
    class_exports: &[
        _nib_archive_decoder::CLASSES,
        ab_people_picker_navigation_controller::CLASSES,
        asidentifier_manager::CLASSES,
        chipmunk_space::CLASSES,
        ns_array::CLASSES,
        ns_assertion_handler::CLASSES,
        ns_autorelease_pool::CLASSES,
        ns_bundle::CLASSES,
        ns_calendar::CLASSES,
        ns_character_set::CLASSES,
        ns_coder::CLASSES,
        ns_condition::CLASSES,
        ns_data::CLASSES,
        ns_date::CLASSES,
        ns_date_components::CLASSES,
        ns_date_formatter::CLASSES,
        ns_decimal_number::CLASSES,
        ns_dictionary::CLASSES,
        ns_enumerator::CLASSES,
        ns_error::CLASSES,
        ns_exception::CLASSES,
        ns_file_handle::CLASSES,
        ns_file_manager::CLASSES,
        ns_host::CLASSES,
        ns_http_cookie_storage::CLASSES,
        ns_index_path::CLASSES,
        ns_index_set::CLASSES,
        ns_json_serialization::CLASSES,
        ns_input_stream::CLASSES,
        ns_invocation::CLASSES,
        ns_keyed_archiver::CLASSES,
        ns_keyed_unarchiver::CLASSES,
        ns_locale::CLASSES,
        ns_lock::CLASSES,
        ns_metadata_query::CLASSES,
        ns_notification::CLASSES,
        ns_notification_center::CLASSES,
        ns_null::CLASSES,
        ns_number_formatter::CLASSES,
        ns_object::CLASSES,
        ns_operation::CLASSES,
        ns_persistent_store_coordinator::CLASSES,
        ns_predicate::CLASSES,
        ns_process_info::CLASSES,
        ns_property_list_serialization::CLASSES,
        ns_regular_expression::CLASSES,
        ns_run_loop::CLASSES,
        ns_scanner::CLASSES,
        ns_hash_table::CLASSES,
        ns_set::CLASSES,
        ns_sort_descriptor::CLASSES,
        ns_string::CLASSES,
        ns_text_checking_result::CLASSES,
        ns_thread::CLASSES,
        ns_time_zone::CLASSES,
        ns_timer::CLASSES,
        ns_ubiquitous_key_value_store::CLASSES,
        ns_undo_manager::CLASSES,
        ns_url::CLASSES,
        ns_url_connection::CLASSES,
        ns_url_request::CLASSES,
        ns_url_response::CLASSES,
        ns_user_defaults::CLASSES,
        ns_uuid::CLASSES,
        ns_value::CLASSES,
        ns_xml_parser::CLASSES,
    ],
    constant_exports: &[
        ns_calendar::CONSTANTS,
        ns_error::CONSTANTS,
        ns_exception::CONSTANTS,
        ns_file_manager::CONSTANTS,
        ns_keyed_unarchiver::CONSTANTS,
        ns_locale::CONSTANTS,
        ns_persistent_store_coordinator::CONSTANTS,
        ns_run_loop::CONSTANTS,
        ns_url::CONSTANTS,
        STUB_CONSTANTS,
    ],
    function_exports: &[
        FUNCTIONS,
        ns_exception::FUNCTIONS,
        ns_file_manager::FUNCTIONS,
        ns_log::FUNCTIONS,
        ns_objc_runtime::FUNCTIONS,
    ],
};

#[derive(Default)]
pub struct State {
    ns_autorelease_pool: ns_autorelease_pool::State,
    ns_bundle: ns_bundle::State,
    ns_calendar: ns_calendar::State,
    pub ns_exception: ns_exception::State,
    ns_file_manager: ns_file_manager::State,
    ns_http_cookie_storage: ns_http_cookie_storage::State,
    ns_locale: ns_locale::State,
    ns_notification_center: ns_notification_center::State,
    ns_null: ns_null::State,
    ns_process_info: ns_process_info::State,
    ns_run_loop: ns_run_loop::State,
    ns_string: ns_string::State,
    ns_thread: ns_thread::State,
    pub ns_ubiquitous_key_value_store: ns_ubiquitous_key_value_store::State,
    pub ns_undo_manager: ns_undo_manager::State,
    ns_user_defaults: ns_user_defaults::State,
    /// Singleton for [NSURLCache sharedURLCache].
    pub url_cache_singleton: crate::objc::id,
}

pub type NSInteger = i32;

pub type NSUInteger = u32;

// this should be equal to NSIntegerMax
pub const NSNotFound: i32 = 0x7fffffff;

#[derive(Debug)]
#[repr(C, packed)]
pub struct NSRange {
    pub location: NSUInteger,
    pub length: NSUInteger,
}
unsafe impl crate::mem::SafeRead for NSRange {}
crate::abi::impl_GuestRet_for_large_struct!(NSRange);

impl crate::abi::GuestArg for NSRange {
    const REG_COUNT: usize = 2;

    fn from_regs(regs: &[u32]) -> Self {
        NSRange {
            location: crate::abi::GuestArg::from_regs(&regs[0..1]),
            length: crate::abi::GuestArg::from_regs(&regs[1..2]),
        }
    }
    fn to_regs(self, regs: &mut [u32]) {
        self.location.to_regs(&mut regs[0..1]);

        self.length.to_regs(&mut regs[1..2]);
    }
}

fn NSStringFromRange(env: &mut Environment, range: NSRange) -> id {
    let loc = range.location;

    let len = range.length;
    let string = format!("{{{loc}, {len}}}");
    ns_string::from_rust_string(env, string)
}

pub type NSComparisonResult = NSInteger;

pub const NSOrderedAscending: NSComparisonResult = -1;
pub const NSOrderedSame: NSComparisonResult = 0;
pub const NSOrderedDescending: NSComparisonResult = 1;

/// Number of seconds.
pub type NSTimeInterval = f64;

/// Convert an [`NSTimeInterval`] into a [`std::time::Duration`] without ever
/// panicking.
///
/// [`std::time::Duration::from_secs_f64`] panics when handed `NaN`, infinity,
/// negative values, or values beyond `u64::MAX` seconds. Real iPhone OS apps
/// have been observed passing all of these — for example HyperHLE log #4
/// (Crazy Frog Racer) panicked at `time.rs:964:23` after the first
/// `NSTimer` fired because the game scheduled a callback with a non-finite
/// interval, and the Resident Evil 4 / accelerometer paths similarly emit
/// junk floats during scene transitions.
///
/// This helper returns `None` for any value the standard library would
/// reject so that callers can either skip the conversion or fall back to a
/// sensible default (such as `0` for "fire immediately"). Apple's own
/// implementation silently clamps these cases instead of aborting the
/// process; mirroring that behaviour here keeps the emulator alive when
/// guest code is buggy in ways the real OS tolerates.
pub fn ns_time_interval_to_duration(ti: NSTimeInterval) -> Option<std::time::Duration> {
    if !ti.is_finite() || ti < 0.0 {
        return None;
    }
    std::time::Duration::try_from_secs_f64(ti).ok()
}

/// Same as [`ns_time_interval_to_duration`] but returns
/// [`std::time::Duration::ZERO`] (i.e. "fire as soon as possible") when the
/// input would otherwise be rejected. Useful in hot paths where the only
/// reasonable fallback is to treat the bad value as "no delay".
pub fn ns_time_interval_to_duration_or_zero(ti: NSTimeInterval) -> std::time::Duration {
    ns_time_interval_to_duration(ti).unwrap_or(std::time::Duration::ZERO)
}

/// UTF-16 code unit.
#[allow(non_camel_case_types)]
pub type unichar = u16;

/// Utility to help with implementing the `hash` method, which various classes
/// in Foundation have to do.
fn hash_helper<T: std::hash::Hash>(hashable: &T) -> NSUInteger {
    use std::hash::Hasher;

    // Rust documentation says DefaultHasher::new() should always return the
    // same instance, so this should give consistent hashes.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hashable.hash(&mut hasher);

    let hash_u64: u64 = hasher.finish();
    (hash_u64 as u32) ^ ((hash_u64 >> 32) as u32)
}

const FUNCTIONS: FunctionExports = &[
    export_c_func!(NSStringFromRange(_)),
    export_c_func!(NSGetSizeAndAlignment(_, _, _)),
    export_c_func!(CFStringGetCharactersPtr(_)),
];