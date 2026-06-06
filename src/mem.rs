/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Types related to the virtual memory of the emulated application, or the
//! "guest memory".
//!
//! The virtual address space is 32-bit, as is the pointer size.
//!
//! No attempt is made to do endianness conversion for reads and writes to
//! memory, because all supported emulated and host platforms are little-endian.
//!
//! Relevant Apple documentation:
//! * [Memory Usage Performance Guidelines](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/ManagingMemory/ManagingMemory.html)

use std::num::NonZeroU32;

use crate::libc::wchar::wchar_t;

mod allocator;
mod host;

/// Equivalent of `usize` for guest memory.
pub type GuestUSize = u32;

/// Equivalent of `isize` for guest memory.
pub type GuestISize = i32;

/// Nonzero version of [GuestUSize].
pub type NonZeroGuestUSize = NonZeroU32;

/// [std::mem::size_of], but returning a [GuestUSize].
pub const fn guest_size_of<T: Sized>() -> GuestUSize {
    assert!(std::mem::size_of::<T>() <= u32::MAX as usize);
    std::mem::size_of::<T>() as u32
}

/// Internal type for representing an untyped virtual address.
type VAddr = GuestUSize;

/// Internal type for representing an untyped virtual address.
type NonZeroVAddr = NonZeroGuestUSize;

/// Pointer type for guest memory, or the "guest pointer" type.
///
/// The `MUT` type parameter determines whether this is mutable or not.
/// Don't write it out explicitly, use [ConstPtr], [MutPtr], [ConstVoidPtr] or
/// [MutVoidPtr] instead instead.
///
/// The implemented methods try to mirror the Rust [pointer] type's methods,
/// where possible.
#[repr(transparent)]
pub struct Ptr<T, const MUT: bool>(VAddr, std::marker::PhantomData<T>);

// #[derive(...)] doesn't work for this type because it expects T to have the
// trait we want implemented
impl<T, const MUT: bool> Clone for Ptr<T, MUT> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, const MUT: bool> Copy for Ptr<T, MUT> {}
impl<T, const MUT: bool> PartialEq for Ptr<T, MUT> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T, const MUT: bool> Eq for Ptr<T, MUT> {}
impl<T, const MUT: bool> std::hash::Hash for Ptr<T, MUT> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Constant guest pointer type (like Rust's `*const T`).
pub type ConstPtr<T> = Ptr<T, false>;

/// Mutable guest pointer type (like Rust's `*mut T`).
pub type MutPtr<T> = Ptr<T, true>;

#[allow(dead_code)]
/// Constant guest pointer-to-void type (like C's `const void *`)
pub type ConstVoidPtr = ConstPtr<std::ffi::c_void>;

/// Mutable guest pointer-to-void type (like C's `void *`)
pub type MutVoidPtr = MutPtr<std::ffi::c_void>;

impl<T, const MUT: bool> Ptr<T, MUT> {
    pub const fn null() -> Self {
        Ptr(0, std::marker::PhantomData)
    }

    pub fn to_bits(self) -> VAddr {
        self.0
    }
    pub const fn from_bits(bits: VAddr) -> Self {
        Ptr(bits, std::marker::PhantomData)
    }

    pub fn cast<U>(self) -> Ptr<U, MUT> {
        Ptr::<U, MUT>::from_bits(self.to_bits())
    }

    pub fn cast_void(self) -> Ptr<std::ffi::c_void, MUT> {
        self.cast()
    }

    pub fn is_null(self) -> bool {
        self.to_bits() == 0
    }

    pub fn non_null(self) -> Option<NonNullPtr<T>> {
        NonNullPtr::try_from_bits(self.0)
    }
}

impl<T> ConstPtr<T> {
    #[allow(dead_code)]
    pub fn cast_mut(self) -> MutPtr<T> {
        Ptr::from_bits(self.to_bits())
    }
}
impl<T> MutPtr<T> {
    pub fn cast_const(self) -> ConstPtr<T> {
        Ptr::from_bits(self.to_bits())
    }
}

impl<T, const MUT: bool> Default for Ptr<T, MUT> {
    fn default() -> Self {
        Self::null()
    }
}

impl<T, const MUT: bool> std::fmt::Debug for Ptr<T, MUT> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_null() {
            write!(f, "(null)")
        } else {
            write!(f, "{:#x}", self.to_bits())
        }
    }
}

// C-like pointer arithmetic
impl<T, const MUT: bool> std::ops::Add<GuestUSize> for Ptr<T, MUT> {
    type Output = Self;

    fn add(self, other: GuestUSize) -> Self {
        let size: GuestUSize = guest_size_of::<T>();
        assert_ne!(size, 0);
        Self::from_bits(
            self.to_bits()
                .checked_add(other.checked_mul(size).unwrap())
                .unwrap(),
        )
    }
}
impl<T, const MUT: bool> std::ops::AddAssign<GuestUSize> for Ptr<T, MUT> {
    fn add_assign(&mut self, rhs: GuestUSize) {
        *self = *self + rhs;
    }
}
impl<T, const MUT: bool> std::ops::Sub<GuestUSize> for Ptr<T, MUT> {
    type Output = Self;

    fn sub(self, other: GuestUSize) -> Self {
        let size: GuestUSize = guest_size_of::<T>();
        assert_ne!(size, 0);
        Self::from_bits(
            self.to_bits()
                .checked_sub(other.checked_mul(size).unwrap())
                .unwrap(),
        )
    }
}
impl<T, const MUT: bool> std::ops::SubAssign<GuestUSize> for Ptr<T, MUT> {
    fn sub_assign(&mut self, rhs: GuestUSize) {
        *self = *self - rhs;
    }
}

/// Non-null pointer type for guest memory, similar to [std::ptr::NonNull].
/// You should use this wrapped in [Option] when storing types instead of
/// storing null pointers.
///
/// You can convert to this type using [Ptr::non_null] (where null pointers
/// will become [None] and other pointers will becone [Some], and convert back
/// using [Self::const_ptr] and [Self::mut_ptr].
#[repr(transparent)]
pub struct NonNullPtr<T>(NonZeroVAddr, std::marker::PhantomData<T>);

#[allow(unused)]
pub type NonNullVoidPtr = NonNullPtr<std::ffi::c_void>;

// #[derive(...)] doesn't work for this type because it expects T to have the
// trait we want implemented
impl<T> Clone for NonNullPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for NonNullPtr<T> {}
impl<T> PartialEq for NonNullPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T> Eq for NonNullPtr<T> {}
impl<T> std::hash::Hash for NonNullPtr<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

#[allow(unused)]
impl<T> NonNullPtr<T> {
    pub fn to_bits(self) -> VAddr {
        self.0.into()
    }
    pub fn try_from_bits(bits: VAddr) -> Option<Self> {
        if bits == 0 {
            None
        } else {
            Some(Self(bits.try_into().unwrap(), std::marker::PhantomData))
        }
    }

    pub fn from_bits(bits: VAddr) -> Self {
        Self::try_from_bits(bits).expect("Tried to create a NonNullPtr with a null value!")
    }

    pub fn cast<U>(self) -> NonNullPtr<U> {
        NonNullPtr::<U>::try_from_bits(self.to_bits()).unwrap()
    }

    pub fn cast_void(self) -> NonNullPtr<std::ffi::c_void> {
        self.cast()
    }

    pub fn mut_ptr(self) -> MutPtr<T> {
        MutPtr::from_bits(self.0.into())
    }

    pub fn const_ptr(self) -> MutPtr<T> {
        MutPtr::from_bits(self.0.into())
    }
}

impl<T> std::fmt::Debug for NonNullPtr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.to_bits())
    }
}

/// Marker trait for types that can be safely read from guest memory.
///
/// See also [SafeWrite] and [crate::abi].
///
/// # Safety
/// Reading from guest memory is essentially doing a [std::mem::transmute],
/// which is notoriously unsafe in Rust.
/// Only types for which all possible bit
/// patterns are legal (e.g. integers) should have this trait.
pub unsafe trait SafeRead: Sized {}
// bool is one byte in size and has 0 as false, 1 as true in both Rust and ObjC
unsafe impl SafeRead for bool {}
unsafe impl SafeRead for i8 {}
unsafe impl SafeRead for u8 {}
unsafe impl SafeRead for i16 {}
unsafe impl SafeRead for u16 {}
unsafe impl SafeRead for i32 {}
unsafe impl SafeRead for u32 {}
unsafe impl SafeRead for i64 {}
unsafe impl SafeRead for u64 {}
unsafe impl SafeRead for f32 {}
unsafe impl SafeRead for f64 {}
unsafe impl<T, const MUT: bool> SafeRead for Ptr<T, MUT> {}

/// Marker trait for types that can be written to guest memory.
///
/// Unlike for [SafeRead], there is no (Rust) safety consideration here;
/// it's
/// just a way to catch accidental use of types unintended for guest use.
/// This was added after discovering that `()` is "[Sized]" and therefore a
/// single stray semicolon can wreak havoc...
///
/// Especially for structs, be careful that the type matches the expected ABI.
/// At minimum you should have `#[repr(C, packed)]` and appropriate padding
/// members.
///
/// See also [SafeRead] and [crate::abi].
pub trait SafeWrite: Sized {}
impl<T: SafeRead> SafeWrite for T {}

type Bytes = [u8; 1 << 32];
pub const PAGE_SIZE: GuestUSize = 4096;
pub const PAGE_SIZE_ALIGN_MASK: GuestUSize = 0xfff;

/// The type that owns the guest memory and provides accessors for it.
pub struct Mem {
    /// This array is 4GiB in size so that it can cover the entire 32-bit
    /// virtual address space, but it should not use that much physical memory,
    /// assuming that the host OS backs it with lazily-allocated pages and we
    /// are careful to avoid accessing most of it.
    ///
    /// iPhone OS devices only had 128MiB or 256MiB of RAM total, with no swap
    /// space, so less than 6.25% of this array should be used, assuming no
    /// fragmentation.
    ///
    /// This is a raw pointer because inevitably we will have to hand out
    /// pointers to memory sometimes, and being able to hold a `&mut` on this
    /// array simultaneously seems like an undefined behavior trap.
    /// This also
    /// means that the underlying memory should never be moved, and therefore
    /// the array can't be growable.
    ///
    /// One advantage of `[u8; 1 << 32]` over `[u8]` is that it might help rustc
    /// optimize away bounds checks for `memory.bytes[ptr_32bit as usize]`.
    ///
    /// Note that unless direct memory access is disabled, the CPU emulation
    /// (dynarmic) accesses memory via this pointer directly except when a page
    /// fault occurs.
    bytes: *mut Bytes,

    /// The size of the __PAGE_ZERO segment, where pointer accesses are trapped
    /// to prevent null pointer derefrences.
    ///
    /// We don't have full memory protection, but we can check accesses in that
    /// range.
    null_segment_size: VAddr,

    allocator: allocator::Allocator,

    /// The flag to control if memory is zeroed out on free (`true`, default)
    /// or on alloc (`false`).
    ///
    /// Right now only one game, Spore Origin, is setting this value to `false`
    /// via a game-specific hack.
    /// See [crate::Environment] for more info.
    pub(super) zero_memory_on_free: bool,

    /// HACK: stub page for null-page READ accesses.
    /// Filled with zeros so that reading *(void**)NULL returns NULL.
    /// This page is NEVER written to by guest code — writes go to
    /// `null_write_sink` instead.
    null_stub_page: *mut u8,

    /// HACK: separate write-sink page for null-page WRITE accesses.
    /// Writes to the null page go here and are silently discarded.
    /// This prevents write operations from corrupting the zero-filled
    /// read stub page.
    null_write_sink: *mut u8,
}

impl Drop for Mem {
    fn drop(&mut self) {
        unsafe {
            crate::mem::host::free_guest_memory(self.bytes.cast(), std::mem::size_of::<Bytes>())
                .unwrap();
            // Free the read stub page
            if !self.null_stub_page.is_null() {
                crate::mem::host::free_memory(self.null_stub_page.cast(), PAGE_SIZE as usize)
                    .unwrap();
            }
            // Free the write sink page
            if !self.null_write_sink.is_null() {
                crate::mem::host::free_memory(self.null_write_sink.cast(), PAGE_SIZE as usize)
                    .unwrap();
            }
        }
    }
}

impl Mem {
    /// [According to Apple](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Multithreading/CreatingThreads/CreatingThreads.html)
    /// among others, the iPhone OS main thread stack size is 1MiB.
    pub const MAIN_THREAD_STACK_SIZE: GuestUSize = 1024 * 1024;

    /// Address of the lowest byte (not the base) of the main thread's stack.
    ///
    /// We are arbitrarily putting the stack at the top of the virtual address
    /// space (see also: stack.rs), I have no idea if this matches iPhone OS.
    pub const MAIN_THREAD_STACK_LOW_END: VAddr = 0u32.wrapping_sub(Self::MAIN_THREAD_STACK_SIZE);

    /// iPhone OS secondary thread stack size.
    pub const SECONDARY_THREAD_DEFAULT_STACK_SIZE: GuestUSize = 512 * 1024;

    /// Create a fresh instance of guest memory.
    pub fn new() -> Mem {
        let size = std::mem::size_of::<Bytes>();
        let ptr = unsafe { crate::mem::host::allocate_guest_memory(size).unwrap() };

        assert_eq!(
            ptr as usize & PAGE_SIZE_ALIGN_MASK as usize,
            0,
            "Failed to align host memory with guest memory"
        );
        let bytes = ptr as *mut Bytes;

        // Allocate read stub page for null-page reads (4KB, zero-filled).
        // Data reads of a NULL pointer (e.g. `*(void**)0`) return NULL.
        let null_stub_page = unsafe {
            let page = crate::mem::host::allocate_memory(PAGE_SIZE as usize).unwrap();
            let stub_slice = std::slice::from_raw_parts_mut(page as *mut u8, PAGE_SIZE as usize);
            stub_slice.fill(0);
            page as *mut u8
        };

        // Allocate a separate write-sink page for null-page writes (4KB).
        // Writes to the null page are absorbed here so that they don't
        // corrupt the read stub page's zeros.
        let null_write_sink = unsafe {
            let page = crate::mem::host::allocate_memory(PAGE_SIZE as usize).unwrap();
            let sink_slice = std::slice::from_raw_parts_mut(page as *mut u8, PAGE_SIZE as usize);
            sink_slice.fill(0);
            page as *mut u8
        };

        let allocator = allocator::Allocator::new();
        Mem {
            bytes,
            null_segment_size: 0,
            allocator,
            zero_memory_on_free: true,
            null_stub_page,
            null_write_sink,
        }
    }

    /// Sets up the null segment of the given size.
    /// There's no reason to call
    /// this outside of binary loading, and it won't be respected even if you
    /// do.
    /// The size must not have been set already, and must be page aligned.
    pub fn set_null_segment_size(&mut self, new_null_segment_size: VAddr) {
        // TODO?: Maybe this should be replaced with a per-page rwx/callback
        //        setting?
        //        Currently we don't properly follow segment
        //        protections, which means that applications can write into
        //        segments they shouldn't be able to.
        //        Adding that would fix
        //        this, along with removing this special case.
        assert!(self.null_segment_size == 0);
        assert!(new_null_segment_size.is_multiple_of(0x1000));
        self.allocator
            .reserve(allocator::Chunk::new(0, new_null_segment_size));
        self.null_segment_size = new_null_segment_size;
    }

    pub fn null_segment_size(&self) -> VAddr {
        self.null_segment_size
    }

    /// Get a pointer to the full 4GiB of memory.
    /// This is only for use when
    /// setting up the CPU, never call this otherwise.
    ///
    /// Safety: You must ensure that this pointer does not outlive the instance
    /// of [Mem].
    /// You must not use it while a `&mut` is held on some region of
    /// guest memory.
    pub unsafe fn direct_memory_access_ptr(&mut self) -> *mut std::ffi::c_void {
        self.bytes.cast()
    }

    fn bytes(&self) -> &Bytes {
        unsafe { &*self.bytes }
    }
    fn bytes_mut(&mut self) -> &mut Bytes {
        unsafe { &mut *self.bytes }
    }

    // Soft handler for null-page accesses. No panic; returns a stub page.
    // Rate-limited: only the first N unique (addr, is_write) pairs are logged,
    // further occurrences are silently counted. This prevents the log from
    // being flooded when the game repeatedly probes null-page addresses.
    #[cold]
    fn null_check_fail(at: VAddr, size: GuestUSize, is_write: bool, caller: &str) {
        use std::collections::HashSet;
        use std::sync::Mutex;
        static SEEN: Mutex<Option<HashSet<(VAddr, bool)>>> = Mutex::new(None);
        const MAX_UNIQUE_LOGS: usize = 64;

        let mut guard = SEEN.lock().unwrap();
        let set = guard.get_or_insert_with(HashSet::new);
        let key = (at, is_write);
        if set.contains(&key) {
            return;
        }
        if set.len() >= MAX_UNIQUE_LOGS {
            if set.len() == MAX_UNIQUE_LOGS {
                // Insert a sentinel to emit the notice only once.
                set.insert((0xFFFF_FFFE, false));
                log!(
                    "touchHLE::mem: further NULL-PAGE warnings silenced after {} unique sites",
                    MAX_UNIQUE_LOGS
                );
            }
            return;
        }
        set.insert(key);
        let op_type = if is_write { "WRITE" } else { "READ" };
        // Provide helpful context: small offsets are typically field accesses
        // on a nil Objective-C object pointer (nil + ivar offset). This is
        // defined behavior in ObjC (returns 0/nil) and is NOT a crash — just
        // a sign that the app is accessing a nil object's fields.
        let context = if !is_write && at < 0x1000 {
            " (likely nil ObjC object field access — returning zero)"
        } else if is_write && at < 0x1000 {
            " (likely nil ObjC object field write — discarding)"
        } else {
            " — returning stub page"
        };
        log!(
            "touchHLE::mem: NULL-PAGE {} at 0x{:08x} (size: 0x{:x}) from {}{} \
             (unique sites logged: {}/{})",
            op_type,
            at,
            size,
            caller,
            context,
            set.len(),
            MAX_UNIQUE_LOGS
        );
    }

    /// Special version of [Self::bytes_at] that returns [None] rather than
    /// panicking on failure.
    /// Only for use by [crate::gdb::GdbServer].
    pub fn get_bytes_fallible(&self, addr: ConstVoidPtr, count: GuestUSize) -> Option<&[u8]> {
        if addr.to_bits() < self.null_segment_size {
            // Для GDB возвращаем stub-страницу
            let offset = (addr.to_bits() % PAGE_SIZE) as usize;
            let count_usize = count as usize;
            let stub_slice = unsafe {
                std::slice::from_raw_parts(
                    self.null_stub_page.add(offset),
                    PAGE_SIZE as usize - offset,
                )
            };
            return Some(&stub_slice[..count_usize.min(stub_slice.len())]);
        }
        self.bytes()
            .get(addr.to_bits() as usize..)?
            .get(..count as usize)
    }
    /// Special version of [Self::bytes_at_mut] that returns [None] rather than
    /// panicking on failure.
    /// Only for use by [crate::gdb::GdbServer].
    pub fn get_bytes_fallible_mut(
        &mut self,
        addr: ConstVoidPtr,
        count: GuestUSize,
    ) -> Option<&mut [u8]> {
        if addr.to_bits() < self.null_segment_size {
            return None;
            // GDB не должен писать в null-page
        }
        self.bytes_mut()
            .get_mut(addr.to_bits() as usize..)?
            .get_mut(..count as usize)
    }

    /// Get a slice for reading `count` bytes.
    /// This is the basic primitive for
    /// safe read-only memory access.
    ///
    /// This will panic when `ptr` is within the null page, even if `count` is
    /// 0. This may be inconvenient in some cases, but it makes the behavior
    /// when deriving a pointer from the slice consistent (though you should use
    /// [Self::ptr_at] for that).
    pub fn bytes_at<const MUT: bool>(&self, ptr: Ptr<u8, MUT>, count: GuestUSize) -> &[u8] {
        // ХАК: Вместо паники логируем и возвращаем данные из stub-страницы
        if ptr.to_bits() < self.null_segment_size {
            Self::null_check_fail(ptr.to_bits(), count, false, "bytes_at");
            // Возвращаем данные из stub-страницы вместо реальной памяти
            // Это предотвращает UndefinedInstruction когда игра использует
            // прочитанные значения как указатели на функции
            let offset = (ptr.to_bits() % PAGE_SIZE) as usize;
            let count_usize = count as usize;
            let available = PAGE_SIZE as usize - offset;
            let actual_count = count_usize.min(available);
            return unsafe {
                std::slice::from_raw_parts(self.null_stub_page.add(offset), actual_count)
            };
        }
        // Guard against out-of-bounds reads near the top of the 32-bit address
        // space. If `ptr + count` wraps around or exceeds the backing array,
        // return the stub page. This prevents panics when a game uses -1 or
        // another near-max address as a pointer (corrupted pointer arithmetic).
        let addr = ptr.to_bits() as usize;
        let end = addr.saturating_add(count as usize);
        if end > self.bytes().len() || end < addr {
            Self::null_check_fail(ptr.to_bits(), count, false, "bytes_at(OOB)");
            let offset = (ptr.to_bits() % PAGE_SIZE) as usize;
            let count_usize = count as usize;
            let available = PAGE_SIZE as usize - offset;
            let actual_count = count_usize.min(available);
            return unsafe {
                std::slice::from_raw_parts(self.null_stub_page.add(offset), actual_count)
            };
        }
        &self.bytes()[addr..][..count as usize]
    }
    /// Get a slice for reading `count` bytes without a null-page check.
    ///
    /// This **doesn't** panic at access within the null page.
    ///
    /// You shall have a good reason to use it instead of [Self::bytes_at]
    pub fn unchecked_bytes_at<const MUT: bool>(
        &self,
        ptr: Ptr<u8, MUT>,
        count: GuestUSize,
    ) -> &[u8] {
        let addr = ptr.to_bits() as usize;
        let end = addr.saturating_add(count as usize);
        if end > self.bytes().len() || end < addr {
            Self::null_check_fail(ptr.to_bits(), count, false, "unchecked_bytes_at(OOB)");
            let offset = (ptr.to_bits() % PAGE_SIZE) as usize;
            let count_usize = count as usize;
            let available = PAGE_SIZE as usize - offset;
            let actual_count = count_usize.min(available);
            return unsafe {
                std::slice::from_raw_parts(self.null_stub_page.add(offset), actual_count)
            };
        }
        &self.bytes()[addr..][..count as usize]
    }
    /// Get a slice for reading or writing `count` bytes.
    /// This is the basic
    /// primitive for safe read-write memory access.
    ///
    /// This will panic when `ptr` is within the null page, even if `count` is
    /// 0. This may be inconvenient in some cases, but it makes the behavior
    /// when deriving a pointer from the slice consistent (though you should use
    /// [Self::ptr_at_mut] for that).
    pub fn bytes_at_mut(&mut self, ptr: MutPtr<u8>, count: GuestUSize) -> &mut [u8] {
        // ХАК: Вместо паники логируем и возвращаем данные из stub-страницы
        if ptr.to_bits() < self.null_segment_size {
            Self::null_check_fail(ptr.to_bits(), count, true, "bytes_at_mut");
            // For writes to null-page, return the write-sink page so that
            // writes are silently absorbed without corrupting the read stub
            // page's zeros.
            let offset = (ptr.to_bits() % PAGE_SIZE) as usize;
            let count_usize = count as usize;
            let available = PAGE_SIZE as usize - offset;
            let actual_count = count_usize.min(available);
            return unsafe {
                std::slice::from_raw_parts_mut(self.null_write_sink.add(offset), actual_count)
            };
        }
        // Guard against out-of-bounds writes near the top of the 32-bit
        // address space (e.g. corrupted pointer = 0xFFFFFFFF).
        let addr = ptr.to_bits() as usize;
        let end = addr.saturating_add(count as usize);
        if end > self.bytes().len() || end < addr {
            Self::null_check_fail(ptr.to_bits(), count, true, "bytes_at_mut(OOB)");
            let offset = (ptr.to_bits() % PAGE_SIZE) as usize;
            let count_usize = count as usize;
            let available = PAGE_SIZE as usize - offset;
            let actual_count = count_usize.min(available);
            return unsafe {
                std::slice::from_raw_parts_mut(self.null_write_sink.add(offset), actual_count)
            };
        }
        &mut self.bytes_mut()[addr..][..count as usize]
    }

    /// Get a pointer for reading an array of `count` elements of type `T`.
    /// Only use this for interfacing with unsafe C-like APIs.
    ///
    /// The `count` argument is purely for bounds-checking and does not affect
    /// the result.
    ///
    /// No guarantee is made about the alignment of the resulting pointer!
    /// Pointers that are well-aligned for the guest are not necessarily
    /// well-aligned for the host.
    /// Rust strictly requires pointers to be
    /// well-aligned when dereferencing them, or when constructing references or
    /// slices from them, so **be very careful**.
    pub fn ptr_at<T, const MUT: bool>(&self, ptr: Ptr<T, MUT>, count: GuestUSize) -> *const T
    where
        T: SafeRead,
    {
        let size = count.checked_mul(guest_size_of::<T>()).unwrap();
        self.bytes_at(ptr.cast(), size).as_ptr().cast()
    }
    /// A variation of [Self::ptr_at] without a null-page check.
    ///
    /// This **doesn't** panic at access within the null page.
    ///
    /// You shall have a good reason to use it instead of [Self::ptr_at]
    pub fn unchecked_ptr_at<T, const MUT: bool>(
        &self,
        ptr: Ptr<T, MUT>,
        count: GuestUSize,
    ) -> *const T
    where
        T: SafeRead,
    {
        let size = count.checked_mul(guest_size_of::<T>()).unwrap();
        self.unchecked_bytes_at(ptr.cast(), size).as_ptr().cast()
    }
    /// Get a pointer for reading or writing to an array of `count` elements of
    /// type `T`.
    /// Only use this for interfacing with unsafe C-like APIs.
    ///
    /// The `count` argument is purely for bounds-checking and does not affect
    /// the result.
    ///
    /// No guarantee is made about the alignment of the resulting pointer!
    /// Pointers that are well-aligned for the guest are not necessarily
    /// well-aligned for the host.
    /// Rust strictly requires pointers to be
    /// well-aligned when dereferencing them, or when constructing references or
    /// slices from them, so **be very careful**.
    pub fn ptr_at_mut<T>(&mut self, ptr: MutPtr<T>, count: GuestUSize) -> *mut T
    where
        T: SafeRead + SafeWrite,
    {
        let size = count.checked_mul(guest_size_of::<T>()).unwrap();
        self.bytes_at_mut(ptr.cast(), size).as_mut_ptr().cast()
    }

    /// Transform a host pointer addressing a location in guest memory back into
    /// a guest pointer.
    /// This exists solely to deal with OpenGL `glGetPointerv`.
    /// You should never have another reason to use this.
    ///
    /// Panics if the host pointer is not addressing a location in guest memory.
    pub fn host_ptr_to_guest_ptr(&self, host_ptr: *const std::ffi::c_void) -> ConstVoidPtr {
        let host_ptr = host_ptr.cast::<u8>();
        let guest_mem_range = self.bytes().as_ptr_range();
        assert!(guest_mem_range.contains(&host_ptr));
        let guest_addr = host_ptr as usize - guest_mem_range.start as usize;
        Ptr::from_bits(u32::try_from(guest_addr).unwrap())
    }

    /// Returns whether a host pointer addresses a location inside the guest's
    /// memory region. Used to sanity-check pointers that touchHLE hands to host
    /// APIs (e.g. client-side OpenGL vertex arrays): a pointer outside this
    /// range is wild and dereferencing it on the host would crash the emulator.
    pub fn is_host_ptr_in_guest_mem(&self, host_ptr: *const std::ffi::c_void) -> bool {
        let host_ptr = host_ptr.cast::<u8>();
        self.bytes().as_ptr_range().contains(&host_ptr)
    }

    /// Read a value for memory.
    /// This is the preferred way to read memory in
    /// most cases.
    pub fn read<T, const MUT: bool>(&self, ptr: Ptr<T, MUT>) -> T
    where
        T: SafeRead,
    {
        // This is unsafe unless we are careful with which types SafeRead is
        // implemented for!
        // This would also be unsafe if the non-unaligned method was used.
        unsafe { self.ptr_at(ptr, 1).read_unaligned() }
    }
    /// Write a value to memory.
    /// This is the preferred way to write memory in
    /// most cases.
    pub fn write<T>(&mut self, ptr: MutPtr<T>, value: T)
    where
        T: SafeWrite,
    {
        let size = guest_size_of::<T>();
        assert!(size > 0);
        let slice = self.bytes_at_mut(ptr.cast(), size);
        let ptr: *mut T = slice.as_mut_ptr().cast();
        // It's unaligned because what is well-aligned for the guest is not
        // necessarily well-aligned for the host.
        // This would be unsafe if the non-unaligned method was used.
        unsafe { ptr.write_unaligned(value) }
    }

    /// C-style `memmove`.
    ///
    /// Sanity-checks the arguments. If `src + size` or `dest + size` would
    /// run off the end of the 4 GiB guest address space, the operation is
    /// logged and skipped instead of panicking. This is a defensive measure
    /// for guest code that calls `memmove`/`memcpy` with corrupted arguments
    /// (for example, an uninitialised `std::string` whose internal length
    /// happens to be wildly out of range): a guest bug shouldn't take down
    /// the whole emulator.
    pub fn memmove(&mut self, dest: MutVoidPtr, src: ConstVoidPtr, size: GuestUSize) {
        let src_addr = src.to_bits() as usize;
        let dest_addr = dest.to_bits() as usize;
        let size_us = size as usize;
        let max = self.bytes_mut().len();

        // Early reject: if size looks like a negative i32 cast to u32
        // (>= 0x8000_0000), it's almost certainly corrupted. Guest code on
        // 32-bit ARM that passes (size_t)(-1) or similar huge values is
        // buggy — skip the operation to keep the emulator alive.
        if size >= 0x8000_0000 {
            log!(
                "WARNING: memmove with likely-negative size ({:#x} = {}); \
                 src={:#x}, dest={:#x} — skipping",
                size,
                size as i32,
                src_addr,
                dest_addr,
            );
            return;
        }

        // FIXED: When src is NULL, zero-fill destination instead of skipping
        // This matches real iOS behavior for corrupted std::string internals
        if src_addr == 0 && size > 0 {
            if dest_addr != 0 && dest_addr < max {
                let actual_size = size_us.min(max - dest_addr);
                self.bytes_mut()[dest_addr..dest_addr + actual_size].fill(0);
                log_dbg!(
                    "memmove from NULL — zero-filled dest={:#x}, size={:#x}",
                    dest_addr, actual_size
                );
            } else {
                log!(
                    "WARNING: memmove from NULL with invalid dest={:#x}, size={:#x} — skipping",
                    dest_addr, size_us,
                );
            }
            return;
        }

        let src_end = match src_addr.checked_add(size_us) {
            Some(v) if v <= max => v,
            _ => {
                log!(
                    "WARNING: memmove with bogus args (src={:#x}, dest={:#x}, \
                     size={:#x}) — skipping to avoid host crash",
                    src_addr,
                    dest_addr,
                    size_us
                );
                return;
            }
        };
        let dest_end = match dest_addr.checked_add(size_us) {
            Some(v) if v <= max => v,
            _ => {
                log!(
                    "WARNING: memmove with bogus args (src={:#x}, dest={:#x}, \
                     size={:#x}) — skipping to avoid host crash",
                    src_addr,
                    dest_addr,
                    size_us
                );
                return;
            }
        };
        let _ = (src_end, dest_end);

        self.bytes_mut()
            .copy_within(src_addr..src_addr + size_us, dest_addr)
    }

    /// Allocate `size` bytes.
    pub fn alloc(&mut self, size: GuestUSize) -> MutVoidPtr {
        let ptr = Ptr::from_bits(self.allocator.alloc(size));
        if !self.zero_memory_on_free {
            self.bytes_at_mut(ptr.cast(), size).fill(0);
        }

        log_dbg!("Allocated {:?} ({:#x} bytes)", ptr, size);
        ptr
    }

    /// Allocate `size` bytes initialized to 0.
    pub fn calloc(&mut self, size: GuestUSize) -> MutVoidPtr {
        let ptr = self.alloc(size);
        self.bytes_at_mut(ptr.cast(), size).fill(0);
        ptr
    }

    /// Implements Apple's documented `malloc_size(3)` contract: returns the
    /// size of the memory block that backs the allocation pointed to by
    /// `ptr`, or `0` if `ptr` is `NULL` or doesn't belong to any block
    /// allocated through malloc. This is deliberately a *silent* lookup —
    /// it's perfectly normal for apps to call `malloc_size` on arbitrary
    /// pointers (interior pointers, `__DATA` symbols, stack addresses,
    /// etc.) and treat a `0` result as "this isn't a heap allocation",
    /// so we must not flood the log when it happens. See
    /// <https://developer.apple.com/library/archive/documentation/Performance/Conceptual/ManagingMemory/Articles/MallocDebug.html>.
    pub fn malloc_size(&self, ptr: ConstVoidPtr) -> GuestUSize {
        if ptr.is_null() {
            return 0;
        }
        self.allocator
            .try_find_allocated_size(ptr.to_bits())
            .unwrap_or(0)
    }

    /// Returns whether `addr` is the exact base of a live allocation. Used to
    /// defensively reject garbage pointers at the libc free() wrapper.
    pub fn is_known_allocation(&self, addr: VAddr) -> bool {
        self.allocator.is_known_allocation(addr)
    }

    pub fn realloc(&mut self, old_ptr: MutVoidPtr, size: GuestUSize) -> MutVoidPtr {
        if old_ptr.is_null() {
            return self.alloc(size);
        }

        // TODO: for a moment we always assume that we do not have enough size
        //       to realloc inplace
        let old_size = self.allocator.find_allocated_size(old_ptr.to_bits());
        if old_size >= size {
            return old_ptr;
        }

        let new_ptr = self.alloc(size);
        self.memmove(new_ptr, old_ptr.cast_const(), old_size);
        self.free(old_ptr);
        new_ptr
    }

    /// Free an allocation made with one of the `alloc` methods on this type.
    pub fn free(&mut self, ptr: MutVoidPtr) {
        if ptr.is_null() {
            return;
        }
        let addr = ptr.to_bits();
        // Silently ignore attempts to free the MACH_TASK_SELF constant
        // (0x7461736b = "task"). The Mono runtime stores this value and
        // attempts to free it during shutdown; it's not a real allocation.
        if addr == 0x7461736b {
            return;
        }
        // Reject obviously bogus pointers before passing to the allocator.
        if !self.allocator.is_known_allocation(addr) {
            log!("Can't free {:#x}, unknown allocation!", addr);
            return;
        }
        let size = self.allocator.free(addr);
        if self.zero_memory_on_free {
            self.bytes_at_mut(ptr.cast(), size).fill(0);
        }

        log_dbg!("Freed {:?} ({:#x} bytes)", ptr, size);
    }

    /// Allocate memory large enough for a value of type `T` and write the value
    /// to it.
    /// Equivalent to [Self::alloc] + [Self::write].
    pub fn alloc_and_write<T>(&mut self, value: T) -> MutPtr<T>
    where
        T: SafeWrite,
    {
        let ptr = self.alloc(guest_size_of::<T>()).cast();
        self.write(ptr, value);
        ptr
    }

    /// Allocate and write a C string.
    /// This method will add a null terminator,
    /// so it is optimal if the input slice does not already contain one.
    pub fn alloc_and_write_cstr(&mut self, str_bytes: &[u8]) -> MutPtr<u8> {
        let len = str_bytes.len().try_into().unwrap();
        let ptr = self.alloc(len + 1).cast();
        self.bytes_at_mut(ptr, len).copy_from_slice(str_bytes);
        self.write(ptr + len, b'\0');
        ptr
    }

    /// Get a C string (null-terminated) as a slice.
    /// The null terminator is not
    /// included in the slice.
    ///
    /// Safety: includes a maximum length guard (64KB) to prevent infinite loops
    /// if the guest provides a pointer to non-terminated data.
    pub fn cstr_at<const MUT: bool>(&self, ptr: Ptr<u8, MUT>) -> &[u8] {
        const MAX_CSTR_LEN: u32 = 65536; // 64KB safety limit
        let mut len: u32 = 0;
        while self.read(ptr + len) != b'\0' {
            len += 1;
            if len >= MAX_CSTR_LEN {
                log!(
                    "Warning: cstr_at({:?}): hit {}B safety limit without finding null terminator; truncating.",
                    ptr, MAX_CSTR_LEN
                );
                break;
            }
        }
        self.bytes_at(ptr, len)
    }

    /// Get a C string (null-terminated) as a string slice, if it is valid
    /// UTF-8, otherwise returning a byte slice.
    /// The null terminator is not
    /// included in the slice.
    pub fn cstr_at_utf8<const MUT: bool>(&self, ptr: Ptr<u8, MUT>) -> Result<&str, &[u8]> {
        let bytes = self.cstr_at(ptr);
        std::str::from_utf8(bytes).map_err(|_| bytes)
    }

    pub fn wcstr_at<const MUT: bool>(&self, ptr: Ptr<wchar_t, MUT>) -> String {
        const MAX_WCSTR_LEN: u32 = 16384; // 16K chars safety limit
        let mut len: u32 = 0;
        while self.read(ptr + len) != wchar_t::default() {
            len += 1;
            if len >= MAX_WCSTR_LEN {
                log!(
                    "Warning: wcstr_at({:?}): hit {} char safety limit without finding null terminator; truncating.",
                    ptr, MAX_WCSTR_LEN
                );
                break;
            }
        }

        // iOS/macOS uses 4-byte wchar_t (UTF-32LE). char::from_u32 returns
        // None for surrogate values (U+D800..U+DFFF) and codepoints above
        // U+10FFFF; in those cases we substitute U+FFFD REPLACEMENT CHARACTER
        // instead of panicking so that bogus data from the guest does not
        // crash the host.
        let bytes = self.bytes_at(ptr.cast(), len * guest_size_of::<wchar_t>());
        let iter = bytes.chunks_exact(4).map(|chunk| {
            // chunks_exact(4) guarantees the length, so try_into never fails.
            let code = u32::from_le_bytes(chunk.try_into().unwrap());
            char::from_u32(code).unwrap_or('\u{FFFD}')
        });
        String::from_iter(iter)
    }

    /// Permanently mark a region of address space as being unusable to the
    /// memory allocator.
    ///
    /// A zero-byte reservation is a documented no-op: it matches what xnu's
    /// `mach_loader.c` does when handed a `LC_SEGMENT` whose `vmsize == 0`
    /// (the kernel reserves no address space, the segment is silently
    /// ignored). We mirror that here so the allocator's `Chunk` invariant —
    /// every chunk must contain at least one byte — is preserved even when
    /// callers (Mach-O loader, `dyld::do_initial_linking`, etc.) hand us a
    /// degenerate request.
    pub fn reserve(&mut self, base: VAddr, size: GuestUSize) {
        if size == 0 {
            log_dbg!(
                "Mem::reserve({:#x}, 0) — no-op (matches xnu mach_loader.c)",
                base
            );
            return;
        }
        self.allocator.reserve(allocator::Chunk::new(base, size));
    }
}

#[cfg(test)]
mod mem_tests {
    use super::{Mem, MutPtr, Ptr};

    #[test]
    fn lazy_commit_far_addresses() {
        let mut mem = Mem::new();

        mem.set_null_segment_size(super::PAGE_SIZE);

        let probes: [u32; 6] = [
            0x0000_1000,
            0x1000_0000,
            0x4000_0000,
            0x8000_0000,
            0xC000_0000,
            0xFFFE_F000,
        ];
        for &addr in &probes {
            let p: MutPtr<u8> = Ptr::from_bits(addr);
            mem.write(p, 0xAB);
            assert_eq!(mem.read(p.cast_const()), 0xAB);
        }
    }
}
