/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UITouch`.

use super::ui_event;
use crate::frameworks::core_graphics::{CGPoint, CGRect};
use crate::frameworks::foundation::{NSInteger, NSTimeInterval, NSUInteger};
use crate::mem::MutVoidPtr;
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};
use crate::window::{Coords, Event, FingerId};
use crate::Environment;
use std::collections::hash_map::{Entry, HashMap};
use std::collections::HashSet;

pub type UITouchPhase = NSInteger;
pub const UITouchPhaseBegan: UITouchPhase = 0;
pub const UITouchPhaseMoved: UITouchPhase = 1;
pub const UITouchPhaseStationary: UITouchPhase = 2;
pub const UITouchPhaseEnded: UITouchPhase = 3;

#[derive(Default)]
pub struct State {
    pub current_touches: HashMap<FingerId, id>,
}

#[derive(Default)]
pub(super) struct UITouchHostObject {
    pub(super) view: id,
    pub(super) window: id,
    location: CGPoint,
    previous_location: CGPoint,
    timestamp: NSTimeInterval,
    phase: UITouchPhase,
}
impl HostObject for UITouchHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UITouch: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(UITouchHostObject {
        view: nil,
        window: nil,
        location: CGPoint { x: 0.0, y: 0.0 },
        previous_location: CGPoint { x: 0.0, y: 0.0 },
        timestamp: 0.0,
        phase: UITouchPhaseBegan,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())dealloc {
    let &mut UITouchHostObject { view, window, .. } = env.objc.borrow_mut(this);
    release(env, view);
    release(env, window);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (CGPoint)locationInView:(id)that_view {
    let &UITouchHostObject { location, window, .. } = env.objc.borrow(this);
    let location_in_window: CGPoint = msg![env; window
        convertPoint:location fromWindow:nil];
    if that_view == nil {
        location_in_window
    } else {
        msg![env;
        that_view convertPoint:location_in_window fromView:window]
    }
}

- (CGPoint)previousLocationInView:(id)that_view {
    let &UITouchHostObject { previous_location, window, .. } = env.objc.borrow(this);
    let location_in_window: CGPoint = msg![env; window
        convertPoint:previous_location fromWindow:nil];
    if that_view == nil {
        location_in_window
    } else {
        msg![env;
        that_view convertPoint:location_in_window fromView:window]
    }
}

- (id)view {
    env.objc.borrow::<UITouchHostObject>(this).view
}

- (id)window {
    env.objc.borrow::<UITouchHostObject>(this).window
}

- (NSTimeInterval)timestamp {
    env.objc.borrow::<UITouchHostObject>(this).timestamp
}

- (NSUInteger)tapCount {
    1
}

- (UITouchPhase)phase {
    env.objc.borrow::<UITouchHostObject>(this).phase
}

@end

};

pub fn handle_event(env: &mut Environment, event: Event) {
    let touch_ids: Vec<id> = env
        .framework_state
        .uikit
        .ui_touch
        .current_touches
        .values()
        .cloned()
        .collect();
    for touch in touch_ids {
        env.objc.borrow_mut::<UITouchHostObject>(touch).phase = UITouchPhaseStationary;
    }
    match event {
        Event::TouchesDown(map) => handle_touches_down(env, map),
        Event::TouchesMove(map) => handle_touches_move(env, map),
        Event::TouchesUp(map) => handle_touches_up(env, map),
        other => {
            // ui_touch::handle_event only ever wants touch events; non-touch
            // events are filtered out before getting here. Log instead of
            // panicking the host if that contract is ever violated.
            log!(
                "Warning: ui_touch::handle_event: unsupported event {:?}; ignored.",
                other
            );
        }
    }
}

fn handle_touches_down(env: &mut Environment, map: HashMap<FingerId, Coords>) {
    let pool: id = msg_class![env;
        NSAutoreleasePool new];

    let timestamp: NSTimeInterval = {
        let process_info = msg_class![env; NSProcessInfo processInfo];
        msg![env; process_info systemUptime]
    };

    let touches: id = msg_class![env;
        NSMutableSet
        allocWithZone:(MutVoidPtr::null())];

    for (finger_id, coords) in map {
        if env
            .framework_state
            .uikit
            .ui_touch
            .current_touches
            .contains_key(&finger_id)
        {
            log!(
                "Warning: New touch {:?} initiated but old one exists.",
                finger_id
            );
            return handle_touches_move(env, HashMap::from([(finger_id, coords)]));
        }

        let location = CGPoint {
            x: coords.0,
            y: coords.1,
        };
        let new_touch: id = msg_class![env; UITouch alloc];
        *env.objc.borrow_mut(new_touch) = UITouchHostObject {
            view: nil,
            window: nil,
            location,
            previous_location: location,
            timestamp,
            phase: UITouchPhaseBegan,
        };
        autorelease(env, new_touch);

        let _: () = msg![env; touches addObject:new_touch];
        env.framework_state
            .uikit
            .ui_touch
            .current_touches
            .insert(finger_id, new_touch);
        retain(env, new_touch);
    }

    let all_touches_set: id = msg_class![env; NSMutableSet
        allocWithZone:(MutVoidPtr::null())];
    let existing_touches: Vec<id> = env
        .framework_state
        .uikit
        .ui_touch
        .current_touches
        .values()
        .cloned()
        .collect();
    for touch in existing_touches {
        let _: () = msg![env; all_touches_set addObject:touch];
    }

    let event = ui_event::new_event(env, all_touches_set);
    autorelease(env, event);
    let views_with_existing_touches: HashSet<id> = env
        .framework_state
        .uikit
        .ui_touch
        .current_touches
        .values()
        .map(|&touch| env.objc.borrow::<UITouchHostObject>(touch).view)
        .collect();
    let mut view_touches: HashMap<id, id> = HashMap::new();
    let touches_arr: id = msg![env; touches allObjects];
    let touches_count: NSUInteger = msg![env;
        touches_arr count];

    for i in 0..touches_count {
        let touch: id = msg![env;
            touches_arr objectAtIndex:i];
        let &UITouchHostObject { location, .. } = env.objc.borrow(touch);

        let windows = env.framework_state.uikit.ui_view.ui_window.windows.clone();
        let found_window = windows.iter().rev().find_map(|&window| {
            let location_in_window: CGPoint = msg![env; window
                convertPoint:location fromWindow:nil];
            if msg![env; window pointInside:location_in_window withEvent:event] {
                Some((window, location_in_window))
            } else {
                None
            }
        });
        // SUPER HACK: Если окно отвергло касание, силой отправляем его в
        // главное окно!
        let Some((window, location_in_window)) = found_window.or_else(|| {
            windows.last().map(|&window| {
                let lx = location.x;
                let ly = location.y;
                log!(
                    "SUPER HACK: Forcing rejected touch at ({}, {}) into window",
                    lx,
                    ly
                );
                let loc: CGPoint = msg![env; window convertPoint:location fromWindow:nil];
                (window, loc)
            })
        }) else {
            let lx = location.x;
            let ly = location.y;
            log!(
                "Couldn't find ANY window for touch at ({}, {}), discarding",
                lx,
                ly
            );
            continue;
        };
        let mut view: id = msg![env; window hitTest:location_in_window withEvent:event];
        if view == nil {
            log!("SUPER HACK: hitTest failed, forcing touch directly into the window");
            view = window;
        } else {
            let f: CGRect = msg![env;
                view frame];
            let view_class: crate::objc::Class = msg![env; view class];
            let class_name = env.objc.get_class_name(view_class).to_owned();
            let lx = location_in_window.x;
            let ly = location_in_window.y;
            log!(
                "Touch at ({}, {}) hit {} {:?} with frame {:?}",
                lx,
                ly,
                class_name,
                view,
                f,
            );
        }

        let is_multi_touch_enabled: bool = msg![env; view isMultipleTouchEnabled];
        if !is_multi_touch_enabled
            && (view_touches.contains_key(&view) || views_with_existing_touches.contains(&view))
        {
            let stuck: Vec<FingerId> = env
                .framework_state
                .uikit
                .ui_touch
                .current_touches
                .iter()
                .filter(|(_, &t)| {
                    env.objc.borrow::<UITouchHostObject>(t).view == view && t != touch
                })
                .map(|(&fid, _)| fid)
                .collect();
            if !stuck.is_empty() {
                for fid in stuck {
                    if let Some(t) = env
                        .framework_state
                        .uikit
                        .ui_touch
                        .current_touches
                        .remove(&fid)
                    {
                        release(env, t);
                    }
                }
            } else {
                continue;
            }
        }

        if let Entry::Vacant(e) = view_touches.entry(view) {
            let s: id = msg_class![env;
                NSMutableSet
                allocWithZone:(MutVoidPtr::null())];
            e.insert(s);
        }
        let v_set: id = *view_touches.get(&view).unwrap();
        let _: () = msg![env; v_set addObject:touch];
        retain(env, view);
        retain(env, window);
        {
            let t_obj = env.objc.borrow_mut::<UITouchHostObject>(touch);
            t_obj.view = view;
            t_obj.window = window;
            t_obj.location = location;
        }
    }

    for (view, v_set) in view_touches {
        let _: () = msg![env;
            view touchesBegan:v_set withEvent:event];
    }
    release(env, pool);
}

fn handle_touches_move(env: &mut Environment, map: HashMap<FingerId, Coords>) {
    let pool: id = msg_class![env;
        NSAutoreleasePool new];
    let timestamp: NSTimeInterval = {
        let pi = msg_class![env; NSProcessInfo processInfo];
        msg![env; pi systemUptime]
    };

    let mut view_touches: HashMap<id, id> = HashMap::new();
    for (finger_id, coords) in map {
        let Some(&touch) = env
            .framework_state
            .uikit
            .ui_touch
            .current_touches
            .get(&finger_id)
        else {
            continue;
        };
        let location = CGPoint {
            x: coords.0,
            y: coords.1,
        };
        let view = env.objc.borrow::<UITouchHostObject>(touch).view;
        let host = env.objc.borrow_mut::<UITouchHostObject>(touch);
        if host.location == location {
            continue;
        }
        host.previous_location = host.location;
        host.location = location;
        host.timestamp = timestamp;
        host.phase = UITouchPhaseMoved;

        if let Entry::Vacant(e) = view_touches.entry(view) {
            let s: id = msg_class![env;
                NSMutableSet
                allocWithZone:(MutVoidPtr::null())];
            e.insert(s);
        }
        let v_set: id = *view_touches.get(&view).unwrap();
        let _: () = msg![env; v_set addObject:touch];
    }

    let all_touches_set: id = msg_class![env; NSMutableSet
        allocWithZone:(MutVoidPtr::null())];
    let existing: Vec<id> = env
        .framework_state
        .uikit
        .ui_touch
        .current_touches
        .values()
        .cloned()
        .collect();
    for t in existing {
        let _: () = msg![env; all_touches_set addObject:t];
    }

    let event = ui_event::new_event(env, all_touches_set);
    autorelease(env, event);
    for (view, v_set) in view_touches {
        let _: () = msg![env;
            view touchesMoved:v_set withEvent:event];
    }
    release(env, pool);
}

fn handle_touches_up(env: &mut Environment, map: HashMap<FingerId, Coords>) {
    let pool: id = msg_class![env;
        NSAutoreleasePool new];
    let timestamp: NSTimeInterval = {
        let pi = msg_class![env; NSProcessInfo processInfo];
        msg![env; pi systemUptime]
    };

    let touches: id = msg_class![env;
        NSMutableSet
        allocWithZone:(MutVoidPtr::null())];
    let all_touches_set: id = msg_class![env;
        NSMutableSet
        allocWithZone:(MutVoidPtr::null())];
    let existing: Vec<id> = env
        .framework_state
        .uikit
        .ui_touch
        .current_touches
        .values()
        .cloned()
        .collect();
    for t in existing {
        let _: () = msg![env; all_touches_set addObject:t];
    }

    let mut view_touches: HashMap<id, id> = HashMap::new();
    for (finger_id, coords) in map {
        let Some(&touch) = env
            .framework_state
            .uikit
            .ui_touch
            .current_touches
            .get(&finger_id)
        else {
            continue;
        };
        let location = CGPoint {
            x: coords.0,
            y: coords.1,
        };
        let view = env.objc.borrow::<UITouchHostObject>(touch).view;
        {
            let host = env.objc.borrow_mut::<UITouchHostObject>(touch);
            host.previous_location = host.location;
            host.location = location;
            host.timestamp = timestamp;
            host.phase = UITouchPhaseEnded;
        }

        let _: () = msg![env;
            touches addObject:touch];

        if let Entry::Vacant(e) = view_touches.entry(view) {
            let s: id = msg_class![env;
                NSMutableSet
                allocWithZone:(MutVoidPtr::null())];
            e.insert(s);
        }
        let v_set: id = *view_touches.get(&view).unwrap();
        let _: () = msg![env; v_set addObject:touch];
        env.framework_state
            .uikit
            .ui_touch
            .current_touches
            .remove(&finger_id);
        release(env, touch);
    }

    let event = ui_event::new_event(env, all_touches_set);
    autorelease(env, event);
    for (view, v_set) in view_touches {
        let _: () = msg![env;
            view touchesEnded:v_set withEvent:event];
    }
    release(env, pool);
}
