/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//!
//! `NSObject`, the root of most class hierarchies in Objective-C.

use super::ns_dictionary::dict_from_keys_and_objects;
use super::ns_run_loop::NSDefaultRunLoopMode;
use super::ns_string::{from_rust_string, get_static_str, to_rust_string};
use super::{NSTimeInterval, NSUInteger};
// ДОБАВЛЕНЫ ИМПОРТЫ ДЛЯ ЭКСПОРТА ФУНКЦИИ И ОКРУЖЕНИЯ
use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::foundation::ns_thread::detach_new_thread_inner;
use crate::libc::semaphore::{host_create_semaphore, host_destroy_semaphore, sem_post, sem_wait};
use crate::mem::MutVoidPtr;
use crate::objc::{
    autorelease, id, msg, msg_class, msg_send, msg_send_no_type_checking, nil, objc_classes,
    retain, Class, ClassExports, NSZonePtr, ObjC, TrivialHostObject, SEL,
};
use crate::Environment;

// Хранилище для отмененных таймеров (target, имя селектора в виде строки)
pub static mut CANCELLED_PERFORMS: std::vec::Vec<(u32, std::option::Option<std::string::String>)> =
    std::vec::Vec::new();

// Хранилище для динамических свойств KVC (когда NIB устанавливает кастомные IBOutlet на базовые классы)
pub static mut DYNAMIC_KVC_STORAGE: std::vec::Vec<(u32, std::string::String, u32)> =
    std::vec::Vec::new();

// Side-channel storage for `performSelectorOnMainThread:withObject:waitUntilDone:YES` requests
// scheduled from background threads. Each entry maps a pending NSTimer's id to the host semaphore
// the background thread is blocked on, so that `_touchHLE_timerFireMethod:` can post the
// semaphore once the selector has finished running on the main thread. Without this, the
// `waitUntilDone:YES` argument is effectively ignored and background threads race ahead of the
// scheduled selector — this manifests, for example, as Call of Duty: Zombies' Marmalade-based
// `RunOnMainThread` helper clobbering `s3eAppDelegate.m_Func` repeatedly before the main thread's
// `-[s3eAppDelegate Functor]` fires, eventually loading a NULL function pointer and crashing.
pub static mut SYNC_PERFORM_SEMAPHORES: std::vec::Vec<(u32, u32)> = std::vec::Vec::new();

// KVO (Key-Value Observing) storage.
// Each entry: (observed_object_bits, observer_bits, keyPath string, options, context_bits)
pub static mut KVO_OBSERVERS: std::vec::Vec<(u32, u32, std::string::String, u32, u32)> =
    std::vec::Vec::new();

// ДОБАВЛЕНА РЕАЛИЗАЦИЯ NSAllocateObject
fn NSAllocateObject(
    env: &mut Environment,
    class: Class,
    extra_bytes: NSUInteger,
    _zone: NSZonePtr,
) -> id {
    if extra_bytes > 0 {
        log!(
            "Warning: NSAllocateObject called with extra_bytes={}, which is currently unhandled!",
            extra_bytes
        );
    }

    // Перенаправляем вызов в стандартный метод alloc данного класса
    msg![env; class alloc]
}

// ДОБАВЛЕН ЭКСПОРТ ФУНКЦИЙ ДЛЯ ДИНАМИЧЕСКОГО ЛИНКЕРА
pub const FUNCTIONS: FunctionExports = &[export_c_func!(NSAllocateObject(_, _, _))];

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSObject

+ (id)alloc {
    msg![env; this allocWithZone:(MutVoidPtr::null())]
}
+ (id)allocWithZone:(NSZonePtr)_zone {
    log_dbg!("[{:?} allocWithZone:]", this);
    env.objc.alloc_object(this, Box::new(TrivialHostObject), &mut env.mem)
}

+ (id)new {
    let new_object: id = msg![env; this alloc];
    msg![env; new_object init]
}

+ (Class)class {
    this
}
+ (bool)isSubclassOfClass:(Class)class {
    env.objc.class_is_subclass_of(this, class)
}

+ (id)retain {
    this
}
+ (())release {
}
+ (())autorelease {
}

+ (())layoutSubviews {
}

// Per Apple's [`+initialize`
// docs](https://developer.apple.com/documentation/objectivec/nsobject/1418639-initialize),
// the runtime sends this exactly once to every class before it receives any
// other message. Subclasses commonly chain to `[super initialize]`; if no
// class in the inheritance chain implemented +initialize then the dispatch
// would fall off the top of the chain and print a `superclass does not
// respond to selector "initialize"` warning (this used to break
// `ASIHTTPRequest` + initialize, which calls `[super initialize]`).
// Provide an explicit no-op at the root so `[super initialize]` is always
// safe.
+ (())initialize {
}

+ (bool)instancesRespondToSelector:(SEL)selector {
    env.objc.class_has_method(this, selector)
}

// ИЗМЕНЕНО: Ищем _objc_msgSend через create_proc_address (без логов)
+ (u32)instanceMethodForSelector:(SEL)selector {
    let dyld = &mut env.dyld;
    let mem = &mut env.mem;
    let cpu = &mut env.cpu;
    match dyld.create_proc_address(mem, cpu, "_objc_msgSend") {
        Ok(guest_func) => guest_func.addr_with_thumb_bit(),
        Err(_) => {
            log!("Error: _objc_msgSend not found! Returning dummy IMP.");
            let ptr: crate::mem::MutPtr<u16> = mem.alloc(2).cast();
            mem.write(ptr, 0x4770);
            ptr.to_bits() | 1
        }
    }
}

+ (id)instanceMethodSignatureForSelector:(SEL)selector {
    let sig: id = msg_class![env; NSMethodSignature signatureWithObjCTypes:(MutVoidPtr::null())];
    let sel_str = selector.as_str(&env.mem);
    let explicit_args = sel_str.chars().filter(|&c| c == ':').count() as NSUInteger;
    let total_args = explicit_args + 2;
    () = msg![env; sig _touchHLE_setNumberOfArguments:total_args];
    sig
}

+ (bool)accessInstanceVariablesDirectly {
    true
}

// Методы класса
+ (id)description {
    let name = env.objc.get_class_name(this);
    let str = from_rust_string(env, name.to_string());
    autorelease(env, str)
}

+ (id)debugDescription {
    msg![env; this description]
}

+ (())cancelPreviousPerformRequestsWithTarget:(id)target
                                     selector:(SEL)selector
                                       object:(id)object {
    let sel_str = selector.as_str(&env.mem).to_string();
    unsafe {
        crate::frameworks::foundation::ns_object::CANCELLED_PERFORMS.push((target.to_bits(), Some(sel_str)));
    }
}

+ (())cancelPreviousPerformRequestsWithTarget:(id)target {
    unsafe {
        crate::frameworks::foundation::ns_object::CANCELLED_PERFORMS.push((target.to_bits(), None));
    }
}

- (id)init {
    this
}

// ИСПРАВЛЕНИЕ: Добавлены методы ЭКЗЕМПЛЯРА description и debugDescription
- (id)description {
    let class: Class = msg![env; this class];
    let name = env.objc.get_class_name(class);
    // Формируем классическую строку вида <ClassName: 0xAddress>
    let desc_str = format!("<{}: 0x{:x}>", name, this.to_bits());
    let str = from_rust_string(env, desc_str);
    autorelease(env, str)
}

- (id)debugDescription {
    msg![env; this description]
}

- (NSUInteger)retainCount {
    env.objc.get_refcount(this).into()
}

- (id)retain {
    log_dbg!("[{:?} retain]", this);
    env.objc.increment_refcount(this);
    this
}
- (())release {
    log_dbg!("[{:?} release]", this);
    if env.objc.decrement_refcount(this) {
        () = msg![env; this dealloc];
    }
}
- (id)autorelease {
    () = msg_class![env; NSAutoreleasePool addObject:this];
    this
}

- (())dealloc {
    log_dbg!("[{:?} dealloc]", this);

    // Очищаем и высвобождаем динамические свойства KVC
    let mut to_release = Vec::new();
    unsafe {
        let target_bits = this.to_bits();
        DYNAMIC_KVC_STORAGE.retain(|entry| {
            if entry.0 == target_bits {
                if entry.2 != 0 {
                    to_release.push(entry.2);
                }
                false
            } else {
                true
            }
        });
    }

    for val_bits in to_release {
        let val: id = unsafe { std::mem::transmute(val_bits) };
        let _: () = msg![env; val release];
    }

    env.objc.dealloc_object(this, &mut env.mem)
}

- (Class)class {
    ObjC::read_isa(this, &env.mem)
}
- (bool)isMemberOfClass:(Class)class {
    let this_class: Class = msg![env; this class];
    class == this_class
}
- (bool)isKindOfClass:(Class)class {
    let this_class: Class = msg![env; this class];
    env.objc.class_is_subclass_of(this_class, class)
}

- (NSUInteger)hash {
    this.to_bits()
}

- (bool)isEqual:(id)other {
    this == other
}

- (id)copy {
    msg![env; this copyWithZone:(MutVoidPtr::null())]
}

- (id)mutableCopy {
    msg![env; this mutableCopyWithZone:(MutVoidPtr::null())]
}

- (())setValue:(id)value forKey:(id)key {
    if key == nil {
        log_dbg!("setValue:forKey: — key is nil, ignored");
        return;
    }

    let key_string = to_rust_string(env, key);
    if key_string.is_empty() || !key_string.is_ascii() {
        log!("Warning: setValue:forKey: key {:?} is empty or non-ASCII — calling setValue:forUndefinedKey:", key_string);
        let sel = env
            .objc
            .register_host_selector("setValue:forUndefinedKey:".to_string(), &mut env.mem);
        let _: () = msg_send(env, (this, sel, value, key));
        return;
    }

    let camel_case_key_string = format!(
        "{}{}",
        key_string.as_bytes()[0].to_ascii_uppercase() as char,
        &key_string[1..]
    );
    let class = msg![env; this class];

    if value == nil {
        log_dbg!("setValue:forKey: value is nil for key {:?} — calling setNilValueForKey:", key_string);
        if let Some(sel) = env.objc.lookup_selector(&format!("set{camel_case_key_string}:")) {
            if env.objc.class_has_method(class, sel) {
                let _: () = msg_send(env, (this, sel, value));
                return;
            }
        }
        // Per Apple's KVC contract: when -setValue:forKey: receives nil for
        // a key that maps to a scalar (non-object) property, it must call
        // -setNilValueForKey: on the receiver. The default NSObject impl of
        // -setNilValueForKey: raises NSInvalidArgumentException. We register
        // the selector defensively so the lookup never returns None even if
        // no host class ever defined the method directly.
        let sel = env
            .objc
            .register_host_selector("setNilValueForKey:".to_string(), &mut env.mem);
        let _: () = msg_send(env, (this, sel, key));
        return;
    }

    let value_class = msg![env; value class];
    let ns_value_class = env.objc.get_known_class("NSValue", &mut env.mem);
    if env.objc.class_is_subclass_of(value_class, ns_value_class) {
        log_dbg!(
            "setValue:forKey: value {:?} is NSValue subclass for key {:?} — proceeding",
            value, key_string
        );
    }

    if let Some(sel) = env.objc.lookup_selector(&format!("set{camel_case_key_string}:")) {
        if env.objc.class_has_method(class, sel) {
            let _: () = msg_send(env, (this, sel, value));
            return;
        }
    }

    if let Some(sel) = env.objc.lookup_selector(&format!("_set{camel_case_key_string}:")) {
        if env.objc.class_has_method(class, sel) {
            let _: () = msg_send(env, (this, sel, value));
            return;
        }
    }

    let access_sel = env
        .objc
        .register_host_selector("accessInstanceVariablesDirectly".to_string(), &mut env.mem);
    let access_ivars: bool = msg_send(env, (class, access_sel));
    if access_ivars {
        if let Some(ivar_ptr) = env.objc
            .object_lookup_ivar(&env.mem, this, &format!("_{key_string}"))
            .or_else(|| env.objc.object_lookup_ivar(&env.mem, this, &format!("_is{camel_case_key_string}")))
            .or_else(|| env.objc.object_lookup_ivar(&env.mem, this, &format!("{key_string}")))
            .or_else(|| env.objc.object_lookup_ivar(&env.mem, this, &format!("is{camel_case_key_string}")))
        {
            retain(env, value);
            env.mem.write(ivar_ptr.cast(), value);
            return;
        }
    }

    let undef_sel = env
        .objc
        .register_host_selector("setValue:forUndefinedKey:".to_string(), &mut env.mem);
    let _: () = msg_send(env, (this, undef_sel, value, key));
}


- (())setValue:(id)value forUndefinedKey:(id)key {
    let class: Class = ObjC::read_isa(this, &env.mem);
    let class_name_string = env.objc.get_class_name(class).to_owned();
    let key_string = to_rust_string(env, key);

    log!("Warning: Object {:?} of class {:?} does not have a setter for {} — storing dynamically",
        this, class_name_string, key_string);

    // Честно сохраняем значение и делаем retain
    if value != nil {
        retain(env, value);
    }

    unsafe {
        let target_bits = this.to_bits();
        let mut found = false;
        for entry in DYNAMIC_KVC_STORAGE.iter_mut() {
            if entry.0 == target_bits && entry.1 == key_string {
                if entry.2 != 0 {
                    let old_val: id = std::mem::transmute(entry.2);
                    let _: () = msg![env; old_val release];
                }
                entry.2 = value.to_bits();
                found = true;
                break;
            }
        }
        if !found {
            DYNAMIC_KVC_STORAGE.push((target_bits, key_string.to_string(), value.to_bits()));
        }
    }
}

// Per Apple's NSKeyValueCoding (NSKeyValueCoding.h / Cocoa Key-Value
// Coding Programming Guide): when -setValue:forKey: receives a nil
// value for a key whose property is a non-object scalar (BOOL, NSInteger,
// CGFloat, struct, …), the receiver is sent -setNilValueForKey:. The
// default NSObject behaviour is to raise an NSInvalidArgumentException
// with reason "[<Class> 0x… setNilValueForKey:]: could not set nil as
// the value for the key <key>." We mirror that contract: log loudly so
// the developer can diagnose the bad write, and do nothing else — most
// iOS games rely on this being a non-fatal call (the surrounding code
// catches the exception or guards against it).
- (())setNilValueForKey:(id)key {
    let class: Class = ObjC::read_isa(this, &env.mem);
    let class_name_string = env.objc.get_class_name(class).to_owned();
    let key_string = if key == nil {
        "(null)".to_string()
    } else {
        to_rust_string(env, key).to_string()
    };
    log!(
        "Warning: -[{} setNilValueForKey:@\"{}\"] on {:?}: nil assigned to a scalar property; ignored (Apple's default would raise NSInvalidArgumentException).",
        class_name_string, key_string, this
    );
}

- (bool)respondsToSelector:(SEL)selector {
    env.objc.object_has_method(&env.mem, this, selector)
}

- (bool)conformsToProtocol:(id)_protocol {
    true
}

// ИЗМЕНЕНО: Ищем _objc_msgSend через create_proc_address (без логов)
- (u32)methodForSelector:(SEL)selector {
    let dyld = &mut env.dyld;
    let mem = &mut env.mem;
    let cpu = &mut env.cpu;
    match dyld.create_proc_address(mem, cpu, "_objc_msgSend") {
        Ok(guest_func) => guest_func.addr_with_thumb_bit(),
        Err(_) => {
            log!("Error: _objc_msgSend not found! Returning dummy IMP.");
            let ptr: crate::mem::MutPtr<u16> = mem.alloc(2).cast();
            mem.write(ptr, 0x4770);
            ptr.to_bits() | 1
        }
    }
}

- (id)methodSignatureForSelector:(SEL)selector {
    let sig: id = msg_class![env; NSMethodSignature signatureWithObjCTypes:(MutVoidPtr::null())];
    let sel_str = selector.as_str(&env.mem);
    let explicit_args = sel_str.chars().filter(|&c| c == ':').count() as NSUInteger;
    let total_args = explicit_args + 2;
    () = msg![env; sig _touchHLE_setNumberOfArguments:total_args];
    sig
}

- (id)performSelector:(SEL)sel {
    assert!(!sel.is_null());
    msg_send_no_type_checking(env, (this, sel))
}

- (id)performSelector:(SEL)sel withObject:(id)o1 {
    assert!(!sel.is_null());
    msg_send_no_type_checking(env, (this, sel, o1))
}

- (id)performSelector:(SEL)sel withObject:(id)o1 withObject:(id)o2 {
    assert!(!sel.is_null());
    msg_send_no_type_checking(env, (this, sel, o1, o2))
}

- (())performSelectorInBackground:(SEL)sel withObject:(id)arg {
    detach_new_thread_inner(env, sel, this, arg, /* tolerate_type_mismatch: */ true)
}

- (())performSelector:(SEL)sel withObject:(id)arg afterDelay:(NSTimeInterval)delay {
    log_dbg!("performSelector:{} withObject:{:?} afterDelay:{}", sel.as_str(&env.mem), arg, delay);
    let sel_key: id = get_static_str(env, "SEL");
    let sel_str = from_rust_string(env, sel.as_str(&env.mem).to_string());
    let arg_key: id = get_static_str(env, "arg");
    let dict = dict_from_keys_and_objects(env, &[(sel_key, sel_str), (arg_key, arg)]);

    let selector = env.objc.lookup_selector("_touchHLE_timerFireMethod:").unwrap();
    let timer:id = msg_class![env;
        NSTimer timerWithTimeInterval:delay
                               target:this
                             selector:selector
                             userInfo:dict
                              repeats:false
    ];
    let run_loop: id = msg_class![env; NSRunLoop mainRunLoop];
    let mode: id = get_static_str(env, NSDefaultRunLoopMode);
    () = msg![env; run_loop addTimer:timer forMode:mode];
}

- (())performSelectorOnMainThread:(SEL)sel
                       withObject:(id)arg
                    waitUntilDone:(bool)wait {
    let sel_name = sel.as_str(&env.mem);

    // Video playback selectors: instead of silently dropping these, we
    // forward them to the object so that MPMoviePlayerController's play/stop
    // implementations run and post the required notifications (e.g.
    // MPMoviePlayerPlaybackDidFinishNotification). This allows apps that
    // start video playback from a background thread to have their
    // completion handlers fire correctly.
    if sel_name == "play" || sel_name == "startMovie:" || sel_name == "stopMovie:" || sel_name == "stopMovie" || sel_name == "moviePlayerInit:" || sel_name == "loadMovie:" {
        log_dbg!(
            "performSelectorOnMainThread:SEL({}) — forwarding video selector to object {:?}",
            sel_name, this
        );
        if sel_name.ends_with(':') {
            () = msg_send(env, (this, sel, arg));
        } else {
            () = msg_send(env, (this, sel));
        }
        return;
    }

    if env.current_thread == 0 {
        if sel_name.ends_with(':') {
            () = msg_send(env, (this, sel, arg));
        } else {
            () = msg_send(env, (this, sel));
        }
        return;
    }

    if env.bundle.bundle_identifier().starts_with("com.gameloft.Ferrari") && wait
        && (sel == env.objc.lookup_selector("initTextInput:").unwrap() ||
           sel == env.objc.lookup_selector("removeTextField:").unwrap()) {
            log!("Applying game-specific hack for Ferrari GT: performing performSelectorOnMainThread:SEL({}) waitUntilDone:true on thread {}", sel_name, env.current_thread);
            () = msg_send(env, (this, sel, arg));
            return;
        }

    if env.bundle.bundle_identifier().starts_with("com.gameloft.HOS2") && wait
        && (sel == env.objc.lookup_selector("sendGameInfo").unwrap() || sel == env.objc.lookup_selector("setStatusBar:").unwrap()) {
            log!("Applying game-specific hack for HOS2: performing performSelectorOnMainThread:SEL({}) waitUntilDone:true on thread {}", sel_name, env.current_thread);
            if sel_name.ends_with(':') {
                () = msg_send(env, (this, sel, arg));
            } else {
                () = msg_send(env, (this, sel));
            }
            return;
        }

    if wait {
        // `waitUntilDone:YES` from a background thread: schedule the selector to run on the main
        // thread and block the calling thread on a host semaphore that
        // `_touchHLE_timerFireMethod:` will post once the selector has finished executing.
        //
        // Games such as Call of Duty: Zombies (Marmalade SDK) rely on this synchronisation to
        // safely hand work off to the main thread via an `s3eAppDelegate.m_Func` slot: the
        // background thread sets `m_Func`, calls `performSelectorOnMainThread:Functor
        // withObject:nil waitUntilDone:YES`, and expects to block until `Functor` has called and
        // cleared `m_Func`. Returning early here lets the background thread overwrite `m_Func`
        // before the main thread's `Functor` has a chance to read it, eventually loading a
        // NULL function pointer and crashing into the guest's null page.
        let sel_name_owned = sel_name.to_string();
        log_dbg!(
            "performSelectorOnMainThread:{} from background thread {} (wait=true) — scheduling and waiting",
            sel_name_owned, env.current_thread
        );

        let sel_key: id = get_static_str(env, "SEL");
        let sel_str = from_rust_string(env, sel_name_owned);
        let arg_key: id = get_static_str(env, "arg");
        let dict = dict_from_keys_and_objects(env, &[(sel_key, sel_str), (arg_key, arg)]);

        let fire_selector = env.objc.lookup_selector("_touchHLE_timerFireMethod:").unwrap();
        let timer: id = msg_class![env;
            NSTimer timerWithTimeInterval:(0.0 as NSTimeInterval)
                                   target:this
                                 selector:fire_selector
                                 userInfo:dict
                                  repeats:false
        ];

        let sem = host_create_semaphore(env, 0);
        unsafe {
            SYNC_PERFORM_SEMAPHORES.push((timer.to_bits(), sem.to_bits()));
        }

        let run_loop: id = msg_class![env; NSRunLoop mainRunLoop];
        let mode: id = get_static_str(env, NSDefaultRunLoopMode);
        () = msg![env; run_loop addTimer:timer forMode:mode];

        sem_wait(env, sem);
        host_destroy_semaphore(env, sem);
        return;
    }

    log_dbg!(
        "performSelectorOnMainThread:{} from background thread {} (wait={}) — scheduling",
        sel_name, env.current_thread, wait
    );
    msg![env; this performSelector:sel withObject:arg afterDelay:0.0]
}

- (())_touchHLE_timerFireMethod:(id)which {
    // Pull out any semaphore associated with this timer up-front so we can
    // always post it before returning, regardless of how this method exits.
    // (If we returned early without posting, a thread blocked in
    // performSelectorOnMainThread:waitUntilDone:YES would hang forever.)
    let sem_to_post = unsafe {
        let timer_bits = which.to_bits();
        SYNC_PERFORM_SEMAPHORES
            .iter()
            .position(|x| x.0 == timer_bits).map(|pos| SYNC_PERFORM_SEMAPHORES.remove(pos).1)
    };

    let dict: id = msg![env; which userInfo];
    let sel_key: id = get_static_str(env, "SEL");
    let sel_str_id: id = msg![env; dict objectForKey:sel_key];
    let sel_str = to_rust_string(env, sel_str_id).into_owned();

    // The stored selector name should always be present, but guard against a
    // missing/empty entry (e.g. the timer's userInfo dictionary was released
    // out from under us) rather than panicking. With no selector there is
    // nothing to fire, so just release any waiter and bail.
    if sel_str.is_empty() {
        log!(
            "Warning: _touchHLE_timerFireMethod: timer {:?} has no stored selector; skipping.",
            which
        );
        if let Some(sem_bits) = sem_to_post {
            let sem: crate::mem::MutPtr<crate::libc::semaphore::sem_t> =
                crate::mem::MutPtr::from_bits(sem_bits);
            sem_post(env, sem);
        }
        return;
    }

    // Turn the stored name into a selector. This mirrors sel_registerName():
    // a non-empty method name always maps to a valid selector, registering a
    // new one if it has not been seen before, so this never returns None.
    let sel = env
        .objc
        .register_host_selector(sel_str.clone(), &mut env.mem);

    let arg_key: id = get_static_str(env, "arg");
    let arg: id = msg![env; dict objectForKey:arg_key];
    let target_bits = this.to_bits();
    let mut cancelled = false;

    unsafe {
        if let Some(pos) = crate::frameworks::foundation::ns_object::CANCELLED_PERFORMS.iter().position(|x| x.0 == target_bits && x.1.as_deref() == Some(sel_str.as_str())) {
            crate::frameworks::foundation::ns_object::CANCELLED_PERFORMS.remove(pos);
            cancelled = true;
        } else if crate::frameworks::foundation::ns_object::CANCELLED_PERFORMS.iter().any(|x| x.0 == target_bits && x.1.is_none()) {
            cancelled = true;
        }
    }

    if !cancelled {
        if sel.as_str(&env.mem).ends_with(':') {
            () = msg_send(env, (this, sel, arg));
        } else {
            () = msg_send(env, (this, sel));
        }
    }

    if let Some(sem_bits) = sem_to_post {
        let sem: crate::mem::MutPtr<crate::libc::semaphore::sem_t> =
            crate::mem::MutPtr::from_bits(sem_bits);
        sem_post(env, sem);
    }
}

- (())awakeFromNib {
}

- (())performSelector:(SEL)sel onThread:(id)_thread withObject:(id)arg waitUntilDone:(bool)_wait {
    log_dbg!("performSelector:{} onThread:withObject:waitUntilDone: — scheduling on main thread instead", sel.as_str(&env.mem));
    msg![env; this performSelector:sel withObject:arg afterDelay:0.0]
}

- (())performSelector:(SEL)sel onThread:(id)_thread withObject:(id)arg waitUntilDone:(bool)_wait modes:(id)_modes {
    log_dbg!("performSelector:{} onThread:withObject:waitUntilDone:modes: — scheduling on main thread instead", sel.as_str(&env.mem));
    msg![env; this performSelector:sel withObject:arg afterDelay:0.0]
}

- (id)valueForKey:(id)key {
    let key_str = super::ns_string::to_rust_string(env, key);
    if key_str.is_empty() { return nil; }

    let camel_case_key_string = format!(
        "{}{}",
        key_str.as_bytes()[0].to_ascii_uppercase() as char,
        &key_str[1..]
    );

    // 1. Поиск геттеров (get<Key>, <key>, is<Key>)
    if let Some(sel) = env.objc.lookup_selector(&key_str) {
        if env.objc.object_has_method(&env.mem, this, sel) {
            return msg_send(env, (this, sel));
        }
    }
    let is_sel_name = format!("is{camel_case_key_string}");
    if let Some(sel) = env.objc.lookup_selector(&is_sel_name) {
        if env.objc.object_has_method(&env.mem, this, sel) {
            return msg_send(env, (this, sel));
        }
    }
    let get_sel_name = format!("get{camel_case_key_string}");
    if let Some(sel) = env.objc.lookup_selector(&get_sel_name) {
        if env.objc.object_has_method(&env.mem, this, sel) {
            return msg_send(env, (this, sel));
        }
    }

    // 2. Чтение реальных ivars
    let class = msg![env; this class];
    if let Some(access_sel) = env.objc.lookup_selector("accessInstanceVariablesDirectly") {
        if env.objc.class_has_method(class, access_sel) {
            let access_ivars: bool = msg_send(env, (class, access_sel));
            if access_ivars {
                if let Some(ivar_ptr) = env.objc
                    .object_lookup_ivar(&env.mem, this, &format!("_{key_str}"))
                    .or_else(|| env.objc.object_lookup_ivar(&env.mem, this, &format!("_is{camel_case_key_string}")))
                    .or_else(|| env.objc.object_lookup_ivar(&env.mem, this, &format!("{key_str}")))
                    .or_else(|| env.objc.object_lookup_ivar(&env.mem, this, &format!("is{camel_case_key_string}")))
                {
                    let val: id = env.mem.read(ivar_ptr.cast());
                    return val;
                }
            }
        }
    }

    // 3. Чтение нашего динамического хранилища
    unsafe {
        let target_bits = this.to_bits();
        for entry in DYNAMIC_KVC_STORAGE.iter() {
            if entry.0 == target_bits && entry.1 == key_str {
                let val: id = std::mem::transmute(entry.2);
                return val;
            }
        }
    }

    // 4. Fallback (valueForUndefinedKey:)
    if let Some(undef_sel) = env.objc.lookup_selector("valueForUndefinedKey:") {
        if env.objc.object_has_method(&env.mem, this, undef_sel) {
            return msg_send(env, (this, undef_sel, key));
        }
    }

    log!("Warning: valueForKey:{} not found on {:?} — returning nil", key_str, this);
    nil
}

- (id)valueForUndefinedKey:(id)key {
    let key_string = to_rust_string(env, key);
    let class: Class = ObjC::read_isa(this, &env.mem);
    let class_name_string = env.objc.get_class_name(class).to_owned();
    log!("Warning: Object {:?} of class {:?} does not have a getter for {} (valueForUndefinedKey:) — returning nil",
        this, class_name_string, key_string);
    nil
}

- (id)valueForKeyPath:(id)key_path {
    msg![env; this valueForKey:key_path]
}

- (())setValue:(id)value forKeyPath:(id)key_path {
    msg![env; this setValue:value forKey:key_path]
}

// MARK: - Key-Value Observing (KVO)

- (NSUInteger)version {
    0
}

- (())zone {

}

- (Class)superclass {
    nil
}

- (())addObserver:(id)observer forKeyPath:(id)keyPath options:(NSUInteger)options context:(id)context {
    if observer == nil || keyPath == nil {
        log_dbg!("addObserver:forKeyPath:options:context: — observer or keyPath is nil, ignored");
        return;
    }
    let key_str = to_rust_string(env, keyPath);
    log_dbg!(
        "addObserver:{:?} forKeyPath:{:?} options:{} context:{:?}",
        observer, key_str, options, context
    );

    // Store the observer registration in our global KVO storage.
    // Format: (observed_object, observer, keyPath string, options, context)
    unsafe {
        KVO_OBSERVERS.push((
            this.to_bits(),
            observer.to_bits(),
            key_str.into_owned(),
            options,
            context.to_bits(),
        ));
    }

    // If NSKeyValueObservingOptionInitial (0x04) is set, deliver an
    // initial notification immediately
    if options & 0x04 != 0 {
        let change_dict: id = msg_class![env; NSDictionary dictionary];
        let observe_sel = env.objc.lookup_selector("observeValueForKeyPath:ofObject:change:context:");
        if let Some(sel) = observe_sel {
            let _: () = msg_send(env, (observer, sel, keyPath, this, change_dict, context));
        }
    }
}

- (())removeObserver:(id)observer forKeyPath:(id)keyPath {
    if observer == nil || keyPath == nil {
        return;
    }
    let key_str = to_rust_string(env, keyPath);
    log_dbg!(
        "removeObserver:{:?} forKeyPath:{:?}",
        observer, key_str
    );
    unsafe {
        KVO_OBSERVERS.retain(|entry| {
            !(entry.0 == this.to_bits()
                && entry.1 == observer.to_bits()
                && entry.2 == key_str.as_ref())
        });
    }
}

- (())removeObserver:(id)observer forKeyPath:(id)keyPath context:(id)_context {
    () = msg![env; this removeObserver:observer forKeyPath:keyPath];
}

- (())willChangeValueForKey:(id)_key {
    // KVO pre-change notification — currently a no-op.
    // A full implementation would snapshot the old value here.
}

- (())didChangeValueForKey:(id)key {
    if key == nil { return; }
    let key_str = to_rust_string(env, key);

    // Collect matching observers
    let observers: Vec<(u32, u32, u32)> = unsafe {
        KVO_OBSERVERS.iter()
            .filter(|entry| entry.0 == this.to_bits() && entry.2 == key_str.as_ref())
            .map(|entry| (entry.1, entry.3, entry.4))
            .collect()
    };

    for (observer_bits, _options, context_bits) in observers {
        use crate::objc::id;
        let observer: id = crate::mem::Ptr::from_bits(observer_bits);
        let context: id = crate::mem::Ptr::from_bits(context_bits);
        let change_dict: id = msg_class![env; NSDictionary dictionary];
        let observe_sel = env.objc.lookup_selector("observeValueForKeyPath:ofObject:change:context:");
        if let Some(sel) = observe_sel {
            let _: () = msg_send(env, (observer, sel, key, this, change_dict, context));
        }
    }
}

- (())observeValueForKeyPath:(id)_keyPath ofObject:(id)_object change:(id)_change context:(id)_context {
    // Default implementation does nothing — subclasses override this.
}

// =========================================================================
// Fallback UIView-like methods on NSObject for compatibility with game
// engines (e.g. Digital Chocolate) that send view-hierarchy messages to
// objects of the wrong type due to corrupted pointers. Returning an empty
// array / nil prevents infinite loops and log spam without crashing.
// =========================================================================
- (id)subviews {
    // Return an empty NSArray so iteration loops terminate immediately.
    msg_class![env; NSArray array]
}

- (id)superview {
    nil
}

@end

};
