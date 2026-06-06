/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Stub for `MessageUI.framework/MessageUI`.
//!
//! On iOS, MessageUI provides `MFMailComposeViewController` and
//! `MFMessageComposeViewController` for in-app email/SMS composition. The
//! vast majority of touchHLE's target apps don't actually open a compose
//! sheet — they just list MessageUI as a Mach-O dependency, often pulled
//! in transitively by an SDK or from a "Send to a friend" feature that's
//! never wired up at runtime.
//!
//! Without a [crate::dyld::HostDylib] entry for the path
//! `/System/Library/Frameworks/MessageUI.framework/MessageUI`, touchHLE
//! prints a `Warning: app binary depends on unimplemented or missing
//! dylib …` at startup, which can spook users into reporting otherwise-
//! fine apps as broken (e.g. HyperHLE appdb report #74, Smack It).
//!
//! `MFMailComposeViewController` (+canSendMail, etc.) is implemented in
//! [crate::frameworks::media_player::mf_mail_compose_view_controller] and
//! exported via the MediaPlayer framework dylib list. This module exists
//! only so the MessageUI Mach-O path is recognized and the warning is
//! suppressed.

use crate::dyld::FunctionExports;

pub const FUNCTIONS: FunctionExports = &[];
