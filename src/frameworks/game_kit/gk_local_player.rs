/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `GKLocalPlayer`.

use crate::abi::{CallFromHost, GuestFunction};
use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::foundation::ns_string;
use crate::mem::{ConstVoidPtr, MutPtr, Ptr};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, ClassExports, HostObject,
    NSZonePtr,
};
use crate::Environment;

/// Apple GameKit `GKError.h`:
/// `GKErrorNotAuthenticated = 6`. Returned by GameKit APIs when the
/// local player is not signed in to Game Center. We use this code in
/// the NSError we hand back to authentication completion handlers,
/// because touchHLE has no Game Center connectivity and so the local
/// player can never be authenticated.
const GK_ERROR_NOT_AUTHENTICATED: i32 = 6;

/// Apple Block ABI: word offset 3 (== byte offset 12) of a block
/// struct holds its `invoke` function pointer.
/// <https://clang.llvm.org/docs/Block-ABI-Apple.html>
const BLOCK_INVOKE_WORD_OFFSET: u32 = 3;

/// Build an autoreleased `NSError*` describing the "not signed in to
/// Game Center" condition. Domain = `GKErrorDomain`,
/// code = `GKErrorNotAuthenticated`, with a localized description so
/// games that surface the error to the user get a sensible message.
fn make_not_authenticated_error(env: &mut Environment) -> id {
    use crate::frameworks::foundation::ns_string::{from_rust_string, get_static_str};
    let domain = from_rust_string(env, "GKErrorDomain".to_string());
    autorelease(env, domain);

    let desc_key = get_static_str(env, "NSLocalizedDescription");
    let desc_val = from_rust_string(
        env,
        "The requested operation could not be completed because local \
         player has not been authenticated. (touchHLE: Game Center is \
         offline)"
            .to_string(),
    );
    autorelease(env, desc_val);

    let user_info: id = msg_class![env; NSMutableDictionary new];
    autorelease(env, user_info);
    () = msg![env; user_info setObject:desc_val forKey:desc_key];

    let error: id = msg_class![env; NSError alloc];
    let error: id = msg![env;
        error initWithDomain:domain
                        code:(GK_ERROR_NOT_AUTHENTICATED)
                    userInfo:user_info];
    autorelease(env, error);
    error
}

/// Invoke an ObjC block whose underlying C function has the signature
/// `void (^)(NSError *)`. Returns silently if `block` is nil or its
/// invoke pointer is zero (the latter happens when the guest hands us
/// a stack-allocated literal block that was never `Block_copy`-ed and
/// has already gone out of scope). Apple's `Block_ABI`:
/// <https://clang.llvm.org/docs/Block-ABI-Apple.html>.
fn invoke_error_block(env: &mut Environment, block: id, error: id) {
    if block == nil {
        return;
    }
    let block_ptr: MutPtr<u32> = Ptr::from_bits(block.to_bits());
    let invoke_addr: u32 = env.mem.read(block_ptr + BLOCK_INVOKE_WORD_OFFSET);
    if invoke_addr == 0 {
        log!(
            "Warning: GKLocalPlayer completion block {:?} has NULL invoke \
             pointer; not calling.",
            block
        );
        return;
    }
    let invoke = GuestFunction::from_addr_with_thumb_bit(invoke_addr);
    let block_arg: ConstVoidPtr = Ptr::from_bits(block.to_bits()).cast_const();
    <GuestFunction as CallFromHost<(), (ConstVoidPtr, id)>>::call_from_host(
        &invoke,
        env,
        (block_arg, error),
    );
}

/// Invoke an ObjC block whose underlying C function has the signature
/// `void (^)(UIViewController *viewController, NSError *error)`. Used
/// by `-[GKLocalPlayer setAuthenticateHandler:]` introduced in iOS 6.
fn invoke_vc_error_block(env: &mut Environment, block: id, vc: id, error: id) {
    if block == nil {
        return;
    }
    let block_ptr: MutPtr<u32> = Ptr::from_bits(block.to_bits());
    let invoke_addr: u32 = env.mem.read(block_ptr + BLOCK_INVOKE_WORD_OFFSET);
    if invoke_addr == 0 {
        log!(
            "Warning: GKLocalPlayer authenticate handler {:?} has NULL \
             invoke pointer; not calling.",
            block
        );
        return;
    }
    let invoke = GuestFunction::from_addr_with_thumb_bit(invoke_addr);
    let block_arg: ConstVoidPtr = Ptr::from_bits(block.to_bits()).cast_const();
    <GuestFunction as CallFromHost<(), (ConstVoidPtr, id, id)>>::call_from_host(
        &invoke,
        env,
        (block_arg, vc, error),
    );
}

// MARK: - Per-process state

/// Singleton cache for `[GKLocalPlayer localPlayer]`.
#[derive(Default)]
pub struct State {
    local_player: Option<id>,
}

impl State {
    fn get(env: &mut Environment) -> &mut State {
        &mut env.framework_state.game_kit.local_player
    }
}

// MARK: - Host object

#[derive(Default)]
struct GKLocalPlayerHostObject {
    /// `NSString*`
    player_id: id,
    /// `NSString*`
    alias: id,
    /// `NSString*`
    display_name: id,
    authenticated: bool,
    underage: bool,
    /// `NSArray*` of `NSString*` friend player IDs
    friends: id,
}
impl HostObject for GKLocalPlayerHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation GKLocalPlayer: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(GKLocalPlayerHostObject {
        player_id: nil,
        alias: nil,
        display_name: nil,
        authenticated: false,
        underage: false,
        friends: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// MARK: - Singleton

+ (id)localPlayer {
    // Real GameKit always returns the same retained singleton.
    // Cache it in State so it survives autorelease pool drains.
    if let Some(player) = State::get(env).local_player {
        return player;
    }

    // alloc gives refcount=1 which is our singleton retain — do NOT
    // autorelease here.
    let player: id = msg![env; this alloc];
    let player: id = msg![env; player init];

    let player_id = ns_string::from_rust_string(
        env,
        "GKLocalPlayer:touchHLE".to_string(),
    );
    let alias   = ns_string::from_rust_string(env, "Player".to_string());
    let display = ns_string::from_rust_string(env, "Player".to_string());
    let friends = msg_class![env; NSArray new];

    {
        let host = env.objc.borrow_mut::<GKLocalPlayerHostObject>(player);
        host.player_id    = player_id;
        host.alias        = alias;
        host.display_name = display;
        host.friends      = friends;
    }

    State::get(env).local_player = Some(player);
    log!("GKLocalPlayer localPlayer: singleton created");
    player
}

// MARK: - Score / achievement convenience (class-level)

// Apple reference (iOS 7+):
// <https://developer.apple.com/documentation/gamekit/gklocalplayer/1521031-setdefaultleaderboardidentifier>
// "The completion handler is called with a nil error if the
//  identifier was set, or an NSError if the request failed."
//
// touchHLE has no Game Center connectivity, so the request always
// fails with `GKErrorNotAuthenticated`. We still invoke the
// completion handler so the caller can proceed.
+ (())setDefaultLeaderboardIdentifier:(id)_identifier
               withCompletionHandler:(id)handler {
    let error = make_not_authenticated_error(env);
    invoke_error_block(env, handler, error);
}

// Apple reference (iOS 7+):
// <https://developer.apple.com/documentation/gamekit/gklocalplayer/1521090-loaddefaultleaderboardidentifier>
// "If the default leaderboard identifier was loaded successfully,
//  this block receives a string. […] Otherwise the error parameter
//  contains an NSError describing the failure."
//
// The completion block signature is
//   void (^)(NSString *leaderboardIdentifier, NSError *error)
// which is the same ABI as `void (^)(id, id)`.
+ (())loadDefaultLeaderboardIdentifierWithCompletionHandler:(id)handler {
    if handler == nil {
        return;
    }
    let block_ptr: MutPtr<u32> = Ptr::from_bits(handler.to_bits());
    let invoke_addr: u32 = env.mem.read(block_ptr + BLOCK_INVOKE_WORD_OFFSET);
    if invoke_addr == 0 {
        return;
    }
    let invoke = GuestFunction::from_addr_with_thumb_bit(invoke_addr);
    let block_arg: ConstVoidPtr = Ptr::from_bits(handler.to_bits()).cast_const();
    let error = make_not_authenticated_error(env);
    <GuestFunction as CallFromHost<(), (ConstVoidPtr, id, id)>>::call_from_host(
        &invoke, env, (block_arg, nil, error),
    );
}

// MARK: - Init / dealloc

- (id)init {
    this
}

- (())dealloc {
    let host = env.objc.borrow::<GKLocalPlayerHostObject>(this);
    let (player_id, alias, display_name, friends) =
        (host.player_id, host.alias, host.display_name, host.friends);
    release(env, player_id);
    release(env, alias);
    release(env, display_name);
    release(env, friends);
    env.objc.dealloc_object(this, &mut env.mem)
}

// MARK: - Identity

- (id)playerID {
    env.objc.borrow::<GKLocalPlayerHostObject>(this).player_id
}

- (id)alias {
    env.objc.borrow::<GKLocalPlayerHostObject>(this).alias
}

- (id)displayName {
    env.objc.borrow::<GKLocalPlayerHostObject>(this).display_name
}

// MARK: - Authentication state

- (bool)isAuthenticated {
    env.objc.borrow::<GKLocalPlayerHostObject>(this).authenticated
}

- (bool)isUnderage {
    env.objc.borrow::<GKLocalPlayerHostObject>(this).underage
}

// Apple reference (iOS 4.1, deprecated iOS 6):
// <https://developer.apple.com/documentation/gamekit/gklocalplayer/1521099-authenticatewithcompletionhandl>
// "If the local player can't be authenticated, GameKit calls your
//  completion handler with an error."
//
// touchHLE has no Game Center connectivity, so we follow the
// documented "not authenticated" branch: emit the change-of-state
// notification, leave `isAuthenticated == NO`, and invoke the
// completion handler with a `GKErrorNotAuthenticated` NSError.
//
// Apple's documentation states that the completion handler is always
// called on the main thread. We enforce this by temporarily switching
// the current_thread to 0 (main) for the callback invocation, so that
// guest code checking `[NSThread isMainThread]` inside the handler
// gets the expected `YES`.
- (())authenticateWithCompletionHandler:(id)completion_handler {
    let error = make_not_authenticated_error(env);

    // Apple posts `GKPlayerAuthenticationDidChangeNotificationName`
    // before invoking the completion handler so registered observers
    // see the new state first. We mirror that ordering.
    let notif_center: id = msg_class![env; NSNotificationCenter defaultCenter];
    let name = ns_string::from_rust_string(
        env,
        GKPlayerAuthenticationDidChangeNotificationName.to_string(),
    );
    autorelease(env, name);
    () = msg![env; notif_center postNotificationName:name object:nil];

    // Ensure the callback runs in main-thread context (thread 0).
    let saved_thread = env.current_thread;
    env.current_thread = 0;
    invoke_error_block(env, completion_handler, error);
    env.current_thread = saved_thread;
}

// Apple reference (iOS 6+):
// <https://developer.apple.com/documentation/gamekit/gklocalplayer/1521050-authenticatehandler>
// "Setting the value of this property triggers authentication. […]
//  If the player needs to sign in, the handler is called with a
//  view controller. If the player cannot sign in, the handler is
//  called with a non-nil error."
//
// We have no UI to present, so we always take the "cannot sign in"
// branch and invoke the handler with a nil view controller and the
// `GKErrorNotAuthenticated` NSError. The handler block is retained
// for the lifetime of the singleton so it survives autorelease pool
// drains, matching what UIKit does internally.
//
// Apple's documentation states that the handler is always called on
// the main thread.
- (())setAuthenticateHandler:(id)handler {
    if handler == nil {
        return;
    }
    // Apple's setter is documented as `copy` — block setters always
    // perform `Block_copy` so the block survives going out of scope
    // in the caller. `-[NSObject copy]` on a heap-allocated block
    // bumps its refcount; on a stack block (rare for property
    // setters) `copy` returns a heap copy.
    let retained_handler: id = msg![env; handler copy];

    let vc: id = nil;
    let error = make_not_authenticated_error(env);

    // Ensure the callback runs in main-thread context (thread 0).
    let saved_thread = env.current_thread;
    env.current_thread = 0;
    invoke_vc_error_block(env, retained_handler, vc, error);
    env.current_thread = saved_thread;

    // Drop our reference once the handler has been invoked. Apps that
    // store the handler themselves are unaffected because Block_copy
    // gives them an independent reference.
    release(env, retained_handler);
}

// MARK: - Friends

- (id)friends {
    env.objc.borrow::<GKLocalPlayerHostObject>(this).friends
}

// Apple reference (iOS 4.1+, deprecated iOS 10):
// <https://developer.apple.com/documentation/gamekit/gklocalplayer/1521101-loadfriendswithcompletionhandle>
// "If the friend list was loaded, this block receives an array of
//  player IDs (NSString). Otherwise, the error parameter contains an
//  NSError object that describes the problem."
//
// The completion block signature is
//   void (^)(NSArray *friends, NSError *error)
// We have no remote service, but we *do* have a deterministic local
// friends array (always empty) — so we report success with an empty
// array, matching what a real device returns when the local player
// is authenticated but has no friends.
- (())loadFriendsWithCompletionHandler:(id)completion_handler {
    if completion_handler == nil {
        return;
    }
    let block_ptr: MutPtr<u32> = Ptr::from_bits(completion_handler.to_bits());
    let invoke_addr: u32 = env.mem.read(block_ptr + BLOCK_INVOKE_WORD_OFFSET);
    if invoke_addr == 0 {
        return;
    }
    let invoke = GuestFunction::from_addr_with_thumb_bit(invoke_addr);
    let block_arg: ConstVoidPtr =
        Ptr::from_bits(completion_handler.to_bits()).cast_const();

    // Hand back the cached empty friends array; do NOT autorelease, the
    // contract is that the block borrows the reference for the call.
    let friends: id = env.objc.borrow::<GKLocalPlayerHostObject>(this).friends;
    let error: id = nil;
    <GuestFunction as CallFromHost<(), (ConstVoidPtr, id, id)>>::call_from_host(
        &invoke, env, (block_arg, friends, error),
    );
}

- (id)description {
    let player_id =
        env.objc.borrow::<GKLocalPlayerHostObject>(this).player_id;
    let id_str = ns_string::to_rust_string(env, player_id);
    let desc = format!("<GKLocalPlayer: playerID={}>", id_str);
    let ns = ns_string::from_rust_string(env, desc);
    crate::objc::autorelease(env, ns)
}

@end

};

pub const GKPlayerAuthenticationDidChangeNotificationName: &str =
    "GKPlayerAuthenticationDidChangeNotificationName";
pub const GKPlayerDidChangeNotificationName: &str = "GKPlayerDidChangeNotificationName";

pub const CONSTANTS: ConstantExports = &[
    (
        "_GKPlayerAuthenticationDidChangeNotificationName",
        HostConstant::NSString(GKPlayerAuthenticationDidChangeNotificationName),
    ),
    (
        "_GKPlayerDidChangeNotificationName",
        HostConstant::NSString(GKPlayerDidChangeNotificationName),
    ),
];
