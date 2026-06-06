/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! GameKit framework.
//!
//! Some features of this framework are only in iOS 4.1+, but some games (like
//! "Cut the Rope") may use it to check for game center availability with
//! a `respondsToSelector:` call to some objects of this framework.
//!
//! Thus, we need to provide some stubs in order to not crash on that call.

pub mod ad_banner_view;
pub mod fb_session;
pub mod gk_achievement;
pub mod gk_challenge_event_handler;
mod gk_leaderboard;
pub mod gk_leaderboard_view_controller;
pub mod gk_local_player;
mod gk_score;
mod gk_session;

use crate::dyld::{ConstantExports, HostConstant};

/// Apple GameKit framework — `GKError.h`. `GKErrorDomain` is the
/// `NSError.domain` value used for every error reported by GameKit
/// APIs (Game Center authentication, leaderboards, achievements,
/// matchmaking). Apps compare against it with
/// `[error.domain isEqualToString:GKErrorDomain]` to filter
/// GameKit-specific failures, so the literal must match Apple's.
pub const CONSTANTS: ConstantExports =
    &[("_GKErrorDomain", HostConstant::NSString("GKErrorDomain"))];

/// Per-process state for the GameKit framework.
#[derive(Default)]
pub struct State {
    pub local_player: gk_local_player::State,
    pub challenge_event_handler: gk_challenge_event_handler::State,
}

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/System/Library/Frameworks/GameKit.framework/GameKit",
    aliases: &[],
    class_exports: &[
        ad_banner_view::CLASSES,
        fb_session::CLASSES,
        gk_achievement::CLASSES,
        gk_challenge_event_handler::CLASSES,
        gk_leaderboard::CLASSES,
        gk_leaderboard_view_controller::CLASSES,
        gk_local_player::CLASSES,
        gk_score::CLASSES,
        gk_session::CLASSES,
    ],
    constant_exports: &[
        gk_local_player::CONSTANTS,
        ad_banner_view::CONSTANTS,
        CONSTANTS,
    ],
    function_exports: &[],
};
