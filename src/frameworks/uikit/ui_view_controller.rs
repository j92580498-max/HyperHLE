/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIViewController`.
//!
//! Resources:
//! - [View Controller Programming Guide for iOS (Legacy)](https://developer.apple.com/library/archive/documentation/WindowsViews/Conceptual/ViewControllerPGforiOSLegacy/BasicViewControllers/BasicViewControllers.html)

use crate::frameworks::core_graphics::{CGRect, CGSize};
use crate::frameworks::foundation::ns_objc_runtime::NSStringFromClass;
use crate::frameworks::foundation::ns_string::{from_rust_string, get_static_str, to_rust_string};
use crate::frameworks::foundation::NSUInteger;
use crate::frameworks::uikit::ui_application::UIInterfaceOrientation;
use crate::frameworks::uikit::ui_view::set_view_controller;
use crate::objc::{
    autorelease, id, msg, msg_class, msg_super, nil, objc_classes, release, retain,
    Class, ClassExports, HostObject, NSZonePtr,
};
use crate::Environment;

pub mod ui_navigation_controller;

pub type UIModalTransitionStyle = i32;
pub type UIModalPresentationStyle = i32;

#[derive(Default)]
pub(crate) struct UIViewControllerHostObject {
    /// The root view.
    /// `UIView*`
    view: id,
    /// Nib name to be used at the load
    /// of the root view, may be nil.
    /// `NSString*`
    nib_name: id,
    /// Bundle to be used for load
    /// of the nib by name, may be nil.
    /// `NSBundle*`
    bundle: id,
    title: id,                  // Для хранения строки заголовка
    parent_view_controller: id, // Для связи с родительским VC
    navigation_controller: id,  // Для фикса ошибки "does not respond to selector"
    /// Currently presented modal view controller (retained), or `nil`.
    /// `UIViewController*`
    presented_view_controller: id,
    /// View controller that is presenting `self` modally (non-retained
    /// back-pointer), or `nil`.
    /// `UIViewController*`
    presenting_view_controller: id,
    /// Lazily-created `UINavigationItem` returned by `-navigationItem`.
    /// Retained while it lives in this slot.
    navigation_item: id,
    // ---------------------------
    modal_transition_style: UIModalTransitionStyle,
    modal_presentation_style: UIModalPresentationStyle,
    editing: bool,
    /// When non-empty, the path (relative to the bundle resource path)
    /// at which the storyboardc directory lives. Storyboard-instantiated
    /// view controllers store the path so `-loadView` knows to load the
    /// view nib from inside the storyboardc.
    pub(crate) storyboard_view_nib_dir: String,
    /// Back-pointer to the `UIStoryboard*` that produced this view
    /// controller (retained). `nil` when the VC was instantiated outside
    /// of a storyboard.
    pub(crate) storyboard: id,
    /// Backing store for `extendedLayoutIncludesOpaqueBars` (default `false`,
    /// matching UIKit). See
    /// <https://developer.apple.com/documentation/uikit/uiviewcontroller/1621515-extendedlayoutincludesopaquebars>.
    extended_layout_includes_opaque_bars: bool,
    /// Backing store for `edgesForExtendedLayout` (default
    /// `UIRectEdgeAll` = 0xF on iOS 7+). See
    /// <https://developer.apple.com/documentation/uikit/uiviewcontroller/1621515-edgesforextendedlayout>.
    edges_for_extended_layout: NSUInteger,
}
impl HostObject for UIViewControllerHostObject {}

// Apple's UIRectEdgeAll = UIRectEdgeTop|UIRectEdgeLeft|UIRectEdgeBottom|UIRectEdgeRight = 15
const UI_RECT_EDGE_ALL: NSUInteger = 15;

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIViewController: UIResponder

+ (id)allocWithZone:(NSZonePtr)_zone {
    let mut host_object = Box::<UIViewControllerHostObject>::default();
    host_object.edges_for_extended_layout = UI_RECT_EDGE_ALL;
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (())attemptRotationToDeviceOrientation {
    // Apple docs (UIViewController Class Reference):
    //   "Attempts to rotate all windows to the orientation of the device.
    //    Some view controllers may want to use app-specific conditions to
    //    determine what interface orientations are supported. If your view
    //    controller does this, when those conditions change your app should
    //    call this class method. The system immediately attempts to rotate
    //    to the new orientation."
    //
    // touchHLE does not currently perform live device-orientation
    // rotations — the window is pinned to the orientation the app launched
    // in. Re-querying supported orientations would therefore not result in
    // any change, so the documented behavior reduces to a no-op for our
    // host. We still log the call so app authors can see that their hook
    // ran in case they wire up rotation handling later.
    log_dbg!(
        "+[UIViewController attemptRotationToDeviceOrientation]: no live \
         rotation system; treating as a no-op."
    );
}

- (id)navigationController {
    env.objc.borrow::<UIViewControllerHostObject>(this).navigation_controller
}

- (id)parentViewController {
    env.objc.borrow::<UIViewControllerHostObject>(this).parent_view_controller
}

- (id)initWithNibName:(id)nib_name // NSString *
               bundle:(id)bundle { // NSBundle *
    if nib_name != nil {
        retain(env, nib_name);
    }
    if bundle != nil {
        retain(env, bundle);
    }

    log_dbg!("[(UIViewController*){:?} initWithNibName:{:?} bundle:{:?}]", this, nib_name, bundle);

    env.objc.borrow_mut::<UIViewControllerHostObject>(this).nib_name = nib_name;
    env.objc.borrow_mut::<UIViewControllerHostObject>(this).bundle = bundle;
    this
}

- (id)initWithCoder:(id)coder {
    let key_ns_string = get_static_str(env, "UIView");
    let view: id = msg![env; coder decodeObjectForKey:key_ns_string];
    () = msg![env; this setView:view];

    // Документация Apple: "When instantiating a view controller from a
    // storyboard/nib,
    // iOS initializes the new view controller by calling its initWithCoder:
    // method instead of this method
    // and sets the nibName property to a nib file stored inside the
    // storyboard."
    let nib_name_key = get_static_str(env, "UINibName");
    let mut nib_name: id = msg![env; coder decodeObjectForKey:nib_name_key];

    // В старых рантаймах/сторибордах ключ может быть другим
    if nib_name == nil {
        let sb_name_key = get_static_str(env, "UIStoryboardName");
        nib_name = msg![env; coder decodeObjectForKey:sb_name_key];
    }

    if nib_name != nil {
        retain(env, nib_name);
        let host_obj = env.objc.borrow_mut::<UIViewControllerHostObject>(this);
        let old_nib_name = host_obj.nib_name;
        host_obj.nib_name = nib_name;
        release(env, old_nib_name);
    }

    let bundle_key = get_static_str(env, "UIBundleName");
    let bundle: id = msg![env; coder decodeObjectForKey:bundle_key];
    if bundle != nil {
        retain(env, bundle);
        let host_obj = env.objc.borrow_mut::<UIViewControllerHostObject>(this);
        let old_bundle = host_obj.bundle;
        host_obj.bundle = bundle;
        release(env, old_bundle);
    }

    this
}

- (())dealloc {
    let (view, nib_name, bundle, title, presented, storyboard) = {
        let h = env.objc.borrow::<UIViewControllerHostObject>(this);
        (h.view, h.nib_name, h.bundle, h.title, h.presented_view_controller, h.storyboard)
    };
    if view != nil { release(env, view); }
    if nib_name != nil { release(env, nib_name); }
    if bundle != nil { release(env, bundle); }
    if title != nil { release(env, title); }
    if presented != nil { release(env, presented); }
    // presenting_view_controller is a non-retained back-pointer; do not
    // release.
    let navigation_item = env.objc.borrow::<UIViewControllerHostObject>(this).navigation_item;
    if navigation_item != nil { release(env, navigation_item); }
    if storyboard != nil { release(env, storyboard); }

    env.objc.dealloc_object(this, &mut env.mem);
}

- (())setParentViewController:(id)parent {
    env.objc.borrow_mut::<UIViewControllerHostObject>(this).parent_view_controller = parent;
}

- (())setNavigationController:(id)nav_controller {
    env.objc.borrow_mut::<UIViewControllerHostObject>(this).navigation_controller = nav_controller;
}

- (())loadView {
    // В этот момент msg![env; this nibName] уже резолвит и применяет правила
    // поиска (см. метод nibName ниже)
    let nib_name: id = msg![env; this nibName];

    let mut bundle: id = msg![env; this nibBundle];
    if bundle == nil {
        bundle = msg_class![env; NSBundle mainBundle];
    }

    // For storyboard-instantiated view controllers, the view nib lives
    // inside the storyboardc directory rather than at the bundle root.
    // Construct the full bundle-relative path before handing it off to
    // `+[UINib nibWithNibName:bundle:]` so its normal resource search
    // picks the right file.
    let storyboard_dir = env
        .objc
        .borrow::<UIViewControllerHostObject>(this)
        .storyboard_view_nib_dir
        .clone();
    if !storyboard_dir.is_empty() && nib_name != nil {
        let nib_name_rust = to_rust_string(env, nib_name).into_owned();
        // The compiled storyboardc stores per-device variants of the view
        // nib as `<name>~iphone.nib` / `<name>~ipad.nib` next to the
        // universal `<name>.nib`. Probe both — `+[UINib
        // nibWithNibName:bundle:]` goes through NSBundle which only
        // automatically applies device suffixes at the bundle root, not
        // for files nested inside storyboardc subdirectories.
        let device_suffix = match env.options.device_family {
            Some(crate::window::DeviceFamily::iPad) => "~ipad",
            Some(
                crate::window::DeviceFamily::iPhone | crate::window::DeviceFamily::iPhone5,
            ) => "~iphone",
            None => "",
        };
        let mut candidates: Vec<String> = Vec::new();
        if !nib_name_rust.starts_with(&storyboard_dir) {
            candidates.push(format!(
                "{}/{}{}",
                storyboard_dir, nib_name_rust, device_suffix,
            ));
            candidates.push(format!("{}/{}", storyboard_dir, nib_name_rust));
        } else {
            candidates.push(nib_name_rust.clone());
        }
        for candidate in &candidates {
            let candidate_ns = from_rust_string(env, candidate.clone());
            let nib: id = msg_class![env; UINib nibWithNibName:candidate_ns bundle:bundle];
            release(env, candidate_ns);
            if nib != nil {
                () = msg![env; nib instantiateWithOwner:this options:nil];
                if env.objc.borrow::<UIViewControllerHostObject>(this).view != nil {
                    return;
                }
            }
        }
    }

    if nib_name != nil {
        let nib: id = msg_class![env; UINib nibWithNibName:nib_name bundle:bundle];
        if nib != nil {
            () = msg![env; nib instantiateWithOwner:this options:nil];

            // Если NIB загружен и outlet view инициализирован:
            if env.objc.borrow::<UIViewControllerHostObject>(this).view != nil {
                return;
            }
        }
    }

    // "If the view controller does not have an associated nib file, this method
    // creates a plain UIView object instead."
    let screen = msg_class![env; UIScreen mainScreen];
    let bounds: CGRect = msg![env; screen bounds];

    let view = msg_class![env; UIView alloc];
    let view: id = msg![env; view initWithFrame:bounds];
    () = msg![env; this setView:view];
    release(env, view); // setView сделает retain
}

- (())setView:(id)new_view { // UIView*
    // When loading a storyboard scene nib, the scene's outlet connection
    // for `view` sends `setView:` with a `UIStoryboardPlaceholder` rather
    // than a real `UIView`. The placeholder acts as a marker: we capture
    // the storyboardc directory it carries, but do NOT store the
    // placeholder as the actual view. Once the storyboard machinery
    // finishes wiring things up, `-view` will trigger `-loadView`, which
    // can now resolve the view nib relative to that directory.
    if new_view != nil {
        let placeholder_class = env.objc.get_known_class(
            "UIStoryboardPlaceholder",
            &mut env.mem,
        );
        let view_class: Class = msg![env; new_view class];
        if env.objc.class_is_subclass_of(view_class, placeholder_class) {
            let storyboardc_dir_ns = env
                .objc
                .borrow::<crate::frameworks::uikit::ui_storyboard
                    ::UIStoryboardPlaceholderHostObject>(new_view)
                .storyboardc_relative_dir;
            if storyboardc_dir_ns != nil {
                let dir = to_rust_string(env, storyboardc_dir_ns).into_owned();
                env.objc
                    .borrow_mut::<UIViewControllerHostObject>(this)
                    .storyboard_view_nib_dir = dir;
            }
            return;
        }
    }

    let host_obj = env.objc.borrow_mut::<UIViewControllerHostObject>(this);
    let old_view = std::mem::replace(&mut host_obj.view, new_view);
    if old_view != nil {
        set_view_controller(env, old_view, nil);
    }
    if new_view != nil {
        set_view_controller(env, new_view, this);
    }
    if new_view != nil {
        retain(env, new_view);
    }
    if old_view != nil {
        release(env, old_view);
    }
}

- (id)storyboard {
    env.objc.borrow::<UIViewControllerHostObject>(this).storyboard
}

- (id)view {
    // Apple's UIViewController.view documentation:
    // "If you access this property and its value is currently nil, the view
    // controller automatically calls the loadView method and returns the
    // resulting view."  The view *must* be re-read from the host object after
    // -viewDidLoad runs, because subclasses (e.g. ones that build an OpenGL
    // EAGL view in -viewDidLoad) commonly call -setView: from within
    // -viewDidLoad to swap out the placeholder UIView created by -loadView.
    // If we returned the value captured before -viewDidLoad, callers would
    // hold a dangling pointer to the just-released placeholder.
    let view = env.objc.borrow::<UIViewControllerHostObject>(this).view;
    if view == nil {
        () = msg![env; this loadView];
        () = msg![env; this viewDidLoad];
        env.objc.borrow::<UIViewControllerHostObject>(this).view
    } else {
        view
    }
}

// Перехватываем NIB-соединение (KVC) для свойства view
- (())setValue:(id)value forKey:(id)key {
    let key_str = to_rust_string(env, key);
    if key_str == "view" {
        () = msg![env; this setView:value];
    } else {
        () = msg_super![env; this setValue:value forKey:key];
    }
}

// Usually overridden by the application
- (())viewDidLoad {
    log_dbg!("[(UIViewController*){:?} viewDidLoad]", this);
}
- (())viewWillAppear:(bool)animated {
    log_dbg!("[(UIViewController*){:?} viewWillAppear:{}]", this, animated);
}
- (())viewDidAppear:(bool)animated {
    log_dbg!("[(UIViewController*){:?} viewDidAppear:{}]", this, animated);
}
- (())viewWillDisappear:(bool)animated {
    log_dbg!("[(UIViewController*){:?} viewWillDisappear:{}]", this, animated);
}
- (())viewDidDisappear:(bool)animated {
    log_dbg!("[(UIViewController*){:?} viewDidDisappear:{}]", this, animated);
}

- (())setTitle:(id)title {
    let old_title = env.objc.borrow::<UIViewControllerHostObject>(this).title;
    if old_title != nil {
        release(env, old_title);
    }
    if title != nil {
        retain(env, title);
    }
    env.objc.borrow_mut::<UIViewControllerHostObject>(this).title = title;
}

- (())setEditing:(bool)editing {
    env.objc.borrow_mut::<UIViewControllerHostObject>(this).editing = editing;
}
- (bool)isEditing {
    env.objc.borrow::<UIViewControllerHostObject>(this).editing
}
- (())setWantsFullScreenLayout:(bool)_wants {
    // Apple docs: When set to YES, the view controller's view extends
    // underneath the status bar and navigation bar. On real iOS this
    // affects layout insets. In touchHLE we always render full-screen,
    // so this is effectively a no-op but we accept it silently.
}
- (bool)wantsFullScreenLayout {
    // Always report YES since touchHLE renders games full-screen.
    true
}
- (())setExtendedLayoutIncludesOpaqueBars:(bool)value {
    // iOS 7+ layout hint: whether the extended layout includes the area
    // behind opaque bars. touchHLE renders full-screen and does not model
    // bar opacity insets, so we faithfully store the value (for the getter)
    // but it has no layout effect.
    env.objc
        .borrow_mut::<UIViewControllerHostObject>(this)
        .extended_layout_includes_opaque_bars = value;
}
- (bool)extendedLayoutIncludesOpaqueBars {
    env.objc
        .borrow::<UIViewControllerHostObject>(this)
        .extended_layout_includes_opaque_bars
}

// =========================================================================
// MARK: - edgesForExtendedLayout (iOS 7+)
// Reference: <https://developer.apple.com/documentation/uikit/uiviewcontroller/1621515-edgesforextendedlayout>
// =========================================================================

- (())setEdgesForExtendedLayout:(NSUInteger)edges {
    // UIRectEdge bitmask: specifies which edges the view controller's view
    // should extend beneath bars (status bar, navigation bar, tab bar, toolbar).
    // touchHLE renders full-screen without system bars, so this has no visual
    // effect, but we store the value faithfully for the getter.
    env.objc
        .borrow_mut::<UIViewControllerHostObject>(this)
        .edges_for_extended_layout = edges;
}

- (NSUInteger)edgesForExtendedLayout {
    env.objc
        .borrow::<UIViewControllerHostObject>(this)
        .edges_for_extended_layout
}

- (())dismissModalViewControllerAnimated:(bool)animated {
    // Apple docs: "If you call this method on the modal view controller
    // itself, it automatically forwards the message to the presenting view
    // controller." Forward the call up the chain when `self` does not own a
    // presented view controller.
    let presented = env.objc.borrow::<UIViewControllerHostObject>(this).presented_view_controller;
    if presented == nil {
        let presenter = env.objc.borrow::<UIViewControllerHostObject>(this).presenting_view_controller;
        if presenter != nil {
            return msg![env; presenter dismissModalViewControllerAnimated:animated];
        }
        log_dbg!(
            "[(UIViewController*){:?} dismissModalViewControllerAnimated:{}] no presented vc",
            this, animated
        );
        return;
    }

    () = msg![env; presented viewWillDisappear:animated];

    let presented_view: id = msg![env; presented view];
    if presented_view != nil {
        () = msg![env; presented_view removeFromSuperview];
    }

    () = msg![env; presented viewDidDisappear:animated];

    env.objc.borrow_mut::<UIViewControllerHostObject>(presented).presenting_view_controller = nil;
    env.objc.borrow_mut::<UIViewControllerHostObject>(this).presented_view_controller = nil;
    release(env, presented);
}
- (())dismissMoviePlayerViewControllerAnimated {
    log!("TODO: [(UIViewController*){:?} dismissMoviePlayerViewControllerAnimated]", this);
    // TODO
}

- (bool)shouldAutorotateToInterfaceOrientation:(UIInterfaceOrientation)interface_orientation {
    interface_orientation == 3 || interface_orientation == 4
}

- (id)nextResponder {
    let view = msg![env; this view];
    let next_responder = msg![env; view superview];
    log_dbg!("[(UIView*){:?} nextResponder] => {:?}", this, next_responder);
    next_responder
}

- (id)title {
    let stored_title = env.objc.borrow::<UIViewControllerHostObject>(this).title;
    if stored_title != nil {
        stored_title
    } else {
        let class: Class = msg![env; this class];
        NSStringFromClass(env, class)
    }
}

- (bool)isViewLoaded {
    env.objc.borrow::<UIViewControllerHostObject>(this).view != nil
}

- (id)nibName {
    let host = env.objc.borrow::<UIViewControllerHostObject>(this);
    if host.nib_name != nil {
        return host.nib_name;
    }

    // Если bundle равен nil, Apple использует [NSBundle mainBundle] для поиска
    let mut bundle: id = msg![env; this nibBundle];
    if bundle == nil {
        bundle = msg_class![env; NSBundle mainBundle];
    }

    let class: Class = msg![env; this class];
    let class_name: id = NSStringFromClass(env, class);

    let resolved = resolve_nib_name_from_class(env, bundle, class_name);
    release(env, class_name);

    if resolved != nil {
        retain(env, resolved);
        env.objc.borrow_mut::<UIViewControllerHostObject>(this).nib_name = resolved;
    }
    resolved
}

- (id)nibBundle {
    env.objc.borrow::<UIViewControllerHostObject>(this).bundle
}

- (())viewDidUnload {
    log_dbg!("[(UIViewController*){:?} viewDidUnload]", this);
}

- (())viewWillLayoutSubviews {
    log_dbg!("[(UIViewController*){:?} viewWillLayoutSubviews]", this);
}

- (())viewDidLayoutSubviews {
    log_dbg!("[(UIViewController*){:?} viewDidLayoutSubviews]", this);
}

- (())setEditing:(bool)editing animated:(bool)_animated {
    msg![env; this setEditing:editing]
}

- (id)editButtonItem {
    msg_class![env; UIBarButtonItem new]
}

- (())presentModalViewController:(id)modal_vc animated:(bool)animated {
    // Logged at info level so we can confirm modal presentation actually
    // happens at runtime even on builds without RUST_LOG=debug.
    log!(
        "[(UIViewController*){:?} presentModalViewController:{:?} animated:{}]",
        this, modal_vc, animated
    );
    if modal_vc == nil {
        return;
    }

    // The presenting view controller retains the presented one until it is
    // dismissed. The reverse pointer (`presenting_view_controller`) is a weak
    // back-reference per Apple's UIViewController documentation.
    retain(env, modal_vc);
    let host_obj = env.objc.borrow_mut::<UIViewControllerHostObject>(this);
    let old_modal = std::mem::replace(&mut host_obj.presented_view_controller, modal_vc);
    if old_modal != nil {
        // Stack-on-stack presentation is not supported here; just drop the
        // previous reference cleanly. Apps that need this should be fixed up
        // separately.
        log!(
            "WARNING: presentModalViewController: replacing existing presented vc {:?} on {:?}",
            old_modal, this
        );
        release(env, old_modal);
    }
    env.objc.borrow_mut::<UIViewControllerHostObject>(modal_vc).presenting_view_controller = this;

    // Trigger -loadView/-viewDidLoad if the view hasn't been instantiated yet.
    let modal_view: id = msg![env; modal_vc view];
    if modal_view == nil {
        log!(
            "WARNING: presentModalViewController: modal vc {:?} has no view",
            modal_vc
        );
        return;
    }

    // Locate a window to host the modal view. Prefer the presenting view's
    // window (matches Apple's behaviour of presenting on top of the existing
    // hierarchy). Fall back to the application's key window when the
    // presenting controller is not yet attached.
    let presenter_view: id = msg![env; this view];
    let mut window: id = if presenter_view != nil {
        msg![env; presenter_view window]
    } else {
        nil
    };
    if window == nil {
        let app: id = msg_class![env; UIApplication sharedApplication];
        window = msg![env; app keyWindow];
    }
    if window == nil {
        log!(
            "WARNING: presentModalViewController: no window found for {:?}",
            this
        );
        return;
    }

    () = msg![env; modal_vc viewWillAppear:animated];
    // UIWindow's -addSubview: applies the auto-rotation transform when the
    // view has an associated UIViewController and posts the appropriate
    // appearance hooks for the new view, so we route through the window
    // directly instead of adding the view to the presenter's view.
    () = msg![env; window addSubview:modal_view];
    () = msg![env; modal_vc viewDidAppear:animated];
}

- (id)modalViewController {
    env.objc.borrow::<UIViewControllerHostObject>(this).presented_view_controller
}

- (id)presentingViewController {
    env.objc.borrow::<UIViewControllerHostObject>(this).presenting_view_controller
}

- (id)presentedViewController {
    env.objc.borrow::<UIViewControllerHostObject>(this).presented_view_controller
}

- (())presentViewController:(id)vc
                  animated:(bool)animated
                completion:(id)_completion {
    msg![env; this presentModalViewController:vc animated:animated]
}

- (())dismissViewControllerAnimated:(bool)animated
                         completion:(id)_completion {
    msg![env; this dismissModalViewControllerAnimated:animated]
}

// Apple docs: The style used to transition the receiver's modal view controller
// (its modalViewController property) to the screen.
- (UIModalTransitionStyle)modalTransitionStyle {
    env.objc.borrow::<UIViewControllerHostObject>(this).modal_transition_style
}
- (())setModalTransitionStyle:(UIModalTransitionStyle)style {
    env.objc.borrow_mut::<UIViewControllerHostObject>(this).modal_transition_style = style;
}

// Apple docs: The presentation style for modally presented view controllers.
- (UIModalPresentationStyle)modalPresentationStyle {
    env.objc.borrow::<UIViewControllerHostObject>(this).modal_presentation_style
}
- (())setModalPresentationStyle:(UIModalPresentationStyle)style {
    env.objc.borrow_mut::<UIViewControllerHostObject>(this).modal_presentation_style = style;
}

// =========================================================================
// MARK: - Preferred content size (iOS 7+, replaces deprecated
//         contentSizeForViewInPopover)
// =========================================================================

- (CGSize)preferredContentSize {
    // Return zero — the caller (UIPopoverController) uses a default size when
    // this is zero.
    CGSize { width: 0.0, height: 0.0 }
}

- (())setPreferredContentSize:(CGSize)_size {
    // Stub — we don't render popovers, so we just swallow the value.
}

// Deprecated predecessor (iOS 3.2–7.0)
- (CGSize)contentSizeForViewInPopover {
    msg![env; this preferredContentSize]
}

- (())setContentSizeForViewInPopover:(CGSize)size {
    let _: () = msg![env; this setPreferredContentSize:size];
}

- (bool)hidesBottomBarWhenPushed {
    false
}

- (())setHidesBottomBarWhenPushed:(bool)_value {
    // TODO
}

- (id)tabBarItem {
    msg_class![env; UITabBarItem new]
}

- (())setTabBarItem:(id)_item {
    // TODO
}

- (id)tabBarController {
    nil
}

- (id)interfaceOrientation {
    nil
}

- (id)navigationItem {
    let existing = env.objc.borrow::<UIViewControllerHostObject>(this).navigation_item;
    if existing != nil {
        return existing;
    }
    // Match Apple: lazily create an item whose title is the controller's
    // current `title` (falling back to the class name when unset). Retain it
    // on the host object so subsequent property writes (e.g.
    // `self.navigationItem.rightBarButtonItem = ...`) are not lost.
    let title: id = msg![env; this title];
    let init_title = if title != nil {
        title
    } else {
        let class: Class = msg![env; this class];
        NSStringFromClass(env, class)
    };
    let item: id = msg_class![env; UINavigationItem alloc];
    let item: id = msg![env; item initWithTitle:init_title];
    env.objc.borrow_mut::<UIViewControllerHostObject>(this).navigation_item = item;
    item
}

- (())didReceiveMemoryWarning {
    log_dbg!("[(UIViewController*){:?} didReceiveMemoryWarning]", this);
    let view = env.objc.borrow::<UIViewControllerHostObject>(this).view;
    if view != nil {
        let superview: id = msg![env; view superview];
        if superview == nil {
            () = msg![env; this viewDidUnload];
            () = msg![env; this setView:nil];
        }
    }
}

- (bool)shouldAutorotate {
    true
}

- (NSUInteger)supportedInterfaceOrientations {
    // UIInterfaceOrientationMaskAll = 0xFF
    0xFF
}

- (UIInterfaceOrientation)preferredInterfaceOrientationForPresentation {
    3
}

- (id)childViewControllers {
    msg_class![env; NSArray new]
}

- (())addChildViewController:(id)_child {
    log_dbg!("[(UIViewController*){:?} addChildViewController:]", this);
}

- (())removeFromParentViewController {
    log_dbg!("[(UIViewController*){:?} removeFromParentViewController]", this);
}

- (())willMoveToParentViewController:(id)_parent {
    // TODO
}

- (())didMoveToParentViewController:(id)_parent {
    // TODO
}

- (())beginAppearanceTransition:(bool)_appearing animated:(bool)_animated {
    // TODO
}

- (())endAppearanceTransition {
    // TODO
}

- (bool)automaticallyForwardAppearanceAndRotationMethodsToChildViewControllers {
    true
}

- (bool)shouldAutomaticallyForwardAppearanceMethods {
    true
}

@end

// --- Умные заглушки для пропуска видео и камеры ---

@implementation VideoViewController: UIViewController

- (())viewDidAppear:(bool)animated {
    () = msg_super![env; this viewDidAppear:animated];
    log!("[HACK] VideoViewController auto-closing!");

    () = msg![env; this dismissModalViewControllerAnimated:false];
    let view: id = msg![env; this view];
    if view != nil {
        () = msg![env; view removeFromSuperview];
    }
}

@end

@implementation BarcodeReaderViewController: UIViewController

- (())viewDidAppear:(bool)animated {
    () = msg_super![env; this viewDidAppear:animated];
    log!("[HACK] BarcodeReaderViewController auto-closing!");

    () = msg![env; this dismissModalViewControllerAnimated:false];
    let view: id = msg![env; this view];
    if view != nil {
        () = msg![env; view removeFromSuperview];
    }
}

@end

};

fn check_and_resolve_nib(env: &mut Environment, bundle: id, base_name: id) -> id {
    if base_name == nil {
        return nil;
    }
    let type_: id = get_static_str(env, "nib");
    let base_name_str = to_rust_string(env, base_name);
    // Перебираем варианты регистра и суффиксов
    let bases = [base_name_str.to_string(), base_name_str.to_lowercase()];
    let suffixes = [
        "", "~iphone", "~ipad", "-iPhone", "-iPad", "_iPhone", "_iPad",
    ];
    for base in &bases {
        for suffix in &suffixes {
            let candidate = format!("{}{}", base, suffix);
            let candidate_ns: id = from_rust_string(env, candidate);

            // Проверяем существование файла
            let path: id = msg![env; bundle pathForResource:candidate_ns ofType:type_];
            if path != nil {
                release(env, path);
                return autorelease(env, candidate_ns);
            }
            release(env, candidate_ns);
        }
    }
    nil
}

fn resolve_nib_name_from_class(env: &mut Environment, bundle: id, class_name: id) -> id {
    if class_name == nil {
        return nil;
    }

    // 1. Пробуем полное имя класса (напр. MainViewController)
    let res = check_and_resolve_nib(env, bundle, class_name);
    if res != nil {
        return res;
    }

    // 2. Пробуем имя без суффикса "Controller" (напр. MainView)
    let class_str = to_rust_string(env, class_name);
    if let Some(short_name) = class_str.strip_suffix("Controller") {
        let short_ns = from_rust_string(env, short_name.to_string());
        let res = check_and_resolve_nib(env, bundle, short_ns);
        release(env, short_ns);
        if res != nil {
            return res;
        }
    }

    nil
}

// Helpers used by `ui_storyboard.rs` to attach storyboard state to a
// freshly-instantiated view controller. They live here so the
// `UIViewControllerHostObject` private fields don't have to be exposed.

/// Attach the storyboardc directory (bundle-relative) that should be
/// consulted when the view controller's `-loadView` looks up its view
/// nib. Promoted from a `UIStoryboardPlaceholder` that was set as the
/// view via the scene nib's outlet connection.
pub(crate) fn promote_storyboard_placeholder(
    env: &mut Environment,
    vc: id,
    storyboardc_relative_dir: &str,
) {
    let host = env.objc.borrow_mut::<UIViewControllerHostObject>(vc);
    if host.storyboard_view_nib_dir.is_empty() {
        host.storyboard_view_nib_dir = storyboardc_relative_dir.to_string();
    }
}

/// Set the `UIStoryboard*` back-pointer on the view controller. The
/// storyboard is retained so the view controller can be returned without
/// the user having to keep a reference to it themselves.
pub(crate) fn set_storyboard(env: &mut Environment, vc: id, storyboard: id) {
    if storyboard != nil { retain(env, storyboard); }
    let host = env.objc.borrow_mut::<UIViewControllerHostObject>(vc);
    let old = host.storyboard;
    host.storyboard = storyboard;
    if old != nil { release(env, old); }
}
