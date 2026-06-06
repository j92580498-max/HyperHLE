/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `malloc/malloc.h` memory zones.
//!
//! Unlike Apple's libmalloc, this fork backs every zone with the single
//! global guest heap (see [crate::mem]). Apps that go through the zone API
//! (e.g. `malloc_create_zone` + `malloc_zone_malloc`) get correct behaviour;
//! the per-zone heap isolation that the real allocator provides is purely an
//! optimisation and is not relied upon by guest code we target.

use crate::abi::GuestFunction;
use crate::dyld::FunctionExports;
use crate::environment::Environment;
use crate::export_c_func;
use crate::libc::string::strdup;
use crate::mem::{ConstPtr, ConstVoidPtr, GuestUSize, MutPtr, MutVoidPtr, Ptr, SafeRead};

#[derive(Default)]
pub struct State {
    default_zone: Option<MutPtr<malloc_zone_t>>,
}

#[repr(C, packed)]
#[allow(non_camel_case_types)]
pub struct malloc_zone_t {
    reserved1: MutVoidPtr,
    reserved2: MutVoidPtr,
    size: MutVoidPtr,
    malloc: MutVoidPtr,
    calloc: MutVoidPtr,
    valloc: MutVoidPtr,
    free: MutVoidPtr,
    realloc: MutVoidPtr,
    destroy: MutVoidPtr,
    zone_name: ConstPtr<u8>,
    batch_malloc: MutVoidPtr,
    batch_free: MutVoidPtr,
    introspect: MutVoidPtr,
    version: u32,
    memalign: MutVoidPtr,
}
unsafe impl SafeRead for malloc_zone_t {}

impl malloc_zone_t {
    fn new(env: &mut Environment) -> malloc_zone_t {
        let fn_ptr = |env: &mut Environment, symbol: &str| -> MutVoidPtr {
            let f: GuestFunction = env
                .dyld
                .create_proc_address(&mut env.mem, &mut env.cpu, symbol)
                .unwrap_or_else(|_| panic!("Failed to create function pointer for {symbol}"));
            f.to_ptr().cast_mut()
        };
        malloc_zone_t {
            reserved1: Ptr::null(),
            reserved2: Ptr::null(),
            size: fn_ptr(env, "_malloc_zone_size"),
            malloc: fn_ptr(env, "_malloc_zone_malloc"),
            calloc: Ptr::null(),
            valloc: Ptr::null(),
            free: fn_ptr(env, "_malloc_zone_free"),
            realloc: fn_ptr(env, "_malloc_zone_realloc"),
            destroy: fn_ptr(env, "_malloc_destroy_zone"),
            zone_name: Ptr::null(),
            batch_malloc: Ptr::null(),
            batch_free: Ptr::null(),
            introspect: Ptr::null(),
            version: 0,
            memalign: Ptr::null(),
        }
    }
}

fn malloc_default_zone(env: &mut Environment) -> MutPtr<malloc_zone_t> {
    if env.libc_state.malloc.default_zone.is_none() {
        let zone_data = malloc_zone_t::new(env);
        let zone = env.mem.alloc_and_write(zone_data);
        env.libc_state.malloc.default_zone = Some(zone);
    }

    env.libc_state.malloc.default_zone.unwrap()
}

fn malloc_create_zone(
    env: &mut Environment,
    _start_size: GuestUSize,
    flags: u32,
) -> MutPtr<malloc_zone_t> {
    assert_eq!(flags, 0);
    // All zones are backed by the single global heap, so a created zone is
    // just another `malloc_zone_t` struct.
    let zone_data = malloc_zone_t::new(env);
    env.mem.alloc_and_write(zone_data)
}

fn malloc_destroy_zone(env: &mut Environment, zone: MutPtr<malloc_zone_t>) {
    if zone == malloc_default_zone(env) {
        panic!("Attempted to destroy default zone");
    }
    let zone_name = env.mem.read(zone).zone_name;
    if !zone_name.is_null() {
        env.mem.free(zone_name.cast_mut().cast());
    }
    env.mem.free(zone.cast());
}

fn malloc_zone_free(env: &mut Environment, _zone: MutPtr<malloc_zone_t>, ptr: MutVoidPtr) {
    env.mem.free(ptr)
}

fn malloc_zone_malloc(
    env: &mut Environment,
    _zone: MutPtr<malloc_zone_t>,
    size: GuestUSize,
) -> MutVoidPtr {
    env.mem.alloc(size)
}

fn malloc_zone_realloc(
    env: &mut Environment,
    _zone: MutPtr<malloc_zone_t>,
    ptr: MutVoidPtr,
    size: GuestUSize,
) -> MutVoidPtr {
    env.mem.realloc(ptr, size)
}

fn malloc_zone_size(
    env: &mut Environment,
    _zone: MutPtr<malloc_zone_t>,
    ptr: ConstVoidPtr,
) -> GuestUSize {
    if ptr.is_null() {
        return 0;
    }
    env.mem.malloc_size(ptr)
}

/// Not a part of the API. However as such a function needs to be accessible in
/// the zone struct, it has to be exported.
fn malloc_set_zone_name(env: &mut Environment, zone: MutPtr<malloc_zone_t>, name: ConstPtr<u8>) {
    let name = strdup(env, name).cast_const();
    let mut zone_data = env.mem.read(zone);
    zone_data.zone_name = name;
    env.mem.write(zone, zone_data);
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(malloc_create_zone(_, _)),
    export_c_func!(malloc_default_zone()),
    export_c_func!(malloc_destroy_zone(_)),
    export_c_func!(malloc_zone_free(_, _)),
    export_c_func!(malloc_zone_malloc(_, _)),
    export_c_func!(malloc_zone_realloc(_, _, _)),
    export_c_func!(malloc_zone_size(_, _)),
    export_c_func!(malloc_set_zone_name(_, _)),
];
