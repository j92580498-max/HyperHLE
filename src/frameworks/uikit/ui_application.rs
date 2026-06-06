/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIApplication` and `UIApplicationMain`.

use super::ui_device::*;
use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::core_graphics::CGRect;
use crate::frameworks::foundation::ns_string::{from_rust_string, get_static_str};
use crate::frameworks::foundation::{ns_array, ns_string, NSInteger, NSUInteger};
use crate::mem::MutPtr;
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr, SEL,
};
use crate::window::DeviceOrientation;
use crate::Environment;

#[derive(Default)]
pub struct State {
    /// [UIApplication sharedApplication]
    shared_application: Option<id>,
    pub(super) status_bar_hidden: bool,
    /// Whether shake to edit is enabled
    pub(super) application_supports_shake_to_edit: bool,
    pub(super) ignoring_interaction_events_count: u32,
}

#[derive(Default)]
struct UIApplicationHostObject {
    delegate: id,
    delegate_is_retained: bool,
    status_bar_style: UIStatusBarStyle,
    /// The most recent value set via `-setApplicationIconBadgeNumber:`.
    /// Per Apple's UIApplication docs the property is read/write and
    /// defaults to 0; we honour both the getter and the setter even
    /// though touchHLE has no springboard to actually render the badge.
    application_icon_badge_number: NSInteger,
}
impl HostObject for UIApplicationHostObject {}

pub type UIInterfaceOrientation = UIDeviceOrientation;
#[allow(unused)]
pub const UIInterfaceOrientationPortrait: UIInterfaceOrientation = UIDeviceOrientationPortrait;
#[allow(unused)]
pub const UIInterfaceOrientationPortraitUpsideDown: UIInterfaceOrientation =
    UIDeviceOrientationPortraitUpsideDown;
// These are intentionally swapped and documented as such (the UI on the device
// rotates in the opposite direction to how the device is rotated).
pub const UIInterfaceOrientationLandscapeLeft: UIInterfaceOrientation =
    UIDeviceOrientationLandscapeRight;
pub const UIInterfaceOrientationLandscapeRight: UIInterfaceOrientation =
    UIDeviceOrientationLandscapeLeft;
type UIRemoteNotificationType = NSUInteger;
type UIStatusBarAnimation = NSInteger;
type UIStatusBarStyle = NSInteger;
pub type UIApplicationState = NSInteger;
pub const UIApplicationStateActive: UIApplicationState = 0;
pub const UIApplicationStateInactive: UIApplicationState = 1;
pub const UIApplicationStateBackground: UIApplicationState = 2;

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);
@implementation UIApplication: UIResponder

// This should only be called by UIApplicationMain
+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(UIApplicationHostObject {
        delegate: nil,
        delegate_is_retained: false,
        status_bar_style: 0,
        application_icon_badge_number: 0,
    });
    env.objc.alloc_static_object(this, host_object, &mut env.mem)
}

+ (id)sharedApplication {
    env.framework_state.uikit.ui_application.shared_application.unwrap_or(nil)
}

- (())setNetworkActivityIndicatorVisible:(bool)visible {
    // touchHLE doesn't render the iOS status bar, so we just stub this
    // and ignore the request to show/hide the spinner.
    log_dbg!("Stubbed setNetworkActivityIndicatorVisible: {}", visible);
}

- (bool)isNetworkActivityIndicatorVisible {
    // Always report that it's hidden.
    false
}

// This should only be called by UIApplicationMain
- (id)init {
    assert!(env.framework_state.uikit.ui_application.shared_application.is_none());
    env.framework_state.uikit.ui_application.shared_application = Some(this);
    this
}

// This is a singleton, it shouldn't be deallocated.
- (id)retain { this }
- (id)autorelease { this }
- (())release {}

- (id)delegate {
    env.objc.borrow::<UIApplicationHostObject>(this).delegate
}
- (())setDelegate:(id)delegate { // something implementing UIApplicationDelegate
    let host_object = env.objc.borrow_mut::<UIApplicationHostObject>(this);
    // This property is quasi-non-retaining: https://stackoverflow.com/a/14271150/736162
    let old_delegate = std::mem::replace(&mut host_object.delegate, delegate);
    if host_object.delegate_is_retained {
        host_object.delegate_is_retained = false;
        if delegate != old_delegate {
            release(env, old_delegate);
        }
    }
}

- (bool)isStatusBarHidden {
    env.framework_state.uikit.ui_application.status_bar_hidden
}
- (())setStatusBarHidden:(bool)hidden {
    env.framework_state.uikit.ui_application.status_bar_hidden = hidden;
}
- (())setStatusBarHidden:(bool)hidden
                animated:(bool)_animated {
    // TODO: animation
    msg![env; this setStatusBarHidden:hidden]
}
- (())setStatusBarHidden:(bool)hidden
           withAnimation:(UIStatusBarAnimation)_animation {
    // TODO: animation
    msg![env; this setStatusBarHidden:hidden]
}

- (())setStatusBarStyle:(UIStatusBarStyle)style {
    env.objc.borrow_mut::<UIApplicationHostObject>(this).status_bar_style = style;
}
- (UIStatusBarStyle)statusBarStyle {
    env.objc.borrow::<UIApplicationHostObject>(this).status_bar_style
}

- (())setStatusBarStyle:(UIStatusBarStyle)style
               animated:(bool)_animated {
    msg![env; this setStatusBarStyle:style]
}

- (UIInterfaceOrientation)statusBarOrientation {
    match env.window().current_rotation() {
        DeviceOrientation::Portrait => UIDeviceOrientationPortrait,
        DeviceOrientation::LandscapeLeft => UIDeviceOrientationLandscapeLeft,
        DeviceOrientation::LandscapeRight => UIDeviceOrientationLandscapeRight
    }
}

- (f64)statusBarOrientationAnimationDuration {
    0.3
}

- (())setStatusBarOrientation:(UIInterfaceOrientation)orientation {
    match orientation {
        UIDeviceOrientationUnknown => {
            // Per Apple docs UIDeviceOrientationUnknown (0) means the
            // orientation cannot be determined.  Ignore it.
        }
        UIDeviceOrientationPortrait => {
            env.on_parent_stack_in_coroutine(|window, _| window.rotate_device(DeviceOrientation::Portrait));
        }
        UIDeviceOrientationLandscapeLeft => {
            env.on_parent_stack_in_coroutine(|window, _| window.rotate_device(DeviceOrientation::LandscapeLeft));
        }
        UIDeviceOrientationLandscapeRight => {
            env.on_parent_stack_in_coroutine(|window, _| window.rotate_device(DeviceOrientation::LandscapeRight));
        }
        _ => {
            log!("Warning: Orientation {} not handled yet (ignoring to prevent panic)", orientation);
        }
    }
}

- (())setStatusBarOrientation:(UIInterfaceOrientation)orientation
                     animated:(bool)_animated {
    // TODO: animation
    msg![env; this setStatusBarOrientation:orientation]
}

- (bool)isIdleTimerDisabled {
    !env.window().is_screen_saver_enabled()
}
- (())setIdleTimerDisabled:(bool)disabled {
    env.on_parent_stack_in_coroutine(|window, _| window.set_screen_saver_enabled(!disabled))
}

- (bool)canOpenURL:(id)url { // NSURL
    // Apple `-[UIApplication canOpenURL:]` documentation: returns `YES`
    // if the URL can be handled — i.e. the device has at least one
    // installed application that has registered for the URL's scheme
    // (`CFBundleURLTypes` → `CFBundleURLSchemes`). On iOS 9+ apps must
    // additionally list each scheme they intend to query in
    // `LSApplicationQueriesSchemes`, otherwise the call always returns
    // NO.
    //
    // touchHLE has no app launcher, but legitimately reports YES for
    // the URL schemes that the host environment (Android) routes to
    // its own Intent handlers — `http`/`https`/`tel`/`mailto`/`sms`
    // are universally handled, and we permit them here so apps that
    // gate sharing buttons on `canOpenURL:` (Talking Carl's social
    // links, the Bubble Witch share sheet, the Imobamoba "rate me"
    // popup) take the "yes, link the user out" branch instead of
    // greying out the button. Application-specific schemes (e.g.
    // `fb://`, `twitter://`) we report as unavailable because no
    // host-side app responds to them.
    if url == nil {
        return false;
    }
    let ns_string: id = msg![env; url scheme];
    if ns_string == nil {
        return false;
    }
    let scheme = ns_string::to_rust_string(env, ns_string);
    let scheme_lower = scheme.to_lowercase();

    // Schemes the host environment (browsers / mail / phone / SMS /
    // file viewers) is guaranteed to handle on every reasonable
    // device. Source: Apple "URL Schemes" Technote (
    // https://developer.apple.com/library/archive/featuredarticles/iPhoneURLScheme_Reference/
    // ).
    const HOST_HANDLED_SCHEMES: &[&str] = &[
        "http",
        "https",
        "ftp",
        "tel",
        "telprompt",
        "facetime",
        "facetime-audio",
        "mailto",
        "sms",
        "imessage",
        "file",
        "data",
        "itms",
        "itms-apps",
        "itms-services",
        "itmss",
        "maps",
    ];
    if HOST_HANDLED_SCHEMES.contains(&scheme_lower.as_str()) {
        return true;
    }

    // Honour the Info.plist allow-list (`LSApplicationQueriesSchemes`).
    // Real iOS uses this only to *gate* the query, not to answer it,
    // but if the app lists a scheme there it almost always genuinely
    // expects the answer to be NO when the corresponding app is not
    // installed. We therefore log a debug note and return false so the
    // app's "app isn't installed" fallback runs.
    let main_bundle: id = msg_class![env; NSBundle mainBundle];
    if main_bundle != nil {
        let key_str = ns_string::get_static_str(env, "LSApplicationQueriesSchemes");
        let allowed_arr: id = msg![env; main_bundle objectForInfoDictionaryKey:key_str];
        if allowed_arr != nil {
            let count: u32 = msg![env; allowed_arr count];
            for i in 0..count {
                let entry: id = msg![env; allowed_arr objectAtIndex:i];
                if entry == nil {
                    continue;
                }
                let entry_str = ns_string::to_rust_string(env, entry);
                if entry_str.to_lowercase() == scheme_lower {
                    log_dbg!(
                        "canOpenURL: {:?} is in LSApplicationQueriesSchemes; \
                         host can't launch it, returning NO",
                        scheme_lower
                    );
                    return false;
                }
            }
        }
    }

    log_dbg!(
        "canOpenURL: scheme {:?} not handled by host, returning NO",
        scheme_lower
    );
    false
}

- (bool)openURL:(id)url { // NSURL
    let ns_string = msg![env; url absoluteString];
    let url_string = ns_string::to_rust_string(env, ns_string);
    if let Err(e) = crate::window::open_url(env, &url_string) {
        echo!("App opened URL {:?} unsuccessfully ({}), exiting.", url_string, e);
    } else {
        echo!("App opened URL {:?}, exiting.", url_string);
    }

    exit(env);
    true
}

- (())beginIgnoringInteractionEvents {
    env.framework_state.uikit.ui_application.ignoring_interaction_events_count += 1;
}

- (bool)isIgnoringInteractionEvents {
    env.framework_state.uikit.ui_application.ignoring_interaction_events_count > 0
}

- (())endIgnoringInteractionEvents {
    let count = &mut env.framework_state.uikit.ui_application.ignoring_interaction_events_count;
    if *count > 0 {
        *count -= 1;
    } else {
        // В реальной iOS здесь выбрасывается исключение
        // NSInternalInconsistencyException,
        // но для стабильности эмулятора мы просто залогируем предупреждение,
        // если игра ошиблась со счетчиком.
        log!("Warning: endIgnoringInteractionEvents called without matching beginIgnoringInteractionEvents");
    }
}

- (())sendEvent:(id)event { // UIEvent*
    log_dbg!("UIApplication sendEvent: forwarding to key window");
    let window: id = msg![env; this keyWindow];
    if window != nil {
        msg![env; window sendEvent:event]
    }
}

- (bool)sendAction:(SEL)action
                to:(id)target
              from:(id)sender
          forEvent:(id)event { // UIEvent*
    if target != nil {
        let responds: bool = msg![env; target respondsToSelector:action];
        if responds {
            () = msg![env; target performSelector:action withObject:sender];
            return true;
        }
        return false;
    }
    // Walk responder chain if target is nil.
    let mut responder: id = sender;
    while responder != nil {
        let responds: bool = msg![env; responder respondsToSelector:action];
        if responds {
            () = msg![env; responder performSelector:action withObject:sender];
            return true;
        }
        responder = msg![env; responder nextResponder];
    }
    false
}

- (NSUInteger)beginBackgroundTaskWithExpirationHandler:(id)_handler {
    log_dbg!("UIApplication beginBackgroundTaskWithExpirationHandler");
    // Per Apple docs, this returns a UIBackgroundTaskIdentifier (NSUInteger).
    // A non-zero value indicates a valid task. touchHLE does not implement
    // background execution, so we return a fixed sentinel identifier.
    1
}

- (())endBackgroundTask:(NSUInteger)_task {
    log_dbg!("UIApplication endBackgroundTask: {}", _task);
}

- (NSUInteger)backgroundTimeRemaining {
    // Report effectively infinite time remaining.
    NSUInteger::MAX
}

- (UIApplicationState)applicationState {
    // Always report active.
    UIApplicationStateActive
}

- (bool)isProtectedDataAvailable {
    true
}

- (())setMinimumBackgroundFetchInterval:(f64)_interval {
    log!("UIApplication setMinimumBackgroundFetchInterval: stubbed");
}

- (())registerForRemoteNotifications {
    log!("UIApplication registerForRemoteNotifications: stubbed");
}

- (())unregisterForRemoteNotifications {
    log!("UIApplication unregisterForRemoteNotifications: stubbed");
}

- (bool)isRegisteredForRemoteNotifications {
    false
}

- (())registerUserNotificationSettings:(id)_settings {
    log!("UIApplication registerUserNotificationSettings: stubbed");
}

- (id)currentUserNotificationSettings {
    nil
}

- (())cancelAllLocalNotifications {
    log_dbg!("UIApplication cancelAllLocalNotifications: no-op (notifications not delivered by host)");
}

- (())cancelLocalNotification:(id)_notification {
    log_dbg!("UIApplication cancelLocalNotification: no-op (notifications not delivered by host)");
}

- (())scheduleLocalNotification:(id)_notification {
    log_dbg!("UIApplication scheduleLocalNotification: no-op (notifications not delivered by host)");
}

- (id)scheduledLocalNotifications {
    msg_class![env; NSArray new]
}

- (())setScheduledLocalNotifications:(id)_notifications {
    log!("UIApplication setScheduledLocalNotifications: stubbed");
}

- (bool)supportsShakeToEdit {
    false
}

- (())setSupportsShakeToEdit:(bool)_value {
    // Stub.
}

- (bool)applicationSupportsShakeToEdit {
    env.framework_state.uikit.ui_application.application_supports_shake_to_edit
}

- (())setApplicationSupportsShakeToEdit:(bool)value {
    env.framework_state.uikit.ui_application.application_supports_shake_to_edit = value;
}

- (())clearKeychainIfNecessary {
    // Stub.
}

- (CGRect)statusBarFrame {
    // Report a zero-height status bar since we don't render one.
    CGRect {
        origin: crate::frameworks::core_graphics::CGPoint { x: 0.0, y: 0.0 },
        size: crate::frameworks::core_graphics::CGSize { width: 320.0, height: 0.0 },
    }
}

- (())presentLocalNotificationNow:(id)_notification {
    log_dbg!("UIApplication presentLocalNotificationNow: no-op (notifications not delivered by host)");
}

- (id)keyWindow {
    let Some(key_window) = env
        .framework_state
        .uikit
        .ui_view
        .ui_window
        .key_window else {
        return nil;
    };
    assert!(env
        .framework_state
        .uikit
        .ui_view
        .ui_window
        .windows
        .contains(&key_window));
    key_window
}

- (id)windows {
    let windows: Vec<id> = (*env
        .framework_state
        .uikit
        .ui_view
        .ui_window
        .windows).to_vec();
    for window in &windows {
        retain(env, *window);
    }
    let windows = ns_array::from_vec(env, windows);
    autorelease(env, windows)
}

- (())registerForRemoteNotificationTypes:(UIRemoteNotificationType)types {
    log!("TODO: ignoring registerForRemoteNotificationTypes:{}", types);
}

// `- (UIRemoteNotificationType)enabledRemoteNotificationTypes` —
// per Apple's [UIApplication Reference](https://developer.apple.com/documentation/uikit/uiapplication/1623060-enabledremotenotificationtypes):
// the bitmask of remote notification types the user has explicitly
// enabled in Settings. touchHLE has no system Settings UI and no real
// push notification subsystem, so no notification types are enabled —
// the documented value for this state is `UIRemoteNotificationTypeNone`
// (0). Returning this lets apps using the iOS 3.0–7.x API path skip
// their push-registration branch without crashing.
- (UIRemoteNotificationType)enabledRemoteNotificationTypes {
    0 // UIRemoteNotificationTypeNone
}

// `applicationIconBadgeNumber` is the integer shown on the SpringBoard
// app icon badge. touchHLE has no SpringBoard, but games (e.g.
// notification-driven trial flows) often *read* the value back after
// setting it to gate logic, so we store it for round-trip fidelity per
// Apple's [UIApplication Reference](https://developer.apple.com/documentation/uikit/uiapplication/1622918-applicationiconbadgenumber).
- (NSInteger)applicationIconBadgeNumber {
    env.objc.borrow::<UIApplicationHostObject>(this).application_icon_badge_number
}
- (())setApplicationIconBadgeNumber:(NSInteger)bn {
    log_dbg!("setApplicationIconBadgeNumber:{}", bn);
    env.objc.borrow_mut::<UIApplicationHostObject>(this).application_icon_badge_number = bn;
}

- (id)nextResponder {
    let delegate = msg![env; this delegate];
    let app_delegate_class = msg![env; delegate class];
    let ui_responder_class = env.objc.get_known_class("UIResponder", &mut env.mem);
    if env.objc.class_is_subclass_of(app_delegate_class, ui_responder_class) {
        delegate
    } else {
        nil
    }
}

@end

};

/// Best-effort discovery of the application delegate class.
///
/// `UIApplicationMain` normally learns the delegate class from its
/// `delegateClassName` argument (or from a connection in the main nib). When
/// neither is available, look through the loaded classes for one that declares
/// a `UIApplicationDelegate` launch method and use that. Returns `nil` if no
/// candidate is found.
fn find_app_delegate_class(env: &Environment) -> id {
    for sel_name in [
        "application:didFinishLaunchingWithOptions:",
        "applicationDidFinishLaunching:",
    ] {
        if let Some(class) = env.objc.class_declaring_instance_method(sel_name) {
            return class;
        }
    }
    nil
}

/// `UIApplicationMain`, the entry point of the application.
pub(super) fn UIApplicationMain(
    env: &mut Environment,
    _argc: i32,
    _argv: MutPtr<MutPtr<u8>>,
    principal_class_name: id, // NSString*
    delegate_class_name: id,  // NSString*
) {
    let ui_application = {
        let pool: id = msg_class![env; NSAutoreleasePool new];

        let principal_class = if principal_class_name != nil {
            let name = ns_string::to_rust_string(env, principal_class_name);
            env.objc.get_known_class(&name, &mut env.mem)
        } else {
            env.objc.get_known_class("UIApplication", &mut env.mem)
        };
        let ui_application: id = msg![env; principal_class new];

        let device_family = env.options.device_family;
        if let Some(main_nib_filename) = env
            .bundle
            .main_nib_filename(device_family)
            .map(str::to_owned)
        {
            let ns_main_nib_filename = from_rust_string(env, main_nib_filename);
            let type_: id = get_static_str(env, "nib");
            let bundle: id = msg_class![env; NSBundle mainBundle];
            let res: id = msg![env; bundle pathForResource:ns_main_nib_filename ofType:type_];
            if res != nil {
                let nib: id = msg_class![env; UINib nibWithNibName:ns_main_nib_filename bundle:nil];
                release(env, ns_main_nib_filename);
                let _: id = msg![env; nib instantiateWithOwner:ui_application
                                               options:nil];
            } else {
                log!("Warning: couldn't load main nib file.");
            }
        }

        if env.bundle.status_bar_hidden() {
            let _: () = msg![env; ui_application setStatusBarHidden:true];
        }

        let delegate: id = msg![env; ui_application delegate];
        if delegate != nil {
            // The delegate was wired up while loading the main nib.
            env.objc
                .borrow_mut::<UIApplicationHostObject>(ui_application)
                .delegate_is_retained = true;
            retain(env, delegate);
        } else if delegate_class_name != nil
            && msg![env; delegate_class_name isEqual:principal_class_name]
        {
            // The app uses its principal class as its own delegate.
            let _: () = msg![env; ui_application setDelegate:ui_application];
        } else {
            // The delegate is normally created from the class named by the
            // `delegate_class_name` argument. Some apps reach this point
            // without a usable name: the argument is nil (the delegate was
            // expected from the main nib, but no nib connection set it), or
            // the value the binary passed could not be resolved to a real
            // class (e.g. it was derived from a class reference that ended up
            // nil, which decodes to the empty string). Previously this left
            // the application with a nil delegate, so
            // `application:didFinishLaunchingWithOptions:` was never sent and
            // the app stayed frozen on its launch image. Fall back to
            // discovering the app's delegate class from the loaded classes.
            let mut delegate_class: id = nil;
            if delegate_class_name != nil {
                let name = ns_string::to_rust_string(env, delegate_class_name).into_owned();
                if !name.is_empty() {
                    delegate_class = env
                        .objc
                        .try_get_known_class(&name, &mut env.mem)
                        .unwrap_or(nil);
                }
            }
            if delegate_class == nil {
                delegate_class = find_app_delegate_class(env);
                if delegate_class != nil {
                    log!(
                        "UIApplicationMain: no usable delegate class name was \
                         provided; using discovered application delegate class."
                    );
                }
            }
            if delegate_class != nil {
                let delegate: id = msg![env; delegate_class new];
                let _: () = msg![env; ui_application setDelegate:delegate];
            } else {
                log!(
                    "Warning: UIApplicationMain could not determine an \
                     application delegate; the app may not finish launching."
                );
            }
        };

        // UIMainStoryboardFile (iOS 5+) replaces NSMainNibFile for modern
        // apps. When present, the documented launch flow is:
        //   1. Load the named storyboard.
        //   2. Create a UIWindow sized to the main screen's bounds.
        //   3. Instantiate the storyboard's initial view controller.
        //   4. Set that view controller as the window's rootViewController.
        //   5. Send -makeKeyAndVisible to the window.
        //   6. Assign the window to the app delegate's `window` property
        //      via KVC (Apple's UIApplicationMain does this so app
        //      delegates can return the window from `-window` without any
        //      manual wiring inside `application:didFinishLaunchingWithOptions:`).
        let storyboard_name = env
            .bundle
            .main_storyboard_filename(device_family)
            .map(str::to_owned);
        if let Some(storyboard_name) = storyboard_name {
            let storyboard_name_ns = from_rust_string(env, storyboard_name.clone());
            let storyboard_class = env.objc.get_known_class("UIStoryboard", &mut env.mem);
            let storyboard: id =
                msg![env; storyboard_class storyboardWithName:storyboard_name_ns bundle:nil];
            release(env, storyboard_name_ns);

            if storyboard != nil {
                let initial_vc: id = msg![env; storyboard instantiateInitialViewController];
                if initial_vc != nil {
                    let screen: id = msg_class![env; UIScreen mainScreen];
                    let bounds: CGRect = msg![env; screen bounds];
                    let window: id = msg_class![env; UIWindow alloc];
                    let window: id = msg![env; window initWithFrame:bounds];

                    let _: () = msg![env; window setRootViewController:initial_vc];
                    let _: () = msg![env; window makeKeyAndVisible];

                    let delegate: id = msg![env; ui_application delegate];
                    if delegate != nil {
                        let window_key: id = get_static_str(env, "window");
                        let _: () = msg![env; delegate setValue:window forKey:window_key];
                    }
                } else {
                    log!(
                        "Warning: storyboard {:?} has no initial view controller; \
                         skipping window setup.",
                        storyboard_name,
                    );
                }
            } else {
                log!(
                    "Warning: couldn't load main storyboard {:?}.",
                    storyboard_name
                );
            }
        }

        let _: () = msg![env; pool drain];
        ui_application
    };

    {
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let delegate: id = msg![env; ui_application delegate];
        if env.objc.object_has_method_named(
            &env.mem,
            delegate,
            "application:didFinishLaunchingWithOptions:",
        ) {
            let empty_dict: id = msg_class![env; NSDictionary dictionary];
            () = msg![env; delegate application:ui_application didFinishLaunchingWithOptions:empty_dict];
        } else if env.objc.object_has_method_named(
            &env.mem,
            delegate,
            "applicationDidFinishLaunching:",
        ) {
            () = msg![env; delegate applicationDidFinishLaunching:ui_application];
        }

        let center: id = msg_class![env; NSNotificationCenter defaultCenter];
        let notif_name = get_static_str(env, UIApplicationDidFinishLaunchingNotification);
        () = msg![env; center postNotificationName:notif_name object:ui_application userInfo:nil];

        let _: () = msg![env; pool drain];
    }

    let views = env.framework_state.uikit.ui_view.views.clone();
    for view in views {
        () = msg![env; view layoutSubviews];
    }

    {
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let delegate: id = msg![env; ui_application delegate];
        if env
            .objc
            .object_has_method_named(&env.mem, delegate, "applicationDidBecomeActive:")
        {
            () = msg![env; delegate applicationDidBecomeActive:ui_application];
        }
        let center: id = msg_class![env; NSNotificationCenter defaultCenter];
        let notif_name = get_static_str(env, UIApplicationDidBecomeActiveNotification);
        () = msg![env; center postNotificationName:notif_name object:ui_application userInfo:nil];
        let _: () = msg![env; pool drain];
    }

    // Apple docs: after `beginGeneratingDeviceOrientationNotifications` the
    // device begins generating `UIDeviceOrientationDidChangeNotification`s.
    // On a real iPhone the accelerometer wakes the moment the user picks up
    // the device, so the very first orientation event practically always
    // arrives during launch. Some games (e.g. Dead Space) gate the start of
    // their C++ engine on this first notification — if it never fires they
    // sit forever in the run loop waiting for it. Post an initial
    // notification here so the app can transition out of its idle splash
    // state. See `UIDevice` documentation:
    //   https://developer.apple.com/documentation/uikit/uidevice/1620018-beginGeneratingdeviceorientationn
    {
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let current_device: id = msg_class![env; UIDevice currentDevice];
        let is_generating: bool =
            msg![env; current_device isGeneratingDeviceOrientationNotifications];
        if is_generating {
            log_dbg!(
                "Posting initial UIDeviceOrientationDidChangeNotification \
                 so apps observing device orientation can finish initializing."
            );
            let _: () = msg![env; current_device _postOrientationChangeNotification];
        }
        let _: () = msg![env; pool drain];
    }

    let run_loop: id = msg_class![env; NSRunLoop mainRunLoop];
    let _: () = msg![env; run_loop run];
}

pub(super) fn exit(env: &mut Environment) {
    let ui_application: id = msg_class![env; UIApplication sharedApplication];
    let center: id = msg_class![env; NSNotificationCenter defaultCenter];

    {
        let pool: id = msg_class![env; NSAutoreleasePool new];
        if !env.is_app_picker {
            let user_defaults: id = msg_class![env; NSUserDefaults standardUserDefaults];
            let _: bool = msg![env; user_defaults synchronize];
        }
        let delegate: id = msg![env; ui_application delegate];
        if env
            .objc
            .object_has_method_named(&env.mem, delegate, "applicationWillResignActive:")
        {
            () = msg![env; delegate applicationWillResignActive:ui_application];
        }
        let notif_name = get_static_str(env, UIApplicationWillResignActiveNotification);
        () = msg![env; center postNotificationName:notif_name object:ui_application userInfo:nil];
        let _: () = msg![env; pool drain];
    };
    {
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let delegate: id = msg![env; ui_application delegate];
        if env
            .objc
            .object_has_method_named(&env.mem, delegate, "applicationWillTerminate:")
        {
            () = msg![env; delegate applicationWillTerminate:ui_application];
        }
        let notif_name = get_static_str(env, UIApplicationWillTerminateNotification);
        () = msg![env; center postNotificationName:notif_name object:ui_application userInfo:nil];
        let _: () = msg![env; pool drain];
    };

    std::process::exit(0);
}

const UIApplicationDidFinishLaunchingNotification: &str =
    "UIApplicationDidFinishLaunchingNotification";
const UIApplicationDidBecomeActiveNotification: &str = "UIApplicationDidBecomeActiveNotification";
const UIApplicationDidEnterBackgroundNotification: &str =
    "UIApplicationDidEnterBackgroundNotification";
const UIApplicationWillEnterForegroundNotification: &str =
    "UIApplicationWillEnterForegroundNotification";
const UIApplicationWillResignActiveNotification: &str = "UIApplicationWillResignActiveNotification";
const UIApplicationWillTerminateNotification: &str = "UIApplicationWillTerminateNotification";
const UIApplicationLaunchOptionsRemoteNotificationKey: &str =
    "UIApplicationLaunchOptionsRemoteNotificationKey";
const UIApplicationDidReceiveMemoryWarningNotification: &str =
    "UIApplicationDidReceiveMemoryWarningNotification";

// Apple `UIApplication.h` declares these as
// `UIKIT_EXTERN NSNotificationName const ...` (and `NSString * const` in
// older SDKs). The exact literal values are what Apple uses for
// notification posting and dictionary keys; matching them lets observer
// registration via `-[NSNotificationCenter addObserver:selector:name:object:]`
// resolve correctly.
//
// References:
// * Apple [UIApplication notification names](https://developer.apple.com/documentation/uikit/uiapplication)
// * Apple [Launch options keys](https://developer.apple.com/documentation/uikit/uiapplication/launchoptionskey)
// * Apple [Status-bar related notifications](https://developer.apple.com/documentation/uikit/uiapplicationdidchangestatusbarframenotification)
const UIApplicationProtectedDataDidBecomeAvailable: &str =
    "UIApplicationProtectedDataDidBecomeAvailable";
const UIApplicationProtectedDataWillBecomeUnavailable: &str =
    "UIApplicationProtectedDataWillBecomeUnavailable";
const UIApplicationSignificantTimeChangeNotification: &str =
    "UIApplicationSignificantTimeChangeNotification";
const UIApplicationDidChangeStatusBarFrameNotification: &str =
    "UIApplicationDidChangeStatusBarFrameNotification";
const UIApplicationWillChangeStatusBarFrameNotification: &str =
    "UIApplicationWillChangeStatusBarFrameNotification";
const UIApplicationDidChangeStatusBarOrientationNotification: &str =
    "UIApplicationDidChangeStatusBarOrientationNotification";
const UIApplicationWillChangeStatusBarOrientationNotification: &str =
    "UIApplicationWillChangeStatusBarOrientationNotification";
const UIApplicationStatusBarFrameUserInfoKey: &str = "UIApplicationStatusBarFrameUserInfoKey";
const UIApplicationStatusBarOrientationUserInfoKey: &str =
    "UIApplicationStatusBarOrientationUserInfoKey";
const UIApplicationBackgroundFetchIntervalMinimum: &str =
    "UIApplicationBackgroundFetchIntervalMinimum";
const UIApplicationBackgroundFetchIntervalNever: &str =
    "UIApplicationBackgroundFetchIntervalNever";
// Launch options keys — Apple `UIApplication.h` (`UIApplicationLaunchOptionsKey`).
const UIApplicationLaunchOptionsURLKey: &str = "UIApplicationLaunchOptionsURLKey";
const UIApplicationLaunchOptionsSourceApplicationKey: &str =
    "UIApplicationLaunchOptionsSourceApplicationKey";
const UIApplicationLaunchOptionsAnnotationKey: &str = "UIApplicationLaunchOptionsAnnotationKey";
const UIApplicationLaunchOptionsLocalNotificationKey: &str =
    "UIApplicationLaunchOptionsLocalNotificationKey";
const UIApplicationLaunchOptionsLocationKey: &str = "UIApplicationLaunchOptionsLocationKey";
const UIApplicationLaunchOptionsNewsstandDownloadsKey: &str =
    "UIApplicationLaunchOptionsNewsstandDownloadsKey";
const UIApplicationLaunchOptionsBluetoothCentralsKey: &str =
    "UIApplicationLaunchOptionsBluetoothCentralsKey";
const UIApplicationLaunchOptionsBluetoothPeripheralsKey: &str =
    "UIApplicationLaunchOptionsBluetoothPeripheralsKey";
const UIApplicationLaunchOptionsShortcutItemKey: &str = "UIApplicationLaunchOptionsShortcutItemKey";
// `UIApplicationOpenSettingsURLString` from `UIApplication.h`. iOS apps
// pass this to `-[UIApplication openURL:]` to open the system Settings
// app at their own page; the literal value is `"app-settings:"` per
// Apple's docs (<https://developer.apple.com/documentation/uikit/uiapplicationopensettingsurlstring>).
const UIApplicationOpenSettingsURLString: &str = "app-settings:";
// Newer (iOS 9) open-URL option keys — `UIApplicationOpenURLOptionsKey`.
const UIApplicationOpenURLOptionsSourceApplicationKey: &str =
    "UIApplicationOpenURLOptionsSourceApplicationKey";
const UIApplicationOpenURLOptionsAnnotationKey: &str = "UIApplicationOpenURLOptionsAnnotationKey";
const UIApplicationOpenURLOptionsOpenInPlaceKey: &str = "UIApplicationOpenURLOptionsOpenInPlaceKey";
const UIApplicationOpenURLOptionUniversalLinksOnly: &str =
    "UIApplicationOpenURLOptionUniversalLinksOnly";
pub const CONSTANTS: ConstantExports = &[
    (
        "_UIApplicationDidFinishLaunchingNotification",
        HostConstant::NSString(UIApplicationDidFinishLaunchingNotification),
    ),
    (
        "_UIApplicationDidBecomeActiveNotification",
        HostConstant::NSString(UIApplicationDidBecomeActiveNotification),
    ),
    (
        "_UIApplicationDidEnterBackgroundNotification",
        HostConstant::NSString(UIApplicationDidEnterBackgroundNotification),
    ),
    (
        "_UIApplicationWillEnterForegroundNotification",
        HostConstant::NSString(UIApplicationWillEnterForegroundNotification),
    ),
    (
        "_UIApplicationWillResignActiveNotification",
        HostConstant::NSString(UIApplicationWillResignActiveNotification),
    ),
    (
        "_UIApplicationWillTerminateNotification",
        HostConstant::NSString(UIApplicationWillTerminateNotification),
    ),
    (
        "_UIApplicationDidReceiveMemoryWarningNotification",
        HostConstant::NSString(UIApplicationDidReceiveMemoryWarningNotification),
    ),
    (
        "_UIApplicationLaunchOptionsRemoteNotificationKey",
        HostConstant::NSString(UIApplicationLaunchOptionsRemoteNotificationKey),
    ),
    (
        "_UIApplicationProtectedDataDidBecomeAvailable",
        HostConstant::NSString(UIApplicationProtectedDataDidBecomeAvailable),
    ),
    (
        "_UIApplicationProtectedDataWillBecomeUnavailable",
        HostConstant::NSString(UIApplicationProtectedDataWillBecomeUnavailable),
    ),
    (
        "_UIApplicationSignificantTimeChangeNotification",
        HostConstant::NSString(UIApplicationSignificantTimeChangeNotification),
    ),
    (
        "_UIApplicationDidChangeStatusBarFrameNotification",
        HostConstant::NSString(UIApplicationDidChangeStatusBarFrameNotification),
    ),
    (
        "_UIApplicationWillChangeStatusBarFrameNotification",
        HostConstant::NSString(UIApplicationWillChangeStatusBarFrameNotification),
    ),
    (
        "_UIApplicationDidChangeStatusBarOrientationNotification",
        HostConstant::NSString(UIApplicationDidChangeStatusBarOrientationNotification),
    ),
    (
        "_UIApplicationWillChangeStatusBarOrientationNotification",
        HostConstant::NSString(UIApplicationWillChangeStatusBarOrientationNotification),
    ),
    (
        "_UIApplicationStatusBarFrameUserInfoKey",
        HostConstant::NSString(UIApplicationStatusBarFrameUserInfoKey),
    ),
    (
        "_UIApplicationStatusBarOrientationUserInfoKey",
        HostConstant::NSString(UIApplicationStatusBarOrientationUserInfoKey),
    ),
    (
        "_UIApplicationBackgroundFetchIntervalMinimum",
        HostConstant::NSString(UIApplicationBackgroundFetchIntervalMinimum),
    ),
    (
        "_UIApplicationBackgroundFetchIntervalNever",
        HostConstant::NSString(UIApplicationBackgroundFetchIntervalNever),
    ),
    (
        "_UIApplicationLaunchOptionsURLKey",
        HostConstant::NSString(UIApplicationLaunchOptionsURLKey),
    ),
    (
        "_UIApplicationLaunchOptionsSourceApplicationKey",
        HostConstant::NSString(UIApplicationLaunchOptionsSourceApplicationKey),
    ),
    (
        "_UIApplicationLaunchOptionsAnnotationKey",
        HostConstant::NSString(UIApplicationLaunchOptionsAnnotationKey),
    ),
    (
        "_UIApplicationLaunchOptionsLocalNotificationKey",
        HostConstant::NSString(UIApplicationLaunchOptionsLocalNotificationKey),
    ),
    (
        "_UIApplicationLaunchOptionsLocationKey",
        HostConstant::NSString(UIApplicationLaunchOptionsLocationKey),
    ),
    (
        "_UIApplicationLaunchOptionsNewsstandDownloadsKey",
        HostConstant::NSString(UIApplicationLaunchOptionsNewsstandDownloadsKey),
    ),
    (
        "_UIApplicationLaunchOptionsBluetoothCentralsKey",
        HostConstant::NSString(UIApplicationLaunchOptionsBluetoothCentralsKey),
    ),
    (
        "_UIApplicationLaunchOptionsBluetoothPeripheralsKey",
        HostConstant::NSString(UIApplicationLaunchOptionsBluetoothPeripheralsKey),
    ),
    (
        "_UIApplicationLaunchOptionsShortcutItemKey",
        HostConstant::NSString(UIApplicationLaunchOptionsShortcutItemKey),
    ),
    (
        "_UIApplicationOpenSettingsURLString",
        HostConstant::NSString(UIApplicationOpenSettingsURLString),
    ),
    (
        "_UIApplicationOpenURLOptionsSourceApplicationKey",
        HostConstant::NSString(UIApplicationOpenURLOptionsSourceApplicationKey),
    ),
    (
        "_UIApplicationOpenURLOptionsAnnotationKey",
        HostConstant::NSString(UIApplicationOpenURLOptionsAnnotationKey),
    ),
    (
        "_UIApplicationOpenURLOptionsOpenInPlaceKey",
        HostConstant::NSString(UIApplicationOpenURLOptionsOpenInPlaceKey),
    ),
    (
        "_UIApplicationOpenURLOptionUniversalLinksOnly",
        HostConstant::NSString(UIApplicationOpenURLOptionUniversalLinksOnly),
    ),
];
pub const FUNCTIONS: FunctionExports = &[export_c_func!(UIApplicationMain(_, _, _, _))];
