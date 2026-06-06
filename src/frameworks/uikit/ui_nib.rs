/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//!
//! `UINib` and loading of nib files.
//!
//! Resources:
//! - Apple's [Resource Programming Guide](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/LoadingResources/CocoaNibs/CocoaNibs.html) is very helpful.
//!
//! - GitHub user 0xced's [reverse-engineering of UIClassSwapper](https://gist.github.com/0xced/45daf79b62ad6a20be1c).

use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::foundation::ns_string::{get_static_str, to_rust_string};
use crate::frameworks::foundation::{ns_string, NSUInteger};
use crate::frameworks::uikit::ui_view::ui_control::UIControlEvents;
use crate::fs::GuestPathBuf;
use crate::mem::ConstVoidPtr;
use crate::objc::{
    autorelease, id, impl_HostObject_with_superclass, msg, msg_class, msg_super, nil, objc_classes,
    release, retain, Class, ClassExports, HostObject,
};
use crate::Environment;

// Per Apple's UINib loading documentation, the `options` dictionary passed to
// `-[UINib instantiateWithOwner:options:]` recognises a single documented key:
// `UINibExternalObjects`. Older binaries (FaceFighter and a handful of other
// iPhone OS 3.x apps) instead import the private `UINibProxiedObjectsKey`
// constant; if it's missing the dyld stub silently resolves to NULL and the
// app crashes when looking up its proxied objects. Expose the constant as a
// host NSString so dyld can fix up the non-lazy import.
pub const UINibProxiedObjectsKey: &str = "UINibProxiedObjectsKey";
pub const UINibExternalObjects: &str = "UINibExternalObjects";

pub const CONSTANTS: ConstantExports = &[
    (
        "_UINibProxiedObjectsKey",
        HostConstant::NSString(UINibProxiedObjectsKey),
    ),
    (
        "_UINibExternalObjects",
        HostConstant::NSString(UINibExternalObjects),
    ),
];

#[derive(Default)]
struct UINibHostObject {
    /// `NSString*`
    nib_name: id,
    /// `NSBundle*`
    bundle: id,
    /// File's Owner (weak, non-retaining)
    file_owner: id,
    /// External objects table (NSDictionary<NSString*, id>*), set during
    /// `-instantiateWithOwner:options:` when the caller passes
    /// `UINibExternalObjects` / `UINibProxiedObjectsKey`. Non-retained:
    /// the caller owns the dictionary for the duration of the call.
    external_objects: id,
}
impl HostObject for UINibHostObject {}

#[derive(Default)]
struct UIRuntimeConnectionHostObject {
    destination: id,
    label: id,
    source: id,
}
impl HostObject for UIRuntimeConnectionHostObject {}

#[derive(Default)]
struct UIRuntimeEventConnectionHostObject {
    superclass: UIRuntimeConnectionHostObject,
    event_mask: UIControlEvents,
}
impl_HostObject_with_superclass!(UIRuntimeEventConnectionHostObject);

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UINib: NSObject

+ (id)nibWithNibName:(id)nib_name bundle:(id)bundle {
    let main_bundle = msg_class![env; NSBundle mainBundle];
    let bundle: id = if bundle == nil {
        main_bundle
    } else {
        // TODO: non-main bundles
        assert_eq!(bundle, main_bundle);
        bundle
    };

    retain(env, nib_name);
    retain(env, bundle);
    let host_object = Box::new(UINibHostObject {
        nib_name,
        bundle,
        file_owner: nil,
        external_objects: nil,
    });

    let new = env.objc.alloc_object(this, host_object, &mut env.mem);
    autorelease(env, new)
}

- (())dealloc {
    let &UINibHostObject {
        nib_name,
        bundle,
        ..
    } = env.objc.borrow(this);

    release(env, nib_name);
    release(env, bundle);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)instantiateWithOwner:(id)owner options:(id)options {
    assert!(owner != nil);

    // Apple's UINib loading documentation: the only documented `options`
    // key is `UINibExternalObjects`; many older binaries also use the
    // private `UINibProxiedObjectsKey`. Both map proxy identifiers to
    // real objects so that `UIProxyObject` references in the nib resolve
    // to host-supplied placeholders (e.g. for storyboard view loading).
    let mut external_objects: id = nil;
    if options != nil {
        let key_proxied = get_static_str(env, UINibProxiedObjectsKey);
        external_objects = msg![env; options objectForKey:key_proxied];
        if external_objects == nil {
            let key_external = get_static_str(env, UINibExternalObjects);
            external_objects = msg![env; options objectForKey:key_external];
        }
    }

    let bundle = env.objc.borrow::<UINibHostObject>(this).bundle;
    let nib_name = env.objc.borrow::<UINibHostObject>(this).nib_name;
    let type_: id = get_static_str(env, "nib");

    let mut path: id = msg![env; bundle pathForResource:nib_name ofType:type_];

    // Фолбэк 1: Имя уже может содержать расширение, ищем без указания типа
    if path == nil {
        path = msg![env; bundle pathForResource:nib_name ofType:nil];
    }

    // Фолбэк 2: Автоматический суффикс ~iphone (Universal Apps)
    if path == nil {
        let rust_nib_name = to_rust_string(env, nib_name);
        let iphone_name = format!("{}~iphone", rust_nib_name);
        let iphone_nss = ns_string::from_rust_string(env, iphone_name);
        path = msg![env; bundle pathForResource:iphone_nss ofType:type_];
    }

    // Фолбэк 3: Автоматический суффикс ~ipad
    if path == nil {
        let rust_nib_name = to_rust_string(env, nib_name);
        let ipad_name = format!("{}~ipad", rust_nib_name);
        let ipad_nss = ns_string::from_rust_string(env, ipad_name);
        path = msg![env; bundle pathForResource:ipad_nss ofType:type_];
    }

    if path == nil {
        log!("Warning: UINib instantiateWithOwner: nib file {:?} not found", to_rust_string(env, nib_name));
        return nil;
    }

    let nib_path = to_rust_string(env, path).to_string();
    assert!(env.objc.borrow::<UINibHostObject>(this).file_owner == nil);
    {
        let host = env.objc.borrow_mut::<UINibHostObject>(this);
        host.file_owner = owner;
        host.external_objects = external_objects;
    }

    let top_level_objects = if let Ok(unarchiver) = load_nib_file(env, this, GuestPathBuf::from(nib_path)) {
        let top_level_objects_key = get_static_str(env, "UINibTopLevelObjectsKey");
        let objects = msg![env; unarchiver decodeObjectForKey:top_level_objects_key];

        // Удерживаем объекты ДО удаления анрайхиватора, иначе они могут
        // вычиститься
        if objects != nil {
            retain(env, objects);
        }
        release(env, unarchiver);

        if objects != nil {
            autorelease(env, objects)
        } else {
            nil
        }
    } else {
        nil
    };

    {
        let host = env.objc.borrow_mut::<UINibHostObject>(this);
        host.file_owner = nil;
        host.external_objects = nil;
    }
    top_level_objects
}

@end

@implementation UIProxyObject: NSObject

- (id)initWithCoder:(id)coder {
    let id_key = get_static_str(env, "UIProxiedObjectIdentifier");
    let id_nss: id = msg![env; coder decodeObjectForKey:id_key];
    let id = to_rust_string(env, id_nss).into_owned();

    // Try the external objects table first. This is how storyboards (and
    // anything passing `UINibExternalObjects`/`UINibProxiedObjectsKey` to
    // `-[UINib instantiateWithOwner:options:]`) supply real instances for
    // arbitrary proxy identifiers — most notably `UIStoryboardPlaceholder`.
    let delegate: id = msg![env; coder delegate];
    if delegate != nil {
        let external = env.objc.borrow::<UINibHostObject>(delegate).external_objects;
        if external != nil {
            let replacement: id = msg![env; external objectForKey:id_nss];
            if replacement != nil {
                release(env, this);
                return retain(env, replacement);
            }
        }
    }

    if id == "IBFilesOwner" || id.starts_with("UpstreamPlaceholder-") {
        // `UpstreamPlaceholder-*` identifiers are emitted by Xcode's
        // storyboard compiler for connections that reach *out* of a
        // scene nib into its enclosing context — typically the view
        // controller that owns the view nib being loaded. The runtime
        // passes that controller as the nib's file's owner, so the
        // upstream placeholder and `IBFilesOwner` resolve to the same
        // object.
        if delegate != nil {
            let file_owner = env.objc.borrow::<UINibHostObject>(delegate).file_owner;
            if file_owner != nil {
                release(env, this);
                return retain(env, file_owner);
            }
        }

        log!("touchHLE Warning: {} requested but file_owner is nil! Returning dummy.", id);
        let ns_object_class = env.objc.get_known_class("NSObject", &mut env.mem);
        let dummy: id = msg![env; ns_object_class alloc];
        release(env, this);
        msg![env; dummy init]
    } else if id == "IBFirstResponder" {
        log!("touchHLE: Bypassing IBFirstResponder replacement with dummy NSObject");
        let proxy_class = env.objc.get_known_class("NSObject", &mut env.mem);
        let dummy: id = msg![env; proxy_class alloc];
        let dummy_init: id = msg![env; dummy init];
        release(env, this);
        dummy_init
    } else {
        log!("TODO: UIProxyObject replacement for {}, instance {:?} left unreplaced", id, this);
        this
    }
}

@end

@implementation UIClassSwapper: NSObject

- (id)initWithCoder:(id)coder {
    let name_key = get_static_str(env, "UIClassName");
    let name_nss: id = msg![env; coder decodeObjectForKey:name_key];
    let name = to_rust_string(env, name_nss);

    let orig_key = get_static_str(env, "UIOriginalClassName");
    let orig_nss: id = msg![env; coder decodeObjectForKey:orig_key];
    let orig = to_rust_string(env, orig_nss);

    log!("[DEBUG NIB] UIClassSwapper loading class: {} (original: {})", name, orig);

    // Use try_get_known_class so the lookup returns None instead of
    // panicking if the app references a custom class (e.g. FirstViewController
    // in Inotia3) that has no host implementation and isn't registered in the
    // app binary. We then fall back to the NIB's original class.
    let selected_class = {
        let problematic_views = ["FBLoginButton"];
        let mut c = if problematic_views.iter().any(|&prob| name == prob) {
            log!("[DEBUG NIB] Warning: Substituting {} with generic UIView", name);
            None
        } else {
            env.objc.try_get_known_class(&name, &mut env.mem)
        };

        if c.is_none() {
            log!("[DEBUG NIB] Warning: Custom class {} not found. Falling back to original: {}", name, orig);
            c = env.objc.try_get_known_class(&orig, &mut env.mem);
        }

        if c.is_none() {
            log!("[DEBUG NIB] Warning: Original class {} not found either. Falling back to UIView.", orig);
            c = env.objc.try_get_known_class("UIView", &mut env.mem);
        }

        c.unwrap_or_else(|| {
            log!("[DEBUG NIB] CRITICAL: Fallback class not found! Falling back to NSObject.");
            env.objc.get_known_class("NSObject", &mut env.mem)
        })
    };

    let object: id = msg![env; selected_class alloc];

    // ВАЖНО: Всегда используем initWithCoder:, кроме тех случаев, когда это
    // чисто кастомный плейсхолдер Interface Builder
    // Инициализация системных UIViewController через 'init' оставляет их
    // сломанными и ведет к NULL-PAGE READ.
    let object: id = if orig == "UICustomObject" {
        msg![env; object init]
    } else {
        msg![env; object initWithCoder:coder]
    };

    release(env, this);
    object
}

@end

@implementation UIRuntimeConnection: NSObject

+ (id)alloc {
    let host_object = Box::<UIRuntimeConnectionHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithCoder:(id)coder {
    let destination_key = get_static_str(env, "UIDestination");
    let destination: id = msg![env; coder decodeObjectForKey: destination_key];

    let label_key = get_static_str(env, "UILabel");
    let label: id = msg![env; coder decodeObjectForKey: label_key];

    let source_key = get_static_str(env, "UISource");
    let source: id = msg![env; coder decodeObjectForKey: source_key];

    retain(env, destination);
    retain(env, source);
    retain(env, label);

    let host_obj = env.objc.borrow_mut::<UIRuntimeConnectionHostObject>(this);
    host_obj.destination = destination;
    host_obj.label = label;
    host_obj.source = source;
    this
}

- (())dealloc {
    let &UIRuntimeConnectionHostObject { destination, label, source } = env.objc.borrow(this);
    release(env, destination);
    release(env, label);
    release(env, source);
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

@implementation UIRuntimeEventConnection: UIRuntimeConnection

+ (id)alloc {
    let host_object = Box::<UIRuntimeEventConnectionHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())connect {
    let &UIRuntimeConnectionHostObject { destination, label, source } = env.objc.borrow(this);
    let &UIRuntimeEventConnectionHostObject { superclass: _, event_mask } = env.objc.borrow(this);

    if source != nil && destination != nil && label != nil {
        let selector = to_rust_string(env, label).into_owned();
        // `lookup_selector` only finds selectors that the binary or some
        // already-loaded host class has registered. Storyboard-only
        // selectors (e.g. an `IBAction` that is reached purely through a
        // nib connection and never appears as a literal `@selector(...)`
        // in the binary's selref section) are not guaranteed to be
        // pre-registered, so fall back to allocating a new SEL on the
        // fly. The selector string is owned by the connection's label
        // NSString, but `register_host_selector` makes its own C copy.
        let action = env
            .objc
            .lookup_selector(&selector)
            .unwrap_or_else(|| env.objc.register_host_selector(selector.clone(), &mut env.mem));
        log!(
            "UIRuntimeEventConnection: [{:?} addTarget:{:?} action:{} forControlEvents:{:#x}]",
            source,
            destination,
            selector,
            event_mask,
        );
        () = msg![env; source addTarget:destination action:action forControlEvents:event_mask];
    } else {
        log!(
            "Warning: UIRuntimeEventConnection skipping connect — source={:?} destination={:?} label={:?}",
            source,
            destination,
            label,
        );
    }
}

- (id)initWithCoder:(id)coder {
    let this: id = msg_super![env; this initWithCoder: coder];
    let event_mask_key = get_static_str(env, "UIEventMask");
    let event_mask: i32 = msg![env; coder decodeIntForKey: event_mask_key];

    let host_obj = env.objc.borrow_mut::<UIRuntimeEventConnectionHostObject>(this);
    host_obj.event_mask = event_mask as UIControlEvents;
    this
}

- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

@implementation UIRuntimeOutletConnection: UIRuntimeConnection

- (())connect {
    let &UIRuntimeConnectionHostObject { destination, label, source } = env.objc.borrow(this);

    if source != nil && destination != nil && label != nil {
        // ЯВНО УКАЗЫВАЕМ ТИП Class, чтобы компилятор не ругался
        let source_class: Class = msg![env; source class];
        let ns_object_class = env.objc.get_known_class("NSObject", &mut env.mem);

        // Предотвращаем краш KVC (Key-Value Coding), если source — это просто
        // заглушка NSObject
        if source_class == ns_object_class {
            let label_str = to_rust_string(env, label);
            log!("touchHLE NIB: Skipping outlet '{}' connection because source is an unhandled NSObject", label_str);
            return;
        }

        () = msg![env; source setValue:destination forKey:label];
    }
}

@end

// UIRuntimeOutletCollectionConnection is used by NIBs compiled with Xcode 4+
// for IBOutletCollection properties. Unlike a regular outlet (single object),
// an outlet collection appends its destination to an NSMutableArray stored
// under the given key on the source object.
//
// Apple's runtime implementation:
// 1. Gets the existing array via [source valueForKey:label]
// 2. If nil, creates a new NSMutableArray and sets it via setValue:forKey:
// 3. Adds the destination to the array via addObject:
//
// Reference: https://developer.apple.com/library/archive/documentation/General/Conceptual/CocoaEncyclopedia/OutletCollections/OutletCollections.html
@implementation UIRuntimeOutletCollectionConnection: UIRuntimeConnection

+ (id)alloc {
    let host_object = Box::<UIRuntimeConnectionHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())connect {
    let &UIRuntimeConnectionHostObject { destination, label, source } = env.objc.borrow(this);

    if source == nil || destination == nil || label == nil {
        return;
    }

    let label_str = to_rust_string(env, label);
    log_dbg!(
        "UIRuntimeOutletCollectionConnection: connecting outlet collection '{}' on {:?} -> {:?}",
        label_str, source, destination
    );

    // Try to get the existing array for this key
    let existing_array: id = msg![env; source valueForKey:label];

    if existing_array != nil {
        // Array already exists, just add the destination to it
        () = msg![env; existing_array addObject:destination];
    } else {
        // Create a new NSMutableArray, add the destination, and set it
        let new_array: id = msg_class![env; NSMutableArray new];
        () = msg![env; new_array addObject:destination];
        () = msg![env; source setValue:new_array forKey:label];
        release(env, new_array);
    }
}

- (())dealloc {
    let &UIRuntimeConnectionHostObject { destination, label, source } = env.objc.borrow(this);
    release(env, destination);
    release(env, label);
    release(env, source);
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

};

fn load_nib_file(env: &mut Environment, ui_nib: id, path: GuestPathBuf) -> Result<id, ()> {
    let path_str = ns_string::from_rust_string(env, path.as_str().to_string());
    let mut ns_data: id = msg_class![env; NSData dataWithContentsOfFile:path_str];

    // Если NSData вернул nil (скорее всего путь указывает на директорию),
    // ищем внутри скомпилированный файл keyedobjects.nib
    if ns_data == nil {
        let keyed_path = format!("{}/keyedobjects.nib", path.as_str());
        let keyed_path_str = ns_string::from_rust_string(env, keyed_path);
        ns_data = msg_class![env; NSData dataWithContentsOfFile:keyed_path_str];
    }

    // Современный формат, используемый storyboards и xibs из iOS 8+: внутри
    // NIB-каталога лежит файл `objects-8.0+.nib`. Попробуем его раньше
    // старого `objects.nib`, потому что новые проекты часто содержат и тот
    // и другой, и формат 8.0+ предпочтительнее.
    if ns_data == nil {
        let modern_path = format!("{}/objects-8.0+.nib", path.as_str());
        let modern_path_str = ns_string::from_rust_string(env, modern_path);
        ns_data = msg_class![env; NSData dataWithContentsOfFile:modern_path_str];
    }

    // Готовая «рантайм»-копия (используется ibtool при компиляции
    // storyboardc — обычно идентичная objects-8.0+.nib).
    if ns_data == nil {
        let runtime_path = format!("{}/runtime.nib", path.as_str());
        let runtime_path_str = ns_string::from_rust_string(env, runtime_path);
        ns_data = msg_class![env; NSData dataWithContentsOfFile:runtime_path_str];
    }

    // Запасной старый формат - objects.nib
    if ns_data == nil {
        let objects_path = format!("{}/objects.nib", path.as_str());
        let objects_path_str = ns_string::from_rust_string(env, objects_path);
        ns_data = msg_class![env; NSData dataWithContentsOfFile:objects_path_str];
    }

    if ns_data == nil {
        log!("Warning: couldn't load nib file {:?}", path);
        return Err(());
    };

    let len: NSUInteger = msg![env; ns_data length];
    // Если длина файла достаточна, значит указатель bytes гарантированно
    // существует
    if len < 10 {
        return Err(());
    }

    let bytes: ConstVoidPtr = msg![env; ns_data bytes];

    // ... дальше без изменений, начиная с let unarchiver = ...

    let unarchiver = if env.mem.bytes_at(bytes.cast(), 10) == b"NIBArchive" {
        let decoder: id = msg_class![env; _touchHLE_NIBArchiveDecoder alloc];
        msg![env; decoder _touchHLE_initForReadingWithData:ns_data]
    } else {
        let unarchiver = msg_class![env; NSKeyedUnarchiver alloc];
        msg![env; unarchiver initForReadingWithData:ns_data]
    };

    () = msg![env; unarchiver setDelegate:ui_nib];

    let objects_key = get_static_str(env, "UINibObjectsKey");
    let objects: id = msg![env; unarchiver decodeObjectForKey:objects_key];

    if objects != nil {
        retain(env, objects); // Защита от преждевременного удаления
    }

    let conns_key = get_static_str(env, "UINibConnectionsKey");
    let conns: id = msg![env; unarchiver decodeObjectForKey:conns_key];
    if conns != nil {
        let conns_count: NSUInteger = msg![env; conns count];
        for i in 0..conns_count {
            let conn: id = msg![env; conns objectAtIndex:i];
            if conn != nil {
                () = msg![env; conn connect];
            }
        }
    }

    if objects != nil {
        let enumerator: id = msg![env; objects objectEnumerator];
        if enumerator != nil {
            loop {
                let next: id = msg![env; enumerator nextObject];
                if next == nil {
                    break;
                }
                () = msg![env; next awakeFromNib];
            }
        }
    }

    let visibles_key = get_static_str(env, "UINibVisibleWindowsKey");
    let visibles: id = msg![env; unarchiver decodeObjectForKey:visibles_key];
    if visibles != nil {
        let visibles_count: NSUInteger = msg![env; visibles count];
        for i in 0..visibles_count {
            let visible: id = msg![env; visibles objectAtIndex:i];
            if visible != nil {
                () = msg![env; visible setHidden:false];
            }
        }
    }

    Ok(unarchiver)
}
