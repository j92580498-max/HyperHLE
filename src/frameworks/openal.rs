/*
 * Этот исходный код подчиняется условиям лицензии Mozilla Public License, v. 2.0.
 * Если копия MPL не поставлялась с этим файлом, вы можете получить её на https://mozilla.org/MPL/2.0/.
 */
//! OpenAL.
//!
//! Это тонкая оболочка поверх OpenAL Soft, см. [crate::audio::openal].
//!
//! Ресурсы:
//! - [Спецификация OpenAL 1.1](https://www.openal.org/documentation/openal-1.1-specification.pdf)
//! - Apple [Technical Note TN2199: OpenAL FAQ для iPhone OS](https://web.archive.org/web/20090826202158/http://developer.apple.com/iPhone/library/technotes/tn2008/tn2199.html) (также доступно [здесь](https://developer.apple.com/library/archive/technotes/tn2199/_index.html))

use crate::audio::openal as al;
use crate::audio::openal::al_types::*;
use crate::audio::openal::alc_types::*;
use crate::audio::openal::{
    OpenAL, OpenALContext, ALC_DEVICE_SPECIFIER, ALC_FREQUENCY, ALC_MONO_SOURCES, ALC_REFRESH,
    ALC_STEREO_SOURCES, ALC_SYNC, AL_EXTENSIONS, AL_RENDERER, AL_VENDOR, AL_VERSION,
};
use crate::dyld::{export_c_func, FunctionExports, HostDylib};
use crate::libc::string::strcmp;
use crate::mem::{ConstPtr, ConstVoidPtr, GuestUSize, MutPtr, MutVoidPtr, Ptr, SafeWrite};
use crate::Environment;
use std::collections::HashMap;
use std::ffi::{CStr, CString};

pub const DYLIB: HostDylib = HostDylib {
    path: "/System/Library/Frameworks/OpenAL.framework/OpenAL",
    aliases: &[],
    class_exports: &[],
    constant_exports: &[],
    function_exports: &[FUNCTIONS],
};

#[derive(Default)]
pub struct State {
    devices: HashMap<MutPtr<GuestALCdevice>, *mut ALCdevice>,
    contexts: HashMap<MutPtr<GuestALCcontext>, OpenALContext>,
    /// Cache of guest-memory copies of strings returned from
    /// `alcGetString`. Per OpenAL 1.1 §6.3.5 the returned pointer is
    /// owned by the implementation and must remain valid for the
    /// lifetime of the device, so we hand the same pointer back on
    /// repeat calls instead of allocating a new guest cstr each time.
    /// Keyed on `(device, param)` because device-bound queries can
    /// produce different strings per device.
    strings_cache: HashMap<(MutPtr<GuestALCdevice>, ALenum), ConstPtr<u8>>,
    current_ctx: MutPtr<GuestALCcontext>,
    /// "Zombie" context: when a guest app destroys the current context
    /// without first deleting its buffers/sources, we keep the host
    /// OpenAL context alive here so that subsequent `alDeleteBuffers`
    /// / `alDeleteSources` calls can still execute against a valid
    /// host context. The zombie is dropped when a new context is made
    /// current, or when the device is closed.
    /// This matches Apple's iPhone OS behavior where the implementation
    /// tolerates out-of-order cleanup.
    zombie_context: Option<OpenALContext>,
    /// Maps guest context pointers that have been destroyed to their
    /// associated guest device pointer. This allows `alcGetContextsDevice`
    /// to still return the correct device for contexts in cleanup sequences
    /// (apps like Galaxy On Fire destroy the context, then query its device,
    /// then close the device).
    destroyed_context_devices: HashMap<MutPtr<GuestALCcontext>, MutPtr<GuestALCdevice>>,
    /// Last value passed to `alcMacOSXRenderingQuality`. Zero means the
    /// app never set one, in which case `alcMacOSXGetRenderingQuality`
    /// returns the documented iPhone OS default of
    /// `ALC_IPHONE_SPATIAL_RENDERING_QUALITY_HIGH` (2).
    pub macosx_rendering_quality: ALint,
}
impl State {
    fn get(env: &mut Environment) -> &mut Self {
        &mut env.framework_state.openal
    }

    fn try_make_current(env: &mut Environment) -> Option<OpenAL<'_>> {
        let state = &mut env.framework_state.openal;
        // Try the active context first.
        if let Some(ctx) = state.contexts.get_mut(&state.current_ctx) {
            return Some(ctx.make_current(&mut env.openal_manager));
        }
        // Fall back to the zombie context — this allows cleanup operations
        // (alDeleteBuffers, alDeleteSources) to succeed even after the guest
        // has called alcDestroyContext on what was the current context.
        if let Some(zombie) = state.zombie_context.as_mut() {
            return Some(zombie.make_current(&mut env.openal_manager));
        }
        None
    }
}

/// Непрозрачный тип в гостевой памяти, представляющий [ALCdevice] из памяти
//хоста.
struct GuestALCdevice {
    _filler: u8,
}
impl SafeWrite for GuestALCdevice {}
/// Непрозрачный тип в гостевой памяти, представляющий [ALCcontext] из памяти
//хоста.
struct GuestALCcontext {
    _filler: u8,
}
impl SafeWrite for GuestALCcontext {}

macro_rules! try_get_context {
    ($env: ident, $name: ident) => {
        let state = &mut $env.framework_state.openal;
        let Some($name) = state
            .contexts
            .get_mut(&state.current_ctx)
            .map(|ctx| ctx.make_current(&mut $env.openal_manager))
        else {
            log_dbg!(
                "Попытка получить контекст, но текущий активный контекст {:?} недействителен, пропускаем!",
                State::get($env).current_ctx
            );
            // TODO: установить ошибку
            return;
        };
    };
    ($env: ident, $name: ident, $rval: expr) => {
        let state = &mut $env.framework_state.openal;
        let Some($name) = state
            .contexts
            .get_mut(&state.current_ctx)
            .map(|ctx| ctx.make_current(&mut $env.openal_manager))
        else {
            log_dbg!(
                "Попытка получить контекст, но текущий активный контекст {:?} недействителен, пропускаем!",
                State::get($env).current_ctx
            );
            // TODO: установить ошибку
            return $rval;
        };
    };
}

// === alc.h ===

fn alcOpenDevice(env: &mut Environment, devicename: ConstPtr<u8>) -> MutPtr<GuestALCdevice> {
    if !devicename.is_null() {
        // ObjectAL and some other audio libraries pass a non-NULL
        // device specifier obtained from `alcGetString`. If it doesn't
        // match the default device name, we used to return NULL, but on
        // some Android builds OpenAL Soft advertises a different name
        // than the one ObjectAL expects, breaking audio init. Be
        // lenient: log a warning and try the default device anyway.
        let d_name = alcGetString(env, Ptr::null(), ALC_DEVICE_SPECIFIER);
        if strcmp(env, d_name, devicename) != 0 {
            log!(
                "Warning: alcOpenDevice requested name {:?} differs from default {:?}; opening default anyway",
                env.mem.cstr_at_utf8(devicename),
                env.mem.cstr_at_utf8(d_name)
            );
        }
    }

    let mut res = unsafe { al::alcOpenDevice(std::ptr::null()) };
    if res.is_null() {
        log!("alcOpenDevice(NULL) failed, trying \"No Output\" fallback");
        let null_name = b"No Output\0";
        res = unsafe { al::alcOpenDevice(null_name.as_ptr() as *const _) };
        if res.is_null() {
            log!("alcOpenDevice(\"No Output\") also failed, returning NULL");
            return Ptr::null();
        }
    }

    let guest_res = env.mem.alloc_and_write(GuestALCdevice { _filler: 0 });
    State::get(env).devices.insert(guest_res, res);
    log_dbg!("alcOpenDevice(NULL) => {:?} (хост: {:?})", guest_res, res,);
    guest_res
}
fn alcCloseDevice(env: &mut Environment, device: MutPtr<GuestALCdevice>) -> bool {
    if device.is_null() {
        log!("alcCloseDevice() вызван с устройством NULL, игнорируем");
        return false;
    }
    let Some(host_device) = State::get(env).devices.remove(&device) else {
        log!(
            "Warning: alcCloseDevice({:?}) called with unknown device; ignoring.",
            device
        );
        return false;
    };

    // Drop the zombie context (if any) before closing the device,
    // otherwise OpenAL Soft may complain about dangling contexts.
    State::get(env).zombie_context = None;

    // Clean up destroyed_context_devices entries referencing this device.
    State::get(env)
        .destroyed_context_devices
        .retain(|_, dev| *dev != device);

    // Drop any cached `alcGetString` entries that referenced this
    // device — both the device-specific ones and any global query
    // (NULL device) entry would otherwise keep guest pointers alive
    // forever. The owned guest memory is freed below.
    let stale_keys: Vec<_> = State::get(env)
        .strings_cache
        .keys()
        .filter(|(d, _)| *d == device)
        .copied()
        .collect();
    for key in stale_keys {
        if let Some(ptr) = State::get(env).strings_cache.remove(&key) {
            env.mem.free(ptr.cast_mut().cast());
        }
    }
    env.mem.free(device.cast());
    let res = unsafe { al::alcCloseDevice(host_device) };
    log_dbg!("alcCloseDevice({:?}) => {:?}", device, res,);
    res != al::ALC_FALSE
}

fn alcGetError(env: &mut Environment, device: MutPtr<GuestALCdevice>) -> i32 {
    // Per OpenAL spec, alcGetError on an invalid device returns
    // ALC_INVALID_DEVICE rather than a host-level crash.
    let Some(&host_device) = State::get(env).devices.get(&device) else {
        log!(
            "Warning: alcGetError({:?}) called with unknown/NULL device, returning ALC_INVALID_DEVICE",
            device
        );
        // ALC_INVALID_DEVICE = 0xA001, per the OpenAL 1.1 specification.
        return 0xA001;
    };

    let res = unsafe { al::alcGetError(host_device) };
    log_dbg!("alcGetError({:?}) => {:#x}", host_device, res);
    res
}

fn alcGetString(
    env: &mut Environment,
    device: MutPtr<GuestALCdevice>,
    param: ALenum,
) -> ConstPtr<u8> {
    // Check the cache first — `alcGetString` is documented (OpenAL 1.1
    // spec, §6.3.5) to return a static, library-owned C string. Real
    // implementations literally hand back the same pointer on every
    // call. Apps in the wild (Pou, 3D Magic Words, Chess Free, etc.)
    // call `alcGetString` from inside their audio update loop —
    // allocating a fresh guest cstr each time leaks an unbounded
    // amount of guest memory until the heap is exhausted, which on
    // Android (where touchHLE has a small fixed guest address space)
    // crashes the emulator after a few minutes of play.
    //
    // We key the cache on `(device, param)` because the spec lets
    // device-bound queries (`ALC_DEVICE_SPECIFIER`, `ALC_EXTENSIONS`,
    // ...) return a string that depends on the device identity. We
    // collapse the `device` half to "null" / "non-null" since touchHLE
    // exposes exactly one host device per guest device anyway and the
    // contents-of-the-string are the same.
    let cache_key = (
        if device.is_null() {
            MutPtr::<GuestALCdevice>::null()
        } else {
            device
        },
        param,
    );
    if let Some(&cached) = State::get(env).strings_cache.get(&cache_key) {
        log_dbg!(
            "alcGetString({:?}, {}) => {:?} (cached)",
            device,
            param,
            cached
        );
        return cached;
    }

    // Resolve to the host OpenAL Soft device (or NULL for global queries).
    let host_device = if device.is_null() {
        std::ptr::null_mut()
    } else {
        match State::get(env).devices.get(&device) {
            Some(&dev) => dev,
            None => {
                log!(
                    "Предупреждение: alcGetString вызван с неизвестным устройством {:?}",
                    device
                );
                std::ptr::null_mut()
            }
        }
    };

    // SAFETY: alcGetString returns a pointer to memory owned by OpenAL
    // Soft itself; we only borrow it long enough to copy into guest
    // memory.
    let res = unsafe { al::alcGetString(host_device, param) };
    if res.is_null() {
        log_dbg!("alcGetString({:?}, {}) вернул NULL", device, param);
        return Ptr::null();
    }

    let s = unsafe { CStr::from_ptr(res) };
    let guest_ptr = env.mem.alloc_and_write_cstr(s.to_bytes()).cast_const();
    log_dbg!(
        "alcGetString({:?}, {}) => {:?} (caching for future calls)",
        device,
        param,
        s
    );
    State::get(env).strings_cache.insert(cache_key, guest_ptr);
    guest_ptr
}

const ALLOWED_CONTEXT_ATTRIBUTES: [ALCint; 5] = [
    ALC_FREQUENCY,
    ALC_REFRESH,
    ALC_SYNC,
    ALC_MONO_SOURCES,
    ALC_STEREO_SOURCES,
];

fn alcCreateContext(
    env: &mut Environment,
    device: MutPtr<GuestALCdevice>,
    attr_list: ConstPtr<i32>,
) -> MutPtr<GuestALCcontext> {
    // Вектор для хранения очищенного списка атрибутов, который безопасно передадим в OpenAL Soft
    let mut clean_attrs: Vec<ALCint> = Vec::new();

    let attr_list_ptr: *const ALCint = if attr_list.is_null() {
        std::ptr::null()
    } else {
        let mut ptr: MutPtr<i32> = attr_list.cast_mut();
        // список атрибутов завершается нулем (NULL)
        while env.mem.read(ptr) != 0 {
            let attr = env.mem.read(ptr);
            let val = env.mem.read(ptr + 1);

            // ИСПРАВЛЕНИЕ: Мягко фильтруем атрибуты вместо жесткого краша (assert убран).
            // Неизвестные/специфичные для iOS атрибуты просто игнорируем.
            if ALLOWED_CONTEXT_ATTRIBUTES.contains(&attr) {
                log_dbg!("Атрибут alcCreateContext {:#x} => {} (разрешен)", attr, val);
                clean_attrs.push(attr);
                clean_attrs.push(val);
            } else {
                log!(
                    "Warning: Игнорируем неподдерживаемый атрибут контекста OpenAL: {:#x} => {}",
                    attr,
                    val
                );
            }

            ptr += 2;
        }

        if !clean_attrs.is_empty() {
            clean_attrs.push(0); // Добавляем обязательный завершающий нуль
            clean_attrs.as_ptr()
        } else {
            std::ptr::null()
        }
    };

    let state = State::get(env);
    // Per OpenAL spec, alcCreateContext with an invalid (e.g. NULL) device
    // must set ALC_INVALID_DEVICE and return NULL.
    let Some(&host_device) = state.devices.get(&device) else {
        log!(
            "Warning: alcCreateContext({:?}, ...) called with unknown/NULL device, returning NULL",
            device
        );
        return Ptr::null();
    };

    let res = unsafe {
        OpenALContext::new_with_device_and_attrlist(
            env.openal_manager.as_mut(),
            host_device,
            attr_list_ptr,
        )
    };
    let Ok(ctx) = res else {
        log_dbg!("alcCreateContext({:?}, (...)) вернул NULL", device);
        return Ptr::null();
    };

    let guest_res = env.mem.alloc_and_write(GuestALCcontext { _filler: 0 });

    log_dbg!(
        "alcCreateContext({:?}, ...) => {:?} (хост: {:?})",
        device,
        guest_res,
        ctx,
    );

    State::get(env).contexts.insert(guest_res, ctx);
    guest_res
}
fn alcDestroyContext(env: &mut Environment, context: MutPtr<GuestALCcontext>) {
    if context.is_null() {
        log!("alcDestroyContext() вызван с контекстом NULL, игнорируем");
        return;
    }
    let Some(host_context) = State::get(env).contexts.remove(&context) else {
        // Check if it's already been destroyed (idempotent destroy)
        if State::get(env).destroyed_context_devices.contains_key(&context) {
            log_dbg!(
                "alcDestroyContext({:?}): already destroyed (idempotent call); ignoring.",
                context
            );
        } else {
            log!(
                "Warning: alcDestroyContext({:?}) called with unknown context; ignoring.",
                context
            );
        }
        return;
    };

    // Track the device association so alcGetContextsDevice still works
    // for recently-destroyed contexts (apps query device after destroy).
    let device_ptr = {
        let host_device = host_context.GetContextsDevice();
        State::get(env)
            .devices
            .iter()
            .find(|(&_guest, &host)| host == host_device)
            .map(|(&guest, _)| guest)
            .unwrap_or(Ptr::null())
    };
    State::get(env)
        .destroyed_context_devices
        .insert(context, device_ptr);

    // If this was the current context, save it as a zombie so that
    // subsequent alDeleteBuffers/alDeleteSources calls can still
    // execute cleanup against a valid host OpenAL context.
    let is_current = State::get(env).current_ctx == context;
    if is_current {
        // Drop any previous zombie before replacing it.
        State::get(env).zombie_context = Some(host_context);
        State::get(env).current_ctx = Ptr::null();
    }
    // else: the context is simply dropped (triggering alcDestroyContext on host)

    env.mem.free(context.cast());
    log_dbg!("alcDestroyContext({:?})", context);
}

fn alcProcessContext(env: &mut Environment, context: MutPtr<GuestALCcontext>) {
    if context.is_null() {
        log!("alcProcessContext() вызван с контекстом NULL, игнорируем");
        return;
    }
    let Some(host_context) = State::get(env).contexts.get_mut(&context) else {
        log!(
            "Warning: alcProcessContext({:?}) called with unknown context; ignoring.",
            context
        );
        return;
    };
    host_context.ProcessContext()
}
fn alcSuspendContext(env: &mut Environment, context: MutPtr<GuestALCcontext>) {
    if context.is_null() {
        log!("alcSuspendContext() вызван с контекстом NULL, игнорируем");
        return;
    }
    let Some(host_context) = State::get(env).contexts.get_mut(&context) else {
        log!(
            "Warning: alcSuspendContext({:?}) called with unknown context; ignoring.",
            context
        );
        return;
    };
    host_context.SuspendContext()
}

fn alcMakeContextCurrent(env: &mut Environment, context: MutPtr<GuestALCcontext>) -> bool {
    let res = if context.is_null() || State::get(env).contexts.contains_key(&context) {
        // When switching to a new valid context (or NULL), drop the zombie.
        // The zombie was only kept alive for cleanup operations; once the
        // app sets a new context, cleanup is considered done.
        if !context.is_null() {
            State::get(env).zombie_context = None;
        }
        State::get(env).current_ctx = context;
        true
    } else {
        false
    };
    log_dbg!("alcMakeContextCurrent({:?}) => {}", context, res);
    res
}

fn alcGetCurrentContext(env: &mut Environment) -> MutPtr<GuestALCcontext> {
    State::get(env).current_ctx
}

fn alcGetContextsDevice(
    env: &mut Environment,
    context: MutPtr<GuestALCcontext>,
) -> MutPtr<GuestALCdevice> {
    if context.is_null() {
        log!("alcGetContextsDevice() вызван с контекстом NULL, игнорируем");
        return Ptr::null();
    }
    // Check live contexts first.
    if let Some(host_context) = State::get(env).contexts.get(&context) {
        let host_device = host_context.GetContextsDevice();
        let found = State::get(env)
            .devices
            .iter()
            .find(|(&_guest, &host)| host == host_device)
            .map(|(&guest_device, _)| guest_device);
        if let Some(guest_device) = found {
            return guest_device;
        }
        log!(
            "Warning: alcGetContextsDevice({:?}): host device not tracked; returning NULL.",
            context
        );
        return Ptr::null();
    }
    // Check recently-destroyed contexts — apps like Galaxy On Fire
    // destroy the context and then immediately query its device.
    if let Some(&device_ptr) = State::get(env).destroyed_context_devices.get(&context) {
        log_dbg!(
            "alcGetContextsDevice({:?}): context was destroyed but device {:?} still known.",
            context,
            device_ptr
        );
        return device_ptr;
    }
    log!(
        "Warning: alcGetContextsDevice({:?}) called with unknown context; returning NULL.",
        context
    );
    Ptr::null()
}

fn alcGetProcAddress(
    env: &mut Environment,
    _device: ConstPtr<GuestALCdevice>,
    func_name: ConstPtr<u8>,
) -> MutVoidPtr {
    let raw_name = match env.mem.cstr_at_utf8(func_name) {
        Ok(s) => s.to_owned(),
        Err(_) => {
            log!(
                "Warning: alcGetProcAddress({:?}): function name is not valid UTF-8; returning NULL.",
                func_name
            );
            return Ptr::null();
        }
    };
    let mangled_func_name = format!("_{}", raw_name);
    if !mangled_func_name.starts_with("_al") {
        log!(
            "Warning: alcGetProcAddress: requested non-AL function {:?}; returning NULL.",
            raw_name
        );
        return Ptr::null();
    }

    if let Ok(ptr) = env
        .dyld
        .create_proc_address(&mut env.mem, &mut env.cpu, &mangled_func_name)
    {
        Ptr::from_bits(ptr.addr_with_thumb_bit())
    } else {
        // Some apps look up macOS-specific extension entry points (e.g.
        // alcMacOSMixerOutputRate) and gracefully handle a NULL return.
        // Many apps also probe for extension function pointers and only use
        // them when non-NULL. Returning NULL is the spec-compliant behavior
        // for unsupported function names; panicking would crash the guest.
        log!(
            "Warning: alcGetProcAddress: unimplemented OpenAL function {}; returning NULL.",
            mangled_func_name
        );
        Ptr::null()
    }
}

// TODO: больше функций

// === al.h ===

fn alGetError(env: &mut Environment) -> i32 {
    // Super Monkey Ball и другие приложения пытаются использовать эту функцию
    // (вместо
    // alcGetError), чтобы узнать, удалось ли открыть устройство. Это
    // неправильно и похоже на ошибку.
    // Вероятно, iPhone OS не обращает на это внимания, но OpenAL Soft в этом
    // случае возвращает ошибку,
    // и игра пропускает оставшуюся часть инициализации звука.
    // Некоторые другие приложения пытаются вызвать это в удаленном контексте
    // (обычно из другого потока), поэтому нам нужно просто тихо игнорировать
    // это.
    try_get_context!(env, context, al::AL_NO_ERROR);
    let res = unsafe { context.GetError() };
    log_dbg!("alGetError() => {:#x}", res);
    res
}

fn alDistanceModel(env: &mut Environment, value: ALenum) {
    try_get_context!(env, context);
    unsafe { context.DistanceModel(value) };
}

fn alGetEnumValue(env: &mut Environment, enumName: ConstPtr<u8>) -> ALenum {
    let s = env.mem.cstr_at_utf8(enumName).unwrap();
    let ss = CString::new(s).unwrap();

    let res = unsafe { OpenALContext::GetEnumValue(ss.as_ptr()) };
    log_dbg!("alGetEnumValue({:?}) => {:?}", s, res);
    res
}

fn alIsBuffer(env: &mut Environment, buffer: ALuint) -> ALboolean {
    try_get_context!(env, context, 0);
    unsafe { context.IsBuffer(buffer) }
}

fn alGetBufferi(env: &mut Environment, buffer: ALuint, param: ALenum, value: MutPtr<ALint>) {
    let value = env.mem.ptr_at_mut(value, 1);
    try_get_context!(env, context);
    unsafe { context.GetBufferi(buffer, param, value) }
}

fn alIsSource(env: &mut Environment, source: ALuint) -> ALboolean {
    try_get_context!(env, context, 0);
    unsafe { context.IsSource(source) }
}

fn alIsExtensionPresent(env: &mut Environment, ext_name: ConstPtr<u8>) -> ALboolean {
    try_get_context!(env, context, 0);
    let s = env.mem.cstr_at_utf8(ext_name).unwrap();
    let ss = CString::new(s).unwrap();
    unsafe { context.IsExtensionPresent(ss.as_ptr()) }
}

fn alEnable(env: &mut Environment, capability: ALenum) {
    try_get_context!(env, context);
    unsafe { context.Enable(capability) };
}

fn alGetString(env: &mut Environment, param: ALenum) -> ConstPtr<u8> {
    // alGetString queries are global (not device-bound), so we key the
    // shared strings cache with a null `GuestALCdevice` sentinel.
    let cache_key = (MutPtr::<GuestALCdevice>::null(), param);
    let res = if let Some(&str) = env.framework_state.openal.strings_cache.get(&cache_key) {
        str
    } else {
        // Strings extracted from iPhone 3GS, iOS 4.0.1 (also matches the iPhone
        // simulator). Per the OpenAL 1.1 specification, alGetString must also
        // return human-readable strings for the AL error tokens, otherwise apps
        // that call it from their error-logging path (Farm Frenzy does this on
        // every alSourcePlay failure) crash the emulator.
        // See: https://www.openal.org/documentation/openal-1.1-specification.pdf §6.3.5
        let s: &[u8] = match param {
            AL_VENDOR => b"Apple Inc.",
            AL_VERSION => b"1.1",
            AL_RENDERER => b"Software",
            AL_EXTENSIONS => b"AL_EXT_OFFSET AL_EXT_LINEAR_DISTANCE AL_EXT_EXPONENT_DISTANCE AL_EXT_STATIC_BUFFER",
            // AL error codes — values from <AL/al.h>.
            0x0000 /* AL_NO_ERROR */         => b"No Error",
            0xA001 /* AL_INVALID_NAME */     => b"Invalid Name",
            0xA002 /* AL_INVALID_ENUM */     => b"Invalid Enum",
            0xA003 /* AL_INVALID_VALUE */    => b"Invalid Value",
            0xA004 /* AL_INVALID_OPERATION */=> b"Invalid Operation",
            0xA005 /* AL_OUT_OF_MEMORY */    => b"Out of Memory",
            other => {
                log!(
                    "Warning: alGetString({:#x}) called with unknown enum, \
                     returning empty string",
                    other
                );
                b""
            }
        };
        let new_str = env.mem.alloc_and_write_cstr(s).cast_const();
        env.framework_state
            .openal
            .strings_cache
            .insert(cache_key, new_str);
        new_str
    };
    log_dbg!(
        "alGetString({}) => '{:?}'",
        param,
        env.mem.cstr_at_utf8(res)
    );
    res
}

fn alListenerf(env: &mut Environment, param: ALenum, value: ALfloat) {
    try_get_context!(env, context);
    unsafe { context.Listenerf(param, value) };
}
fn alListenerfv(env: &mut Environment, param: ALenum, values: ConstPtr<ALfloat>) {
    // мы предполагаем, что должен быть передан хотя бы 1 параметр
    let values = env.mem.ptr_at(values, 1);
    try_get_context!(env, context);
    unsafe { context.Listenerfv(param, values) };
}
fn alListener3f(
    env: &mut Environment,

    param: ALenum,
    value1: ALfloat,
    value2: ALfloat,
    value3: ALfloat,
) {
    try_get_context!(env, context);
    unsafe { context.Listener3f(param, value1, value2, value3) };
}
fn alListeneri(env: &mut Environment, param: ALenum, value: ALint) {
    try_get_context!(env, context);
    unsafe { context.Listeneri(param, value) };
}
fn alListener3i(env: &mut Environment, param: ALenum, value1: ALint, value2: ALint, value3: ALint) {
    try_get_context!(env, context);
    unsafe { context.Listener3i(param, value1, value2, value3) };
}
fn alListeneriv(env: &mut Environment, param: ALenum, values: ConstPtr<ALint>) {
    let values = env.mem.ptr_at(values, 3); // верхняя граница
    try_get_context!(env, context);
    unsafe { context.Listeneriv(param, values) };
}

fn alGetListenerf(env: &mut Environment, param: ALenum, value: MutPtr<ALfloat>) {
    let value = env.mem.ptr_at_mut(value, 1);
    try_get_context!(env, context);
    unsafe { context.GetListenerf(param, value) };
}
fn alGetListener3f(
    env: &mut Environment,

    param: ALenum,
    value1: MutPtr<ALfloat>,
    value2: MutPtr<ALfloat>,
    value3: MutPtr<ALfloat>,
) {
    try_get_context!(env, context);
    let mut values = [0.0; 3];
    unsafe { context.GetListener3f(param, &mut values[0], &mut values[1], &mut values[2]) };
    env.mem.write(value1, values[0]);
    env.mem.write(value2, values[1]);
    env.mem.write(value3, values[2]);
}
fn alGetListenerfv(env: &mut Environment, param: ALenum, values: MutPtr<ALfloat>) {
    let values = env.mem.ptr_at_mut(values, 3); // верхняя граница
    try_get_context!(env, context);
    unsafe { context.GetListenerfv(param, values) };
}
fn alGetListeneri(env: &mut Environment, param: ALenum, value: MutPtr<ALint>) {
    let value = env.mem.ptr_at_mut(value, 1);
    try_get_context!(env, context);
    unsafe { context.GetListeneri(param, value) };
}
fn alGetListener3i(
    env: &mut Environment,

    param: ALenum,
    value1: MutPtr<ALint>,
    value2: MutPtr<ALint>,
    value3: MutPtr<ALint>,
) {
    let mut values = [0; 3];
    try_get_context!(env, context);
    unsafe { context.GetListener3i(param, &mut values[0], &mut values[1], &mut values[2]) };
    env.mem.write(value1, values[0]);
    env.mem.write(value2, values[1]);
    env.mem.write(value3, values[2]);
}
fn alGetListeneriv(env: &mut Environment, param: ALenum, values: MutPtr<ALint>) {
    let values = env.mem.ptr_at_mut(values, 3); // верхняя граница
    try_get_context!(env, context);
    unsafe { context.GetListeneriv(param, values) };
}

fn alGenSources(env: &mut Environment, n: ALsizei, sources: MutPtr<ALuint>) {
    let n_usize: GuestUSize = match n.try_into() {
        Ok(val) => val,
        Err(_) => {
            log!(
                "Предупреждение: alGenSources вызван с отрицательным количеством {}",
                n
            );
            return;
        }
    };
    let sources = env.mem.ptr_at_mut(sources, n_usize);
    try_get_context!(env, context);
    unsafe { context.GenSources(n, sources) };
}
fn alDeleteSources(env: &mut Environment, n: ALsizei, sources: ConstPtr<ALuint>) {
    let n_usize: GuestUSize = match n.try_into() {
        Ok(val) => val,
        Err(_) => {
            log!(
                "Предупреждение: alDeleteSources вызван с отрицательным количеством {}",
                n
            );
            return;
        }
    };
    let sources = env.mem.ptr_at(sources, n_usize);
    let Some(context) = State::try_make_current(env) else {
        log_dbg!(
            "alDeleteSources({}, {:?}) called with no active context {:?} — no-op per OpenAL spec",
            n,
            sources,
            State::get(env).current_ctx
        );
        return;
    };
    unsafe { context.DeleteSources(n, sources) };
}

fn alSourcef(env: &mut Environment, source: ALuint, param: ALenum, value: ALfloat) {
    try_get_context!(env, context);
    unsafe { context.Sourcef(source, param, value) };
}
fn alSourcefv(env: &mut Environment, source: ALuint, param: ALenum, values: ConstPtr<ALfloat>) {
    // мы предполагаем, что должен быть передан хотя бы 1 параметр
    let values = env.mem.ptr_at(values, 1);
    try_get_context!(env, context);
    unsafe { context.Sourcefv(source, param, values) };
}
fn alSource3f(
    env: &mut Environment,
    source: ALuint,
    param: ALenum,
    value1: ALfloat,
    value2: ALfloat,
    value3: ALfloat,
) {
    try_get_context!(env, context);
    unsafe { context.Source3f(source, param, value1, value2, value3) };
}
fn alSourcei(env: &mut Environment, source: ALuint, param: ALenum, value: ALint) {
    try_get_context!(env, context);
    unsafe { context.Sourcei(source, param, value) };
}
fn alSource3i(
    env: &mut Environment,
    source: ALuint,
    param: ALenum,
    value1: ALint,
    value2: ALint,
    value3: ALint,
) {
    try_get_context!(env, context);
    unsafe { context.Source3i(source, param, value1, value2, value3) };
}
fn alSourceiv(env: &mut Environment, source: ALuint, param: ALenum, values: ConstPtr<ALint>) {
    let values = env.mem.ptr_at(values, 3); // верхняя граница
    try_get_context!(env, context);
    unsafe { context.Sourceiv(source, param, values) };
}

fn alGetSourcef(env: &mut Environment, source: ALuint, param: ALenum, value: MutPtr<ALfloat>) {
    let value = env.mem.ptr_at_mut(value, 1);
    try_get_context!(env, context);
    unsafe { context.GetSourcef(source, param, value) };
}
fn alGetSource3f(
    env: &mut Environment,
    source: ALuint,
    param: ALenum,
    value1: MutPtr<ALfloat>,
    value2: MutPtr<ALfloat>,
    value3: MutPtr<ALfloat>,
) {
    let mut values = [0.0; 3];
    try_get_context!(env, context);
    unsafe {
        context.GetSource3f(
            source,
            param,
            &mut values[0],
            &mut values[1],
            &mut values[2],
        )
    };
    env.mem.write(value1, values[0]);
    env.mem.write(value2, values[1]);
    env.mem.write(value3, values[2]);
}
fn alGetSourcefv(env: &mut Environment, source: ALuint, param: ALenum, values: MutPtr<ALfloat>) {
    let values = env.mem.ptr_at_mut(values, 3); // верхняя граница
    try_get_context!(env, context);
    unsafe { context.GetSourcefv(source, param, values) };
}
fn alGetSourcei(env: &mut Environment, source: ALuint, param: ALenum, value: MutPtr<ALint>) {
    let value = env.mem.ptr_at_mut(value, 1);
    try_get_context!(env, context);
    unsafe { context.GetSourcei(source, param, value) };
}
fn alGetSource3i(
    env: &mut Environment,
    source: ALuint,
    param: ALenum,
    value1: MutPtr<ALint>,
    value2: MutPtr<ALint>,
    value3: MutPtr<ALint>,
) {
    let mut values = [0; 3];
    try_get_context!(env, context);
    unsafe {
        context.GetSource3i(
            source,
            param,
            &mut values[0],
            &mut values[1],
            &mut values[2],
        )
    };
    env.mem.write(value1, values[0]);
    env.mem.write(value2, values[1]);
    env.mem.write(value3, values[2]);
}
fn alGetSourceiv(env: &mut Environment, source: ALuint, param: ALenum, values: MutPtr<ALint>) {
    let values = env.mem.ptr_at_mut(values, 3); // верхняя граница
    try_get_context!(env, context);
    unsafe { context.GetSourceiv(source, param, values) };
}

fn alSourcePlay(env: &mut Environment, source: ALuint) {
    try_get_context!(env, context);
    unsafe { context.SourcePlay(source) };
    // Streaming sources call SourcePlay every audio buffer refill, so the
    // post-play diagnostic readback is only worth doing (and printing) when
    // debug logging for this module is enabled.
    if crate::log::ENABLED_MODULES.contains(&module_path!()) {
        let mut state: ALint = 0;
        let mut max_gain: ALfloat = 0.0;
        let mut buffer: ALint = 0;
        let err: ALenum;
        unsafe {
            context.GetSourcei(source, al::AL_SOURCE_STATE, &mut state as *mut _);
            context.GetSourcef(source, al::AL_MAX_GAIN, &mut max_gain as *mut _);
            context.GetSourcei(source, al::AL_BUFFER, &mut buffer as *mut _);
            err = context.GetError();
        }
        log_dbg!(
            "alSourcePlay(source={}) -> state=0x{:x}, max_gain={}, buffer={}, err=0x{:x}",
            source,
            state,
            max_gain,
            buffer,
            err
        );
    }
}
fn alSourcePause(env: &mut Environment, source: ALuint) {
    try_get_context!(env, context);
    unsafe { context.SourcePause(source) };
}
fn alSourceStop(env: &mut Environment, source: ALuint) {
    try_get_context!(env, context);
    unsafe { context.SourceStop(source) };
}
fn alSourceRewind(env: &mut Environment, source: ALuint) {
    try_get_context!(env, context);
    unsafe { context.SourceRewind(source) };
}

fn alSourceQueueBuffers(
    env: &mut Environment,
    source: ALuint,
    nb: ALsizei,
    buffers: ConstPtr<ALuint>,
) {
    let nb_usize: GuestUSize = match nb.try_into() {
        Ok(val) => val,
        Err(_) => {
            log!(
                "Предупреждение: alSourceQueueBuffers вызван с отрицательным количеством {}",
                nb
            );
            return;
        }
    };
    let buffers = env.mem.ptr_at(buffers, nb_usize);
    try_get_context!(env, context);
    unsafe { context.SourceQueueBuffers(source, nb, buffers) }
}
fn alSourceUnqueueBuffers(
    env: &mut Environment,
    source: ALuint,
    nb: ALsizei,
    buffers: MutPtr<ALuint>,
) {
    // Пример кода Apple для зацикленного звукового эффекта содержит функцию с
    // названием
    // SoundEngineEffect::ClearSourceBuffers(), которая имеет следующий шаблон:
    //
    //    alGetSourcei(source, AL_BUFFERS_QUEUED, &n);
    //    alSourceUnqueueBuffers(source, n, &buffers);
    //
    // К сожалению, при некоторых обстоятельствах этот код некорректен:
    // извлечение буферов из
    // очереди во время их воспроизведения не разрешено спецификацией OpenAL!
    // Возможно, это по
    // какой-то причине работало с реализацией OpenAL от Apple, но OpenAL Soft
    // этого не допускает,
    // поэтому многие приложения, использовавшие этот пример (например, Super
    // Monkey Ball),
    // сталкиваются с неожиданной ошибкой OpenAL.
    //
    // Ограничение количества извлекаемых буферов кажется эффективным обходным
    // путем для
    // протестированных приложений. Этот пример кода на самом деле не использует
    // возвращаемые
    // идентификаторы буферов, поэтому нет проблемы в том, что мы запишем их
    // меньше.
    try_get_context!(env, context);
    let buffers_processed = {
        let mut val = 0;
        unsafe { context.GetSourcei(source, al::AL_BUFFERS_PROCESSED, &mut val) };
        val
    };
    let nb = if buffers_processed < nb {
        log_dbg!("Применяем обходной путь для бага в примере кода Apple: игнорируем удаление {}/{} обработанных буферов из очереди для источника {}", nb, buffers_processed, source);
        buffers_processed
    } else {
        nb
    };

    let nb_usize: GuestUSize = match nb.try_into() {
        Ok(val) => val,
        Err(_) => {
            log!("Предупреждение: alSourceUnqueueBuffers в итоге получил отрицательное количество {}", nb);
            return;
        }
    };
    let buffers = env.mem.ptr_at_mut(buffers, nb_usize);
    unsafe { context.SourceUnqueueBuffers(source, nb, buffers) }
}

fn alGenBuffers(env: &mut Environment, n: ALsizei, buffers: MutPtr<ALuint>) {
    let n_usize: GuestUSize = match n.try_into() {
        Ok(val) => val,
        Err(_) => {
            log!(
                "Предупреждение: alGenBuffers вызван с отрицательным количеством {}",
                n
            );
            return;
        }
    };
    let buffers = env.mem.ptr_at_mut(buffers, n_usize);
    try_get_context!(env, context);
    unsafe { context.GenBuffers(n, buffers) };
}
fn alDeleteBuffers(env: &mut Environment, n: ALsizei, buffers: ConstPtr<ALuint>) {
    let n_usize: GuestUSize = match n.try_into() {
        Ok(val) => val,
        Err(_) => {
            log!(
                "Предупреждение: alDeleteBuffers вызван с отрицательным количеством {}",
                n
            );
            return;
        }
    };
    let buffers = env.mem.ptr_at(buffers, n_usize);
    let Some(context) = State::try_make_current(env) else {
        log_dbg!(
            "alDeleteBuffers({}, {:?}) called with no active context {:?} — no-op per OpenAL spec",
            n,
            buffers,
            State::get(env).current_ctx
        );
        return;
    };
    unsafe { context.DeleteBuffers(n, buffers) };
}

fn alBufferData(
    env: &mut Environment,
    buffer: ALuint,
    format: ALenum,
    data: ConstVoidPtr,
    size: ALsizei,
    samplerate: ALsizei,
) {
    let size_usize: GuestUSize = match size.try_into() {
        Ok(val) => val,
        Err(_) => {
            log!(
                "Предупреждение: alBufferData вызван с отрицательным размером {}",
                size
            );
            return;
        }
    };
    let data_ptr: *const ALvoid = if data.is_null() {
        std::ptr::null()
    } else {
        let data_slice = env.mem.bytes_at(data.cast(), size_usize);
        data_slice.as_ptr() as *const _
    };
    // Streaming audio refills the same buffers many times per second, so this
    // log line is debug-only to avoid drowning out other diagnostics.
    log_dbg!(
        "alBufferData(buffer={}, format=0x{:x}, size={}, samplerate={})",
        buffer,
        format,
        size,
        samplerate
    );
    try_get_context!(env, context);
    unsafe { context.BufferData(buffer, format, data_ptr, size, samplerate) };
}

/// Это расширение Apple, которое рассматривает переданные данные как
//статический буфер,
/// а не временный, что означает, что его никогда не нужно копировать.
/// OpenAL Soft это не поддерживает, поэтому мы передаем управление в
//`alBufferData`
/// и надеемся, что гостевое приложение не полагается на статичность (не
//должно).
fn alBufferDataStatic(
    env: &mut Environment,
    buffer: ALuint,
    format: ALenum,
    data: ConstVoidPtr,
    size: ALsizei,
    samplerate: ALsizei,
) {
    alBufferData(env, buffer, format, data, size, samplerate);
}

// Специфичное расширение Apple для OpenAL
fn alcMacOSXMixerOutputRate(_env: &mut Environment, value: ALdouble) {
    log!(
        "Приложение хочет установить частоту дискретизации микшера на {} Гц",
        value
    );
}
fn alcMacOSXGetMixerOutputRate(_env: &mut Environment) -> ALdouble {
    // Значение по умолчанию было проверено на iPhone 3GS, iOS 4.0.1
    log!("Приложение хочет получить частоту дискретизации микшера, возвращаем 0 по умолчанию");
    0.0
}

fn alDopplerFactor(env: &mut Environment, value: ALfloat) {
    try_get_context!(env, context);
    unsafe { context.DopplerFactor(value) };
}

fn alDopplerVelocity(env: &mut Environment, value: ALfloat) {
    // По всей видимости, wolf3d устанавливает скорость Доплера в ноль, но это
    // приводит
    // к приглушению всего звука в программной реализации Open AL 1.1!
    // Дополнительную информацию см. в разделе "A note for OpenAL library
    // implementors regarding OpenAL 1.0"
    // спецификации OpenAL 1.1.
    let bundle_id = env.bundle.bundle_identifier();
    if bundle_id.starts_with("com.zodttd.wolf3d")
        || bundle_id.starts_with("com.idsoftware.wolf3d")
        || bundle_id.starts_with("nu.r3.wolf3d")
    {
        // ИСПРАВЛЕНИЕ: Блокируем вызов только если игра реально передает 0.0,
        // чтобы не сломать звук. Жесткий assert убран.
        if value == 0.0 {
            log_dbg!("Применяем хак для Wolf3D-iOS: игнорируем нулевую скорость Доплера (0.0).");
            return;
        }
    }

    // Если передано нормальное значение (например, 1.0) или это другая игра —
    // честно отдаем в OpenAL
    try_get_context!(env, context);
    unsafe { context.DopplerVelocity(value) };
}

fn alSpeedOfSound(env: &mut Environment, value: ALfloat) {
    try_get_context!(env, context);
    unsafe { context.SpeedOfSound(value) };
}

// TODO: больше функций

// Примечание: По некоторым причинам Wolf3d регистрирует много функций OpenAL,
// но фактически использует лишь несколько. Чтобы обойти это, мы просто
// предоставляем заглушки.

// The following functions used to `todo!()` (i.e. panic) — that is wrong
// behaviour for emulator-facing APIs. Per OpenAL 1.1 spec, querying with an
// unsupported enum should set AL_INVALID_ENUM and return a zero/empty value;
// it must not abort the program. Apps such as Farm Frenzy call these from
// regular gameplay code paths and crash the emulator on what should be a
// soft failure.

/// `alcGetEnumValue(device, enumName)` — per the OpenAL 1.1 spec §6.3.4
/// the enum-name → enum-int lookup is delegated to the implementation.
/// We forward to the host OpenAL Soft so that standard tokens (e.g.
/// `ALC_FREQUENCY`) resolve correctly. Apple-extension tokens that the
/// host doesn't know fall back to 0 (the spec-mandated "no match"
/// sentinel) just as on Apple's stock implementation.
fn alcGetEnumValue(
    env: &mut Environment,
    device: MutPtr<GuestALCdevice>,
    enum_name: ConstPtr<u8>,
) -> ALenum {
    let host_device = *State::get(env).devices.get(&device).unwrap_or(&std::ptr::null_mut());
    let Ok(s) = env.mem.cstr_at_utf8(enum_name) else {
        log!("Warning: alcGetEnumValue({:?}): name is not valid UTF-8, returning 0", enum_name);
        return 0;
    };
    let cs = match CString::new(s) {
        Ok(cs) => cs,
        Err(_) => return 0,
    };
    let res = unsafe { al::alcGetEnumValue(host_device, cs.as_ptr()) };
    log_dbg!("alcGetEnumValue({:?}) => {:#x}", s, res);
    res
}

/// `alcGetIntegerv(device, param, size, values)` — per OpenAL 1.1 §6.3.4
/// returns the host-side integer attribute(s) for `param`. We forward to
/// host OpenAL Soft whenever the guest passed a non-null device handle
/// that we recognise (so e.g. ALC_FREQUENCY/ALC_REFRESH/ALC_SYNC/
/// ALC_MAJOR_VERSION/ALC_MINOR_VERSION/ALC_ATTRIBUTES_SIZE/
/// ALC_ALL_ATTRIBUTES/ALC_CAPTURE_SAMPLES all resolve to real values).
/// When the device is NULL or unknown only the version queries
/// (ALC_MAJOR_VERSION = 0x1000, ALC_MINOR_VERSION = 0x1001) are valid
/// per the spec; we still forward those to the host.
fn alcGetIntegerv(
    env: &mut Environment,
    device: MutPtr<GuestALCdevice>,
    param: ALenum,
    size: ALCsizei,
    values: MutPtr<ALCint>,
) {
    if values.is_null() || size <= 0 {
        return;
    }
    let host_device = State::get(env)
        .devices
        .get(&device)
        .copied()
        .unwrap_or(std::ptr::null_mut());
    let n = size as usize;
    let mut buf: Vec<ALCint> = vec![0; n];
    unsafe { al::alcGetIntegerv(host_device, param, size, buf.as_mut_ptr()) };
    for (i, v) in buf.iter().enumerate() {
        env.mem.write(values + (i as u32) * 4, *v);
    }
    log_dbg!(
        "alcGetIntegerv({:?}, {:#x}, {}) => {:?}",
        device, param, size, buf
    );
}

/// `alcIsExtensionPresent(device, extName)` — forwards to the host
/// OpenAL Soft (OpenAL 1.1 §6.3.3). Apple's documented OpenAL
/// extension strings ("ALC_EXT_ASA", "ALC_EXT_MAC_OSX", etc.) aren't
/// advertised by stock OpenAL Soft, which is consistent with running
/// on a non-Apple device — apps that probe for them and gracefully
/// degrade will simply skip the affected code paths.
fn alcIsExtensionPresent(
    env: &mut Environment,
    device: MutPtr<GuestALCdevice>,
    ext_name: ConstPtr<u8>,
) -> ALCboolean {
    let host_device = State::get(env)
        .devices
        .get(&device)
        .copied()
        .unwrap_or(std::ptr::null_mut());
    let Ok(s) = env.mem.cstr_at_utf8(ext_name) else {
        return 0;
    };
    let cs = match CString::new(s) {
        Ok(cs) => cs,
        Err(_) => return 0,
    };
    let res = unsafe { al::alcIsExtensionPresent(host_device, cs.as_ptr()) };
    log_dbg!("alcIsExtensionPresent({:?}) => {}", s, res);
    res
}

// === Apple OpenAL extensions ===
//
// References:
// - oalSourceNotifications_OALExtensions.h (Apple, OpenAL Source
//   Notifications Extension) — declares `alSourceAddNotification` /
//   `alSourceRemoveNotification` plus the AL_QUEUE_HAS_LOOPED enum
//   (0x9000) used as `notificationID`.
// - oalMacOSX_OALExtensions.h (Apple, OpenAL Mac OS X Extensions) —
//   declares the Apple Sound API (ASA) `alcASAGet*`/`alcASASet*` family
//   plus `alcMacOSXRenderingQuality` / `alcMacOSXGetRenderingQuality`.
// - <https://developer.apple.com/library/archive/technotes/tn2199/_index.html>
//
// touchHLE backs onto stock OpenAL Soft, which doesn't implement these
// Apple-specific extensions. Per the extension specifications the
// canonical "not supported" return is AL_NO_ERROR / 0 with the
// requested data left zeroed — apps detect feature presence via
// `alIsExtensionPresent("AL_EXT_SOURCE_NOTIFICATIONS")` /
// `alcIsExtensionPresent(device, "ALC_EXT_ASA")` before configuring
// these properties, so a benign no-op is the spec-compliant fallback.

/// `ALenum alSourceAddNotification(ALuint sid, ALuint notificationID,
///                                 alSourceNotificationProc proc,
///                                 ALvoid *userData)` (Apple).
///
/// Registers a callback for source-state notifications such as
/// `AL_QUEUE_HAS_LOOPED` (0x9000) and `AL_BUFFERS_PROCESSED` (0x1016).
/// Apple's headers specify a return of `AL_NO_ERROR` on success.
fn alSourceAddNotification(
    _env: &mut Environment,
    sid: ALuint,
    notification_id: ALuint,
    _proc: MutVoidPtr,
    _user_data: MutVoidPtr,
) -> ALenum {
    log_dbg!(
        "alSourceAddNotification(sid={}, notificationID={:#x}) — no-op (stock OpenAL Soft)",
        sid,
        notification_id
    );
    al::AL_NO_ERROR
}

/// `ALvoid alSourceRemoveNotification(ALuint sid, ALuint notificationID,
///                                    alSourceNotificationProc proc,
///                                    ALvoid *userData)` (Apple).
fn alSourceRemoveNotification(
    _env: &mut Environment,
    sid: ALuint,
    notification_id: ALuint,
    _proc: MutVoidPtr,
    _user_data: MutVoidPtr,
) {
    log_dbg!(
        "alSourceRemoveNotification(sid={}, notificationID={:#x}) — no-op",
        sid,
        notification_id
    );
}

/// `ALenum alcASAGetSource(const ALuint property, ALuint source,
///                         ALvoid *data, ALuint *dataSize)` (Apple).
///
/// ASA == Apple Sound API. Properties include
/// `ALC_ASA_REVERB_ON` (0x1) / `_ROOM_TYPE` (0x2) / `_PRESET` (0x3) /
/// `_OCCLUSION` (0x4) / `_OBSTRUCTION` (0x5).
fn alcASAGetSource(
    env: &mut Environment,
    property: ALuint,
    source: ALuint,
    data: MutVoidPtr,
    data_size: MutPtr<ALuint>,
) -> ALenum {
    log_dbg!(
        "alcASAGetSource(property={:#x}, source={}) — unsupported, zeroing out",
        property,
        source
    );
    // Per Apple's headers, write 0 / leave size at 0 when the
    // property is unknown. We zero the destination buffer if the
    // caller told us its size.
    if !data_size.is_null() {
        let n = env.mem.read(data_size);
        if !data.is_null() && n > 0 {
            let dst = env.mem.bytes_at_mut(data.cast(), n);
            for b in dst.iter_mut() {
                *b = 0;
            }
        }
    }
    al::AL_NO_ERROR
}

/// `ALenum alcASASetSource(const ALuint property, ALuint source,
///                         ALvoid *data, ALuint dataSize)` (Apple).
fn alcASASetSource(
    _env: &mut Environment,
    property: ALuint,
    source: ALuint,
    _data: MutVoidPtr,
    _data_size: ALuint,
) -> ALenum {
    log_dbg!(
        "alcASASetSource(property={:#x}, source={}) — no-op",
        property,
        source
    );
    al::AL_NO_ERROR
}

/// `ALenum alcASAGetListener(const ALuint property, ALvoid *data,
///                           ALuint *dataSize)` (Apple).
fn alcASAGetListener(
    env: &mut Environment,
    property: ALuint,
    data: MutVoidPtr,
    data_size: MutPtr<ALuint>,
) -> ALenum {
    log_dbg!(
        "alcASAGetListener(property={:#x}) — unsupported, zeroing out",
        property
    );
    if !data_size.is_null() {
        let n = env.mem.read(data_size);
        if !data.is_null() && n > 0 {
            let dst = env.mem.bytes_at_mut(data.cast(), n);
            for b in dst.iter_mut() {
                *b = 0;
            }
        }
    }
    al::AL_NO_ERROR
}

/// `ALenum alcASASetListener(const ALuint property, ALvoid *data,
///                           ALuint dataSize)` (Apple).
fn alcASASetListener(
    _env: &mut Environment,
    property: ALuint,
    _data: MutVoidPtr,
    _data_size: ALuint,
) -> ALenum {
    log_dbg!("alcASASetListener(property={:#x}) — no-op", property);
    al::AL_NO_ERROR
}

/// `ALvoid alcMacOSXRenderingQuality(const ALint quality)` (Apple).
///
/// Selects the spatial-rendering quality:
/// `ALC_IPHONE_SPATIAL_RENDERING_QUALITY_LOW` (1) /
/// `ALC_IPHONE_SPATIAL_RENDERING_QUALITY_HIGH` (2). On stock OpenAL
/// Soft the rendering pipeline is configured at device-open time and
/// can't be changed on the fly, so we just remember the request and
/// hand it back to `alcMacOSXGetRenderingQuality`.
fn alcMacOSXRenderingQuality(env: &mut Environment, quality: ALint) {
    log_dbg!("alcMacOSXRenderingQuality({}) — remembered", quality);
    env.framework_state.openal.macosx_rendering_quality = quality;
}

/// `ALint alcMacOSXGetRenderingQuality()` (Apple).
fn alcMacOSXGetRenderingQuality(env: &mut Environment) -> ALint {
    // Default: high quality, matching iPhone OS 4.x's documented value.
    let q = env.framework_state.openal.macosx_rendering_quality;
    if q == 0 {
        0x0002 // ALC_IPHONE_SPATIAL_RENDERING_QUALITY_HIGH
    } else {
        q
    }
}
fn alGetBufferf(env: &mut Environment, _buffer: ALuint, param: ALenum, value: MutPtr<ALfloat>) {
    log!(
        "Warning: alGetBufferf({:#x}) is a stub, returning 0.0",
        param
    );
    if !value.is_null() {
        env.mem.write(value, 0.0);
    }
}
fn alDisable(_env: &mut Environment, capability: ALenum) {
    log!("Warning: alDisable({:#x}) is a stub", capability);
}
fn alGetBoolean(_env: &mut Environment, param: ALenum) -> ALboolean {
    log!("Warning: alGetBoolean({:#x}) is a stub, returning 0", param);
    0
}
fn alGetBooleanv(env: &mut Environment, param: ALenum, values: MutPtr<ALboolean>) {
    log!("Warning: alGetBooleanv({:#x}) is a stub", param);
    if !values.is_null() {
        env.mem.write(values, 0);
    }
}
fn alGetDouble(_env: &mut Environment, param: ALenum) -> ALdouble {
    log!(
        "Warning: alGetDouble({:#x}) is a stub, returning 0.0",
        param
    );
    0.0
}
fn alGetDoublev(env: &mut Environment, param: ALenum, values: MutPtr<ALdouble>) {
    log!("Warning: alGetDoublev({:#x}) is a stub", param);
    if !values.is_null() {
        env.mem.write(values, 0.0);
    }
}
fn alGetFloat(_env: &mut Environment, param: ALenum) -> ALfloat {
    log!("Warning: alGetFloat({:#x}) is a stub, returning 0.0", param);
    0.0
}
fn alGetFloatv(env: &mut Environment, param: ALenum, values: MutPtr<ALfloat>) {
    log!("Warning: alGetFloatv({:#x}) is a stub", param);
    if !values.is_null() {
        env.mem.write(values, 0.0);
    }
}
fn alGetInteger(_env: &mut Environment, param: ALenum) -> ALint {
    log!("Warning: alGetInteger({:#x}) is a stub, returning 0", param);
    0
}
fn alGetIntegerv(env: &mut Environment, param: ALenum, values: MutPtr<ALint>) {
    log!("Warning: alGetIntegerv({:#x}) is a stub", param);
    if !values.is_null() {
        env.mem.write(values, 0);
    }
}
fn alGetProcAddress(env: &mut Environment, funcName: ConstPtr<u8>) -> MutVoidPtr {
    alcGetProcAddress(env, Ptr::null(), funcName)
}
fn alIsEnabled(_env: &mut Environment, _capability: ALenum) -> ALboolean {
    0
}
fn alSourcePlayv(env: &mut Environment, nsources: ALsizei, sources: ConstPtr<ALuint>) {
    if sources.is_null() || nsources <= 0 {
        return;
    }
    for i in 0..nsources {
        let src: ALuint = env.mem.read(sources + i as u32);
        alSourcePlay(env, src);
    }
}
fn alSourcePausev(env: &mut Environment, nsources: ALsizei, sources: ConstPtr<ALuint>) {
    if sources.is_null() || nsources <= 0 {
        return;
    }
    for i in 0..nsources {
        let src: ALuint = env.mem.read(sources + i as u32);
        alSourcePause(env, src);
    }
}
fn alSourceStopv(env: &mut Environment, nsources: ALsizei, sources: ConstPtr<ALuint>) {
    if sources.is_null() || nsources <= 0 {
        return;
    }
    for i in 0..nsources {
        let src: ALuint = env.mem.read(sources + i as u32);
        alSourceStop(env, src);
    }
}
fn alSourceRewindv(env: &mut Environment, nsources: ALsizei, sources: ConstPtr<ALuint>) {
    if sources.is_null() || nsources <= 0 {
        return;
    }
    for i in 0..nsources {
        let src: ALuint = env.mem.read(sources + i as u32);
        alSourceRewind(env, src);
    }
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(alcOpenDevice(_)),
    export_c_func!(alcCloseDevice(_)),
    export_c_func!(alcGetError(_)),
    export_c_func!(alcCreateContext(_, _)),
    export_c_func!(alcDestroyContext(_)),
    export_c_func!(alcProcessContext(_)),
    export_c_func!(alcSuspendContext(_)),
    export_c_func!(alcMakeContextCurrent(_)),
    export_c_func!(alcGetProcAddress(_, _)),
    export_c_func!(alGetError()),
    export_c_func!(alDistanceModel(_)),
    export_c_func!(alListenerf(_, _)),
    export_c_func!(alListener3f(_, _, _, _)),
    export_c_func!(alListenerfv(_, _)),
    export_c_func!(alListeneri(_, _)),
    export_c_func!(alListener3i(_, _, _, _)),
    export_c_func!(alListeneriv(_, _)),
    export_c_func!(alGetListenerf(_, _)),
    export_c_func!(alGetListener3f(_, _, _, _)),
    export_c_func!(alGetListenerfv(_, _)),
    export_c_func!(alGetListeneri(_, _)),
    export_c_func!(alGetListener3i(_, _, _, _)),
    export_c_func!(alGetListeneriv(_, _)),
    export_c_func!(alGenSources(_, _)),
    export_c_func!(alDeleteSources(_, _)),
    export_c_func!(alSourcef(_, _, _)),
    export_c_func!(alSource3f(_, _, _, _, _)),
    export_c_func!(alSourcefv(_, _, _)),
    export_c_func!(alSourcei(_, _, _)),
    export_c_func!(alSource3i(_, _, _, _, _)),
    export_c_func!(alSourceiv(_, _, _)),
    export_c_func!(alGetSourcef(_, _, _)),
    export_c_func!(alGetSource3f(_, _, _, _, _)),
    export_c_func!(alGetSourcefv(_, _, _)),
    export_c_func!(alGetSourcei(_, _, _)),
    export_c_func!(alGetSource3i(_, _, _, _, _)),
    export_c_func!(alGetSourceiv(_, _, _)),
    export_c_func!(alSourcePlay(_)),
    export_c_func!(alSourcePause(_)),
    export_c_func!(alSourceStop(_)),
    export_c_func!(alSourceRewind(_)),
    export_c_func!(alSourceQueueBuffers(_, _, _)),
    export_c_func!(alSourceUnqueueBuffers(_, _, _)),
    export_c_func!(alGenBuffers(_, _)),
    export_c_func!(alDeleteBuffers(_, _)),
    export_c_func!(alBufferData(_, _, _, _, _)),
    export_c_func!(alBufferDataStatic(_, _, _, _, _)),
    export_c_func!(alcMacOSXMixerOutputRate(_)),
    export_c_func!(alcMacOSXGetMixerOutputRate()),
    export_c_func!(alcGetContextsDevice(_)),
    export_c_func!(alcGetCurrentContext()),
    export_c_func!(alcGetEnumValue(_, _)),
    export_c_func!(alcGetIntegerv(_, _, _, _)),
    export_c_func!(alcGetString(_, _)),
    export_c_func!(alcIsExtensionPresent(_, _)),
    export_c_func!(alIsBuffer(_)),
    export_c_func!(alGetBufferf(_, _, _)),
    export_c_func!(alGetBufferi(_, _, _)),
    export_c_func!(alEnable(_)),
    export_c_func!(alDisable(_)),
    export_c_func!(alDopplerFactor(_)),
    export_c_func!(alDopplerVelocity(_)),
    export_c_func!(alGetBoolean(_)),
    export_c_func!(alGetBooleanv(_, _)),
    export_c_func!(alGetDouble(_)),
    export_c_func!(alGetDoublev(_, _)),
    export_c_func!(alGetFloat(_)),
    export_c_func!(alGetFloatv(_, _)),
    export_c_func!(alGetInteger(_)),
    export_c_func!(alGetIntegerv(_, _)),
    export_c_func!(alGetEnumValue(_)),
    export_c_func!(alGetProcAddress(_)),
    export_c_func!(alGetString(_)),
    export_c_func!(alIsExtensionPresent(_)),
    export_c_func!(alIsEnabled(_)),
    export_c_func!(alIsSource(_)),
    export_c_func!(alSourcePlayv(_, _)),
    export_c_func!(alSourcePausev(_, _)),
    export_c_func!(alSourceStopv(_, _)),
    export_c_func!(alSourceRewindv(_, _)),
    export_c_func!(alSpeedOfSound(_)),
    // Apple OpenAL extensions — see `oalSourceNotifications_OALExtensions.h`
    // and `oalMacOSX_OALExtensions.h`.
    export_c_func!(alSourceAddNotification(_, _, _, _)),
    export_c_func!(alSourceRemoveNotification(_, _, _, _)),
    export_c_func!(alcASAGetSource(_, _, _, _)),
    export_c_func!(alcASASetSource(_, _, _, _)),
    export_c_func!(alcASAGetListener(_, _, _)),
    export_c_func!(alcASASetListener(_, _, _)),
    export_c_func!(alcMacOSXRenderingQuality(_)),
    export_c_func!(alcMacOSXGetRenderingQuality()),
];
