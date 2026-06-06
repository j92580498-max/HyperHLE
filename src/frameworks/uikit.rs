/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! The UIKit framework.
//!
//! For the time being the focus of this project is on running games, which are
//! likely to use UIKit in very simple and limited ways, so this implementation
//! will probably take a lot of shortcuts.

use crate::{msg, Environment};
use std::time::Instant;

use crate::dyld::HostConstant;
use crate::mem::{ConstVoidPtr, MutPtr};

pub mod ui_accelerometer;
pub mod ui_action_sheet;
pub mod ui_activity;
pub mod ui_activity_indicator_view;
pub mod ui_application;
pub mod ui_color;
pub mod ui_custom_object;
pub mod ui_device;
pub mod ui_document;
pub mod ui_event;
pub mod ui_font;
pub mod ui_geometry;
pub mod ui_gesture_recognizer;
pub mod ui_graphics;
pub mod ui_image;
pub mod ui_image_picker_controller;
pub mod ui_keyboard;
pub mod ui_layout_placeholders;
pub mod ui_local_notification;
pub mod ui_navigation_bar;
pub mod ui_nib;
pub mod ui_pasteboard;
pub mod ui_pinch_gesture_recognizer;
pub mod ui_popover_controller;
pub mod ui_responder;
pub mod ui_rotation_gesture_recognizer;
pub mod ui_screen;
pub mod ui_screen_mode;
pub mod ui_search_bar;
pub mod ui_split_view_controller;
pub mod ui_storyboard;
pub mod ui_tab_bar_controller;
pub mod ui_tab_bar_item;
pub mod ui_touch;
pub mod ui_view;
pub mod ui_view_controller;

fn ui_background_task_invalid(env: &mut Environment) -> ConstVoidPtr {
    // UIBackgroundTaskInvalid == NSUIntegerMax == 0xFFFF_FFFF
    let ptr: MutPtr<u32> = env.mem.alloc(4).cast();
    env.mem.write(ptr, 0xFFFF_FFFFu32);
    ptr.cast().cast_const()
}

// UIWindowLevel is a CGFloat (= f32 on 32-bit iOS).
// Standard values from UIWindow.h:
//   UIWindowLevelNormal    =    0.0
//   UIWindowLevelStatusBar = 1000.0
//   UIWindowLevelAlert     = 2000.0

fn ui_window_level_normal(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u32> = env.mem.alloc(4).cast();
    env.mem.write(ptr, 0.0f32.to_bits());
    ptr.cast().cast_const()
}

fn ui_window_level_status_bar(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u32> = env.mem.alloc(4).cast();
    env.mem.write(ptr, 1000.0f32.to_bits());
    ptr.cast().cast_const()
}

fn ui_window_level_alert(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u32> = env.mem.alloc(4).cast();
    env.mem.write(ptr, 2000.0f32.to_bits());
    ptr.cast().cast_const()
}

/// UIScrollViewDecelerationRateNormal = 0.998 (CGFloat)
/// https://developer.apple.com/documentation/uikit/uiscrollview/1619438-decelerationratenormal
fn ui_scroll_view_deceleration_rate_normal(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u32> = env.mem.alloc(4).cast();
    env.mem.write(ptr, 0.998f32.to_bits());
    ptr.cast().cast_const()
}

/// UIScrollViewDecelerationRateFast = 0.99 (CGFloat)
/// https://developer.apple.com/documentation/uikit/uiscrollview/1619438-decelerationratefast
fn ui_scroll_view_deceleration_rate_fast(env: &mut Environment) -> ConstVoidPtr {
    let ptr: MutPtr<u32> = env.mem.alloc(4).cast();
    env.mem.write(ptr, 0.99f32.to_bits());
    ptr.cast().cast_const()
}

// UIFontWeight* — CGFloat constants on the Core Text font-weight axis.
// Apple `UIFont.h` declares them as
// `UIKIT_EXTERN const UIFontWeight UIFontWeightUltraLight ...`;
// the numeric values match the public CTFontDescriptor weight axis
// (range -1.0…1.0, with `Regular` == 0.0). See
// <https://developer.apple.com/documentation/uikit/uifontweight>.
fn write_cgfloat(env: &mut Environment, value: f32) -> ConstVoidPtr {
    let ptr: MutPtr<u32> = env.mem.alloc(4).cast();
    env.mem.write(ptr, value.to_bits());
    ptr.cast().cast_const()
}

fn ui_font_weight_ultralight(env: &mut Environment) -> ConstVoidPtr {
    write_cgfloat(env, -0.8)
}

/// `UITableViewAutomaticDimension` — sentinel value used for automatic
/// row/section heights in self-sizing table views (iOS 5+).
///
/// Value is -1.0 (a `CGFloat`), matching the constant defined in
/// Apple `UITableView.h`:
/// <https://developer.apple.com/documentation/uikit/uitableview/1614961-automaticDimension>
fn ui_table_view_automatic_dimension(env: &mut Environment) -> ConstVoidPtr {
    write_cgfloat(env, -1.0)
}
fn ui_font_weight_thin(env: &mut Environment) -> ConstVoidPtr {
    write_cgfloat(env, -0.6)
}
fn ui_font_weight_light(env: &mut Environment) -> ConstVoidPtr {
    write_cgfloat(env, -0.4)
}
fn ui_font_weight_regular(env: &mut Environment) -> ConstVoidPtr {
    write_cgfloat(env, 0.0)
}
fn ui_font_weight_medium(env: &mut Environment) -> ConstVoidPtr {
    write_cgfloat(env, 0.23)
}
fn ui_font_weight_semibold(env: &mut Environment) -> ConstVoidPtr {
    write_cgfloat(env, 0.3)
}
fn ui_font_weight_bold(env: &mut Environment) -> ConstVoidPtr {
    write_cgfloat(env, 0.4)
}
fn ui_font_weight_heavy(env: &mut Environment) -> ConstVoidPtr {
    write_cgfloat(env, 0.56)
}
fn ui_font_weight_black(env: &mut Environment) -> ConstVoidPtr {
    write_cgfloat(env, 0.62)
}

// `UIAccessibilityTraits` is `uint64_t` (declared in Apple's
// `UIAccessibility.h`). The dyld slot for each trait constant must hold a
// pointer to an `8`-byte value the guest can dereference; using an `NSString`
// would put the wrong bit pattern there and bitwise-OR'ing traits would yield
// garbage.
fn write_uiaccessibility_trait(env: &mut Environment, value: u64) -> ConstVoidPtr {
    let ptr: MutPtr<u64> = env.mem.alloc(8).cast();
    env.mem.write(ptr, value);
    ptr.cast().cast_const()
}

// Bit positions per Apple's public `UIAccessibility.h` header
// (`UIAccessibilityTraitButton` is `(1 << 0)`, etc.).
fn uia_trait_none(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 0)
}
fn uia_trait_button(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 0)
}
fn uia_trait_link(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 1)
}
fn uia_trait_search_field(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 2)
}
fn uia_trait_image(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 3)
}
fn uia_trait_selected(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 4)
}
fn uia_trait_plays_sound(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 5)
}
fn uia_trait_keyboard_key(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 6)
}
fn uia_trait_static_text(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 7)
}
fn uia_trait_summary_element(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 8)
}
fn uia_trait_not_enabled(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 9)
}
fn uia_trait_updates_frequently(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 10)
}
fn uia_trait_starts_media_session(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 11)
}
fn uia_trait_adjustable(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 12)
}
fn uia_trait_allows_direct_interaction(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 13)
}
fn uia_trait_causes_page_turn(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 14)
}
fn uia_trait_header(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 15)
}
fn uia_trait_tab_bar(env: &mut Environment) -> ConstVoidPtr {
    write_uiaccessibility_trait(env, 1 << 18)
}

pub const CONSTANTS: &[(&str, HostConstant)] = &[
    (
        "_UIBackgroundTaskInvalid",
        HostConstant::Custom(ui_background_task_invalid),
    ),
    (
        "_UIImagePickerControllerOriginalImage",
        HostConstant::NSString("UIImagePickerControllerOriginalImage"),
    ),
    (
        "_UIImagePickerControllerEditedImage",
        HostConstant::NSString("UIImagePickerControllerEditedImage"),
    ),
    (
        "_UIImagePickerControllerCropRect",
        HostConstant::NSString("UIImagePickerControllerCropRect"),
    ),
    (
        "_UIImagePickerControllerMediaType",
        HostConstant::NSString("UIImagePickerControllerMediaType"),
    ),
    (
        "_UIImagePickerControllerMediaURL",
        HostConstant::NSString("UIImagePickerControllerMediaURL"),
    ),
    (
        "_UIImagePickerControllerReferenceURL",
        HostConstant::NSString("UIImagePickerControllerReferenceURL"),
    ),
    (
        "_UIScreenDidConnectNotification",
        HostConstant::NSString("UIScreenDidConnectNotification"),
    ),
    // -----------------------------------------------------------------
    // UIPasteboard well-known pasteboard names.
    // -----------------------------------------------------------------
    (
        "_UIPasteboardNameGeneral",
        HostConstant::NSString("UIPasteboardNameGeneral"),
    ),
    (
        "_UIPasteboardNameFind",
        HostConstant::NSString("UIPasteboardNameFind"),
    ),
    (
        "_UIPasteboardChangedNotification",
        HostConstant::NSString("UIPasteboardChangedNotification"),
    ),
    (
        "_UIPasteboardRemovedNotification",
        HostConstant::NSString("UIPasteboardRemovedNotification"),
    ),
    (
        "_UIPasteboardChangedTypesAddedKey",
        HostConstant::NSString("UIPasteboardChangedTypesAddedKey"),
    ),
    (
        "_UIPasteboardChangedTypesRemovedKey",
        HostConstant::NSString("UIPasteboardChangedTypesRemovedKey"),
    ),
    // UIPasteboard type list constants (NSArray of UTI strings).
    // On real iOS these are NSArray singletons; here we export them as
    // NSString constants since the dyld linker only needs a non-NULL
    // address and the app typically uses them for identity comparison.
    (
        "_UIPasteboardTypeListString",
        HostConstant::NSString("public.utf8-plain-text"),
    ),
    (
        "_UIPasteboardTypeListURL",
        HostConstant::NSString("public.url"),
    ),
    (
        "_UIPasteboardTypeListImage",
        HostConstant::NSString("public.image"),
    ),
    (
        "_UIPasteboardTypeListColor",
        HostConstant::NSString("com.apple.uikit.color"),
    ),
    // -----------------------------------------------------------------
    // UITextView change notification.
    // -----------------------------------------------------------------
    (
        "_UITextViewTextDidChangeNotification",
        HostConstant::NSString("UITextViewTextDidChangeNotification"),
    ),
    (
        "_UITextViewTextDidBeginEditingNotification",
        HostConstant::NSString("UITextViewTextDidBeginEditingNotification"),
    ),
    (
        "_UITextViewTextDidEndEditingNotification",
        HostConstant::NSString("UITextViewTextDidEndEditingNotification"),
    ),
    // UIWindowLevel constants (CGFloat / f32 on 32-bit iOS)
    (
        "_UIWindowLevelNormal",
        HostConstant::Custom(ui_window_level_normal),
    ),
    (
        "_UIWindowLevelStatusBar",
        HostConstant::Custom(ui_window_level_status_bar),
    ),
    (
        "_UIWindowLevelAlert",
        HostConstant::Custom(ui_window_level_alert),
    ),
    // UIViewController transition coordinator context keys (iOS 5+).
    (
        "_UITransitionContextFromViewControllerKey",
        HostConstant::NSString("UITransitionContextFromViewController"),
    ),
    (
        "_UITransitionContextToViewControllerKey",
        HostConstant::NSString("UITransitionContextToViewController"),
    ),
    (
        "_UITransitionContextFromViewKey",
        HostConstant::NSString("UITransitionContextFromView"),
    ),
    (
        "_UITransitionContextToViewKey",
        HostConstant::NSString("UITransitionContextToView"),
    ),
    // UIKit text-attribute keys (iOS 5–6 era; deprecated in iOS 7 in favour
    // of NSAttributedString attribute names but still present in apps that
    // target iOS 5). Apple `UIStringDrawing.h` declares them as
    // `UIKIT_EXTERN NSString * const`. Apps reach them through Mach-O
    // symbol lookup or via `[bar setTitleTextAttributes:@{
    //   UITextAttributeFont: ...,  UITextAttributeTextColor: ... }]`.
    (
        "_UITextAttributeFont",
        HostConstant::NSString("UITextAttributeFont"),
    ),
    (
        "_UITextAttributeTextColor",
        HostConstant::NSString("UITextAttributeTextColor"),
    ),
    (
        "_UITextAttributeTextShadowColor",
        HostConstant::NSString("UITextAttributeTextShadowColor"),
    ),
    (
        "_UITextAttributeTextShadowOffset",
        HostConstant::NSString("UITextAttributeTextShadowOffset"),
    ),
    // UIScrollView deceleration rate constants (CGFloat).
    // https://developer.apple.com/documentation/uikit/uiscrollview/decelerationrate
    (
        "_UIScrollViewDecelerationRateNormal",
        HostConstant::Custom(ui_scroll_view_deceleration_rate_normal),
    ),
    (
        "_UIScrollViewDecelerationRateFast",
        HostConstant::Custom(ui_scroll_view_deceleration_rate_fast),
    ),
    // -----------------------------------------------------------------
    // UIScreen disconnect notification (paired with the existing
    // UIScreenDidConnectNotification above), per
    // <https://developer.apple.com/documentation/uikit/uiscreen/1617835-disconnect>.
    // -----------------------------------------------------------------
    (
        "_UIScreenDidDisconnectNotification",
        HostConstant::NSString("UIScreenDidDisconnectNotification"),
    ),
    (
        "_UIScreenModeDidChangeNotification",
        HostConstant::NSString("UIScreenModeDidChangeNotification"),
    ),
    (
        "_UIApplicationBackgroundRefreshStatusDidChangeNotification",
        HostConstant::NSString("UIApplicationBackgroundRefreshStatusDidChangeNotification"),
    ),
    (
        "_UIApplicationUserDidTakeScreenshotNotification",
        HostConstant::NSString("UIApplicationUserDidTakeScreenshotNotification"),
    ),
    // -----------------------------------------------------------------
    // UIAccessibility traits (iOS 3.0+). Declared as
    // `UIKIT_EXTERN const UIAccessibilityTraits UIAccessibilityTrait*` in
    // Apple's `UIAccessibility.h`, where `UIAccessibilityTraits` is
    // `uint64_t`. Bit positions match the header so guests can OR them
    // together.
    // <https://developer.apple.com/documentation/uikit/uiaccessibilitytraits>
    // -----------------------------------------------------------------
    (
        "_UIAccessibilityTraitNone",
        HostConstant::Custom(uia_trait_none),
    ),
    (
        "_UIAccessibilityTraitButton",
        HostConstant::Custom(uia_trait_button),
    ),
    (
        "_UIAccessibilityTraitLink",
        HostConstant::Custom(uia_trait_link),
    ),
    (
        "_UIAccessibilityTraitSearchField",
        HostConstant::Custom(uia_trait_search_field),
    ),
    (
        "_UIAccessibilityTraitImage",
        HostConstant::Custom(uia_trait_image),
    ),
    (
        "_UIAccessibilityTraitSelected",
        HostConstant::Custom(uia_trait_selected),
    ),
    (
        "_UIAccessibilityTraitPlaysSound",
        HostConstant::Custom(uia_trait_plays_sound),
    ),
    (
        "_UIAccessibilityTraitKeyboardKey",
        HostConstant::Custom(uia_trait_keyboard_key),
    ),
    (
        "_UIAccessibilityTraitStaticText",
        HostConstant::Custom(uia_trait_static_text),
    ),
    (
        "_UIAccessibilityTraitSummaryElement",
        HostConstant::Custom(uia_trait_summary_element),
    ),
    (
        "_UIAccessibilityTraitNotEnabled",
        HostConstant::Custom(uia_trait_not_enabled),
    ),
    (
        "_UIAccessibilityTraitUpdatesFrequently",
        HostConstant::Custom(uia_trait_updates_frequently),
    ),
    (
        "_UIAccessibilityTraitStartsMediaSession",
        HostConstant::Custom(uia_trait_starts_media_session),
    ),
    (
        "_UIAccessibilityTraitAdjustable",
        HostConstant::Custom(uia_trait_adjustable),
    ),
    (
        "_UIAccessibilityTraitAllowsDirectInteraction",
        HostConstant::Custom(uia_trait_allows_direct_interaction),
    ),
    (
        "_UIAccessibilityTraitCausesPageTurn",
        HostConstant::Custom(uia_trait_causes_page_turn),
    ),
    (
        "_UIAccessibilityTraitHeader",
        HostConstant::Custom(uia_trait_header),
    ),
    (
        "_UIAccessibilityTraitTabBar",
        HostConstant::Custom(uia_trait_tab_bar),
    ),
    // -----------------------------------------------------------------
    // UIFontTextStyle (iOS 7+). Declared as
    // `UIKIT_EXTERN UIFontTextStyle const UIFontTextStyle*` in Apple's
    // `UIFontDescriptor.h`, where `UIFontTextStyle` is
    // `NSString * NS_TYPED_ENUM`. The string values are passed back to
    // `+[UIFont preferredFontForTextStyle:]` to obtain a dynamic-type
    // appropriate font.
    // <https://developer.apple.com/documentation/uikit/uifonttextstyle>
    // -----------------------------------------------------------------
    (
        "_UIFontTextStyleLargeTitle",
        HostConstant::NSString("UICTFontTextStyleTitle0"),
    ),
    (
        "_UIFontTextStyleTitle1",
        HostConstant::NSString("UICTFontTextStyleTitle1"),
    ),
    (
        "_UIFontTextStyleTitle2",
        HostConstant::NSString("UICTFontTextStyleTitle2"),
    ),
    (
        "_UIFontTextStyleTitle3",
        HostConstant::NSString("UICTFontTextStyleTitle3"),
    ),
    (
        "_UIFontTextStyleHeadline",
        HostConstant::NSString("UICTFontTextStyleHeadline"),
    ),
    (
        "_UIFontTextStyleSubheadline",
        HostConstant::NSString("UICTFontTextStyleSubhead"),
    ),
    (
        "_UIFontTextStyleBody",
        HostConstant::NSString("UICTFontTextStyleBody"),
    ),
    (
        "_UIFontTextStyleCallout",
        HostConstant::NSString("UICTFontTextStyleCallout"),
    ),
    (
        "_UIFontTextStyleFootnote",
        HostConstant::NSString("UICTFontTextStyleFootnote"),
    ),
    (
        "_UIFontTextStyleCaption1",
        HostConstant::NSString("UICTFontTextStyleCaption1"),
    ),
    (
        "_UIFontTextStyleCaption2",
        HostConstant::NSString("UICTFontTextStyleCaption2"),
    ),
    // -----------------------------------------------------------------
    // UIActivityType identifiers (iOS 6+, NSString constants),
    // <https://developer.apple.com/documentation/uikit/uiactivitytype>.
    // -----------------------------------------------------------------
    (
        "_UIActivityTypePostToFacebook",
        HostConstant::NSString("com.apple.UIKit.activity.PostToFacebook"),
    ),
    (
        "_UIActivityTypePostToTwitter",
        HostConstant::NSString("com.apple.UIKit.activity.PostToTwitter"),
    ),
    (
        "_UIActivityTypePostToWeibo",
        HostConstant::NSString("com.apple.UIKit.activity.PostToWeibo"),
    ),
    (
        "_UIActivityTypeMessage",
        HostConstant::NSString("com.apple.UIKit.activity.Message"),
    ),
    (
        "_UIActivityTypeMail",
        HostConstant::NSString("com.apple.UIKit.activity.Mail"),
    ),
    (
        "_UIActivityTypePrint",
        HostConstant::NSString("com.apple.UIKit.activity.Print"),
    ),
    (
        "_UIActivityTypeCopyToPasteboard",
        HostConstant::NSString("com.apple.UIKit.activity.CopyToPasteboard"),
    ),
    (
        "_UIActivityTypeAssignToContact",
        HostConstant::NSString("com.apple.UIKit.activity.AssignToContact"),
    ),
    (
        "_UIActivityTypeSaveToCameraRoll",
        HostConstant::NSString("com.apple.UIKit.activity.SaveToCameraRoll"),
    ),
    (
        "_UIActivityTypeAddToReadingList",
        HostConstant::NSString("com.apple.UIKit.activity.AddToReadingList"),
    ),
    // iOS 7+
    // <https://developer.apple.com/documentation/uikit/uiactivitytype/1620521-airdrop>
    (
        "_UIActivityTypeAirDrop",
        HostConstant::NSString("com.apple.UIKit.activity.AirDropActivityType"),
    ),
    // <https://developer.apple.com/documentation/uikit/uiactivitytype/1620522-openinbooks>
    (
        "_UIActivityTypeOpenInIBooks",
        HostConstant::NSString("com.apple.UIKit.activity.OpenInIBooks"),
    ),
    // iOS 7+
    // <https://developer.apple.com/documentation/uikit/uiactivitytype>
    (
        "_UIActivityTypePostToFlickr",
        HostConstant::NSString("com.apple.UIKit.activity.PostToFlickr"),
    ),
    (
        "_UIActivityTypePostToVimeo",
        HostConstant::NSString("com.apple.UIKit.activity.PostToVimeo"),
    ),
    // -----------------------------------------------------------------
    // UICollectionView supplementary view kinds (NSString constants),
    // <https://developer.apple.com/documentation/uikit/uicollectionview>.
    // -----------------------------------------------------------------
    (
        "_UICollectionElementKindSectionHeader",
        HostConstant::NSString("UICollectionElementKindSectionHeader"),
    ),
    (
        "_UICollectionElementKindSectionFooter",
        HostConstant::NSString("UICollectionElementKindSectionFooter"),
    ),
    // -----------------------------------------------------------------
    // UIDocument state-change notification (iOS 5+),
    // <https://developer.apple.com/documentation/uikit/uidocument>.
    // -----------------------------------------------------------------
    (
        "_UIDocumentStateChangedNotification",
        HostConstant::NSString("UIDocumentStateChangedNotification"),
    ),
    // -----------------------------------------------------------------
    // UIMenuController show / hide notification names (iOS 3.2+). Apple
    // `UIMenuController.h` declares them as
    // `UIKIT_EXTERN NSNotificationName const ...`; the posted value is
    // each constant's own symbol name. See
    // <https://developer.apple.com/documentation/uikit/uimenucontroller>.
    // -----------------------------------------------------------------
    (
        "_UIMenuControllerWillShowMenuNotification",
        HostConstant::NSString("UIMenuControllerWillShowMenuNotification"),
    ),
    (
        "_UIMenuControllerDidShowMenuNotification",
        HostConstant::NSString("UIMenuControllerDidShowMenuNotification"),
    ),
    (
        "_UIMenuControllerWillHideMenuNotification",
        HostConstant::NSString("UIMenuControllerWillHideMenuNotification"),
    ),
    (
        "_UIMenuControllerDidHideMenuNotification",
        HostConstant::NSString("UIMenuControllerDidHideMenuNotification"),
    ),
    (
        "_UIMenuControllerMenuFrameDidChangeNotification",
        HostConstant::NSString("UIMenuControllerMenuFrameDidChangeNotification"),
    ),
    // -----------------------------------------------------------------
    // UITableView selection notification, Apple `UITableView.h`. The
    // notification name is the constant's symbol name verbatim. See
    // <https://developer.apple.com/documentation/uikit/uitableviewselectiondidchangenotification>.
    // -----------------------------------------------------------------
    (
        "_UITableViewSelectionDidChangeNotification",
        HostConstant::NSString("UITableViewSelectionDidChangeNotification"),
    ),
    // UITableViewAutomaticDimension (CGFloat = -1.0) — self-sizing rows/headers.
    // Apple `UITableView.h`, iOS 5.0+.
    // <https://developer.apple.com/documentation/uikit/uitableview/1614961-automaticDimension>
    (
        "_UITableViewAutomaticDimension",
        HostConstant::Custom(ui_table_view_automatic_dimension),
    ),
    // -----------------------------------------------------------------
    // UIContentSizeCategory (iOS 7+ Dynamic Type). Apple `UIApplication.h`
    // declares each as `UIKIT_EXTERN NSString * const`; the literal
    // value matches the constant's name. Apps compare against them with
    // `isEqualToString:` and use them as keys / userInfo values for
    // `UIContentSizeCategoryDidChangeNotification`.
    // <https://developer.apple.com/documentation/uikit/uicontentsizecategory>
    // -----------------------------------------------------------------
    (
        "_UIContentSizeCategoryUnspecified",
        HostConstant::NSString("_UICTContentSizeCategoryUnspecified"),
    ),
    (
        "_UIContentSizeCategoryExtraSmall",
        HostConstant::NSString("UICTContentSizeCategoryXS"),
    ),
    (
        "_UIContentSizeCategorySmall",
        HostConstant::NSString("UICTContentSizeCategoryS"),
    ),
    (
        "_UIContentSizeCategoryMedium",
        HostConstant::NSString("UICTContentSizeCategoryM"),
    ),
    (
        "_UIContentSizeCategoryLarge",
        HostConstant::NSString("UICTContentSizeCategoryL"),
    ),
    (
        "_UIContentSizeCategoryExtraLarge",
        HostConstant::NSString("UICTContentSizeCategoryXL"),
    ),
    (
        "_UIContentSizeCategoryExtraExtraLarge",
        HostConstant::NSString("UICTContentSizeCategoryXXL"),
    ),
    (
        "_UIContentSizeCategoryExtraExtraExtraLarge",
        HostConstant::NSString("UICTContentSizeCategoryXXXL"),
    ),
    (
        "_UIContentSizeCategoryAccessibilityMedium",
        HostConstant::NSString("UICTContentSizeCategoryAccessibilityM"),
    ),
    (
        "_UIContentSizeCategoryAccessibilityLarge",
        HostConstant::NSString("UICTContentSizeCategoryAccessibilityL"),
    ),
    (
        "_UIContentSizeCategoryAccessibilityExtraLarge",
        HostConstant::NSString("UICTContentSizeCategoryAccessibilityXL"),
    ),
    (
        "_UIContentSizeCategoryAccessibilityExtraExtraLarge",
        HostConstant::NSString("UICTContentSizeCategoryAccessibilityXXL"),
    ),
    (
        "_UIContentSizeCategoryAccessibilityExtraExtraExtraLarge",
        HostConstant::NSString("UICTContentSizeCategoryAccessibilityXXXL"),
    ),
    (
        "_UIContentSizeCategoryDidChangeNotification",
        HostConstant::NSString("UIContentSizeCategoryDidChangeNotification"),
    ),
    (
        "_UIContentSizeCategoryNewValueKey",
        HostConstant::NSString("UIContentSizeCategoryNewValueKey"),
    ),
    // -----------------------------------------------------------------
    // UIFontDescriptor attribute dictionary keys (iOS 7+). Apple
    // `UIFontDescriptor.h` declares them as `UIKIT_EXTERN NSString *
    // const`. Used as dictionary keys in
    // `[UIFontDescriptor fontDescriptorWithFontAttributes:]`.
    // <https://developer.apple.com/documentation/uikit/uifontdescriptor>
    // -----------------------------------------------------------------
    (
        "_UIFontDescriptorFamilyAttribute",
        HostConstant::NSString("NSFontFamilyAttribute"),
    ),
    (
        "_UIFontDescriptorNameAttribute",
        HostConstant::NSString("NSFontNameAttribute"),
    ),
    (
        "_UIFontDescriptorFaceAttribute",
        HostConstant::NSString("NSFontFaceAttribute"),
    ),
    (
        "_UIFontDescriptorSizeAttribute",
        HostConstant::NSString("NSFontSizeAttribute"),
    ),
    (
        "_UIFontDescriptorVisibleNameAttribute",
        HostConstant::NSString("NSFontVisibleNameAttribute"),
    ),
    (
        "_UIFontDescriptorMatrixAttribute",
        HostConstant::NSString("NSFontMatrixAttribute"),
    ),
    (
        "_UIFontDescriptorCharacterSetAttribute",
        HostConstant::NSString("NSCTFontCharacterSetAttribute"),
    ),
    (
        "_UIFontDescriptorCascadeListAttribute",
        HostConstant::NSString("NSCTFontCascadeListAttribute"),
    ),
    (
        "_UIFontDescriptorTraitsAttribute",
        HostConstant::NSString("NSCTFontTraitsAttribute"),
    ),
    (
        "_UIFontDescriptorFixedAdvanceAttribute",
        HostConstant::NSString("NSCTFontFixedAdvanceAttribute"),
    ),
    (
        "_UIFontDescriptorFeatureSettingsAttribute",
        HostConstant::NSString("NSCTFontFeatureSettingsAttribute"),
    ),
    (
        "_UIFontDescriptorTextStyleAttribute",
        HostConstant::NSString("NSCTFontUIUsageAttribute"),
    ),
    (
        "_UIFontSymbolicTrait",
        HostConstant::NSString("NSCTFontSymbolicTrait"),
    ),
    (
        "_UIFontWeightTrait",
        HostConstant::NSString("NSCTFontWeightTrait"),
    ),
    (
        "_UIFontWidthTrait",
        HostConstant::NSString("NSCTFontWidthTrait"),
    ),
    (
        "_UIFontSlantTrait",
        HostConstant::NSString("NSCTFontSlantTrait"),
    ),
    // UIFontWeight* constants (CGFloat). Apple `UIFont.h` declares them
    // as `UIKIT_EXTERN const UIFontWeight`. The numeric values are
    // the canonical Core Text font-weight axis values; apps thread
    // them through `+[UIFont systemFontOfSize:weight:]`.
    // <https://developer.apple.com/documentation/uikit/uifontweight>
    (
        "_UIFontWeightUltraLight",
        HostConstant::Custom(ui_font_weight_ultralight),
    ),
    (
        "_UIFontWeightThin",
        HostConstant::Custom(ui_font_weight_thin),
    ),
    (
        "_UIFontWeightLight",
        HostConstant::Custom(ui_font_weight_light),
    ),
    (
        "_UIFontWeightRegular",
        HostConstant::Custom(ui_font_weight_regular),
    ),
    (
        "_UIFontWeightMedium",
        HostConstant::Custom(ui_font_weight_medium),
    ),
    (
        "_UIFontWeightSemibold",
        HostConstant::Custom(ui_font_weight_semibold),
    ),
    (
        "_UIFontWeightBold",
        HostConstant::Custom(ui_font_weight_bold),
    ),
    (
        "_UIFontWeightHeavy",
        HostConstant::Custom(ui_font_weight_heavy),
    ),
    (
        "_UIFontWeightBlack",
        HostConstant::Custom(ui_font_weight_black),
    ),
    // -----------------------------------------------------------------
    // UITrackingRunLoopMode — the run-loop mode entered while tracking
    // a `UIControl` interaction (button, slider, …). Apple
    // `UIApplication.h` declares it as
    // `UIKIT_EXTERN NSRunLoopMode const`. Apps pass it to
    // `+[NSRunLoop runMode:beforeDate:]` to keep their UI responsive
    // during touch tracking. The literal value matches Apple's headers.
    // <https://developer.apple.com/documentation/uikit/uitrackingrunloopmode>
    // -----------------------------------------------------------------
    (
        "_UITrackingRunLoopMode",
        HostConstant::NSString("UITrackingRunLoopMode"),
    ),
    // -----------------------------------------------------------------
    // UIApplication launch-options dictionary keys (continued, iOS 8+).
    // <https://developer.apple.com/documentation/uikit/uiapplication/launchoptionskey>
    // -----------------------------------------------------------------
    (
        "_UIApplicationLaunchOptionsUserActivityDictionaryKey",
        HostConstant::NSString("UIApplicationLaunchOptionsUserActivityDictionary"),
    ),
    (
        "_UIApplicationLaunchOptionsUserActivityTypeKey",
        HostConstant::NSString("UIApplicationLaunchOptionsUserActivityType"),
    ),
    // -----------------------------------------------------------------
    // CoreSpotlight constants (iOS 9+).
    //
    // CSSearchableItemActionType is the NSUserActivity activityType
    // string for items opened via Spotlight search. Apps match this in
    // `application(_:continue:restorationHandler:)` to detect Spotlight
    // handoffs.
    // <https://developer.apple.com/documentation/corespotlight/cssearchableitemactiontype>
    (
        "_CSSearchableItemActionType",
        HostConstant::NSString("com.apple.corespotlight.search-action"),
    ),
    // CSSearchableItemActivityIdentifier is the key in the
    // NSUserActivity userInfo dictionary whose value is the
    // CSSearchableItem uniqueIdentifier string.
    // <https://developer.apple.com/documentation/corespotlight/cssearchableitemactivityidentifier>
    (
        "_CSSearchableItemActivityIdentifier",
        HostConstant::NSString("kCSSearchableItemActivityIdentifier"),
    ),
    (
        "_UIApplicationLaunchOptionsCloudKitShareMetadataKey",
        HostConstant::NSString("UIApplicationLaunchOptionsCloudKitShareMetadataKey"),
    ),
];

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/System/Library/Frameworks/UIKit.framework/UIKit",
    aliases: &[],
    class_exports: &[
        ui_accelerometer::CLASSES,
        ui_action_sheet::CLASSES,
        ui_activity_indicator_view::CLASSES,
        ui_activity::CLASSES,
        ui_application::CLASSES,
        ui_color::CLASSES,
        ui_custom_object::CLASSES,
        ui_device::CLASSES,
        ui_document::CLASSES,
        ui_event::CLASSES,
        ui_font::CLASSES,
        ui_gesture_recognizer::CLASSES,
        ui_image::CLASSES,
        ui_image_picker_controller::CLASSES,
        ui_keyboard::CLASSES,
        ui_local_notification::CLASSES,
        ui_navigation_bar::CLASSES,
        ui_nib::CLASSES,
        ui_pasteboard::CLASSES,
        ui_pinch_gesture_recognizer::CLASSES,
        ui_popover_controller::CLASSES,
        ui_rotation_gesture_recognizer::CLASSES,
        ui_responder::CLASSES,
        ui_screen_mode::CLASSES,
        ui_screen::CLASSES,
        ui_search_bar::CLASSES,
        ui_split_view_controller::CLASSES,
        ui_storyboard::CLASSES,
        ui_layout_placeholders::CLASSES,
        ui_tab_bar_item::CLASSES,
        ui_tab_bar_controller::CLASSES,
        ui_touch::CLASSES,
        ui_view::CLASSES,
        ui_view::ui_alert_view::CLASSES,
        ui_view::ui_collection_view::CLASSES,
        ui_view::ui_control::CLASSES,
        ui_view::ui_control::ui_bar_button_item::CLASSES,
        ui_view::ui_control::ui_button::CLASSES,
        ui_view::ui_control::ui_segmented_control::CLASSES,
        ui_view::ui_control::ui_slider::CLASSES,
        ui_view::ui_control::ui_text_field::CLASSES,
        ui_view::ui_control::ui_switch::CLASSES,
        ui_view::ui_image_view::CLASSES,
        ui_view::ui_label::CLASSES,
        ui_view::ui_page_control::CLASSES,
        ui_view::ui_picker_view::CLASSES,
        ui_view::ui_scroll_view::CLASSES,
        ui_view::ui_scroll_view::ui_text_view::CLASSES,
        ui_view::ui_table_view::CLASSES,
        ui_view::ui_text_selection_view::CLASSES,
        ui_view::ui_toolbar::CLASSES,
        ui_view::ui_web_view::CLASSES,
        ui_view::ui_window::CLASSES,
        ui_view_controller::CLASSES,
        ui_view_controller::ui_navigation_controller::CLASSES,
    ],
    constant_exports: &[
        ui_application::CONSTANTS,
        ui_device::CONSTANTS,
        ui_geometry::CONSTANTS,
        ui_keyboard::CONSTANTS,
        ui_local_notification::CONSTANTS,
        ui_nib::CONSTANTS,
        ui_view::ui_control::ui_text_field::CONSTANTS,
        ui_view::ui_window::CONSTANTS,
        CONSTANTS,
    ],
    function_exports: &[
        ui_application::FUNCTIONS,
        ui_geometry::FUNCTIONS,
        ui_graphics::FUNCTIONS,
        ui_image::FUNCTIONS,
        ui_image_picker_controller::FUNCTIONS,
    ],
};

#[derive(Default)]
pub struct State {
    ui_accelerometer: ui_accelerometer::State,
    ui_application: ui_application::State,
    ui_color: ui_color::State,
    ui_device: ui_device::State,
    ui_font: ui_font::State,
    ui_geometry: ui_geometry::State,
    ui_graphics: ui_graphics::State,
    ui_image: ui_image::State,
    ui_screen: ui_screen::State,
    ui_touch: ui_touch::State,
    pub ui_view: ui_view::State,
    ui_responder: ui_responder::State,
}

/// For use by `NSRunLoop`: handles any events that have queued up.
///
/// Returns the next time this function must be called, if any, e.g. the next
/// time an accelerometer input is due.
pub fn handle_events(env: &mut Environment) -> Option<Instant> {
    use crate::window::Event;
    use crate::window::TextInputEvent;

    // In headless mode there is no window to pull events from. This used to be
    // assumed unreachable without a window, but an app that fully finishes
    // launching enters the main run loop, which calls this unconditionally —
    // so guard explicitly instead of unwrapping the absent window and panicking.
    if env.window.is_none() {
        return None;
    }

    loop {
        let Some(event) = env.window_mut().pop_event() else {
            break;
        };

        match event {
            Event::Quit => {
                echo!("User requested quit, exiting.");
                ui_application::exit(env);
            }
            Event::TouchesDown(..) | Event::TouchesMove(..) | Event::TouchesUp(..) => {
                ui_touch::handle_event(env, event)
            }
            Event::AppWillResignActive => {
                // Getting this event means touchHLE is becoming inactive, e.g.
                // due to switching apps. The obvious way to handle this would
                // be to just send `applicationWillResignActive:` to the
                // UIApplicationDelegate. However:
                // - touchHLE's event loop can't handle an inactive app well
                //   right now. For example, audio isn't paused.
                // - touchHLE's event loop can't handle the subsequent
                //   termination of an app right now: it doesn't manage to send
                //   the `applicationWillTerminate:` message in time. This can
                //   mean loss of data!
                // Therefore, for the moment we will simulate the early iOS
                // behavior where switching app usually resulted in termination.
                // We can usually handle this in time, so there won't be data
                // loss, nor problems with background resource usage or audio.
                // TODO: Handle this better.
                log!("Handling app-will-resign-active event: exiting.");
                ui_application::exit(env);
            }
            Event::AppWillTerminate => {
                log!("Handling app-will-terminate event.");
                ui_application::exit(env);
            }
            Event::EnterDebugger => {
                if env.is_debugging_enabled() {
                    log!("Handling EnterDebugger event: entering debugger.");
                    env.enter_debugger(/* reason: */ None);
                } else {
                    log!("Ignoring EnterDebugger event: no debugger connected.");
                }
            }
            Event::TextInput(text_event) => {
                let responder = env.framework_state.uikit.ui_responder.first_responder;
                let class = msg![env; responder class];
                let ui_text_field_class = env.objc.get_known_class("UITextField", &mut env.mem);

                if !responder.is_null() && env.objc.class_is_subclass_of(class, ui_text_field_class)
                {
                    match text_event {
                        TextInputEvent::Text(text) => {
                            ui_view::ui_control::ui_text_field::handle_text(env, responder, text)
                        }
                        TextInputEvent::Backspace => {
                            ui_view::ui_control::ui_text_field::handle_backspace(env, responder)
                        }
                        TextInputEvent::Return => {
                            ui_view::ui_control::ui_text_field::handle_return(env, responder)
                        }
                    }
                }
            }
        }
    }

    ui_accelerometer::handle_accelerometer(env)
}
