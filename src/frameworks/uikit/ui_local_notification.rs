/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UILocalNotification`.

use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::foundation::NSUInteger;
use crate::objc::{
    id, nil, objc_classes, release, retain, ClassExports, HostObject, NSZonePtr,
};

#[derive(Default)]
pub(super) struct UILocalNotificationHostObject {
    fire_date: id,
    time_zone: id,
    repeat_interval: NSUInteger,
    repeat_calendar: id,
    alert_body: id,
    alert_action: id,
    alert_launch_image: id,
    sound_name: id,
    application_icon_badge_number: i32,
    user_info: id,
    has_action: bool,
}
impl HostObject for UILocalNotificationHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UILocalNotification: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(UILocalNotificationHostObject {
        fire_date: nil,
        time_zone: nil,
        repeat_interval: 0,
        repeat_calendar: nil,
        alert_body: nil,
        alert_action: nil,
        alert_launch_image: nil,
        sound_name: nil,
        application_icon_badge_number: 0,
        user_info: nil,
        has_action: true,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)init {
    this
}

- (())dealloc {
    let host = env.objc.borrow::<UILocalNotificationHostObject>(this);
    let (fd, tz, rc, ab, aa, ali, sn, ui) = (
        host.fire_date,
        host.time_zone,
        host.repeat_calendar,
        host.alert_body,
        host.alert_action,
        host.alert_launch_image,
        host.sound_name,
        host.user_info,
    );
    release(env, fd);
    release(env, tz);
    release(env, rc);
    release(env, ab);
    release(env, aa);
    release(env, ali);
    release(env, sn);
    release(env, ui);

    env.objc.dealloc_object(this, &mut env.mem);
}

// Геттеры и сеттеры свойств
- (id)fireDate { env.objc.borrow::<UILocalNotificationHostObject>(this).fire_date }
- (())setFireDate:(id)val {
    let old = env.objc.borrow::<UILocalNotificationHostObject>(this).fire_date;
    retain(env, val);
    release(env, old);
    env.objc.borrow_mut::<UILocalNotificationHostObject>(this).fire_date = val;
}

- (id)timeZone { env.objc.borrow::<UILocalNotificationHostObject>(this).time_zone }
- (())setTimeZone:(id)val {
    let old = env.objc.borrow::<UILocalNotificationHostObject>(this).time_zone;
    retain(env, val);
    release(env, old);
    env.objc.borrow_mut::<UILocalNotificationHostObject>(this).time_zone = val;
}


- (NSUInteger)repeatInterval { env.objc.borrow::<UILocalNotificationHostObject>(this).repeat_interval }
- (())setRepeatInterval:(NSUInteger)val {
    env.objc.borrow_mut::<UILocalNotificationHostObject>(this).repeat_interval = val;
}

- (id)repeatCalendar { env.objc.borrow::<UILocalNotificationHostObject>(this).repeat_calendar }
- (())setRepeatCalendar:(id)val {
    let old = env.objc.borrow::<UILocalNotificationHostObject>(this).repeat_calendar;
    retain(env, val);
    release(env, old);
    env.objc.borrow_mut::<UILocalNotificationHostObject>(this).repeat_calendar = val;
}

- (id)alertBody { env.objc.borrow::<UILocalNotificationHostObject>(this).alert_body }
- (())setAlertBody:(id)val {
    let old = env.objc.borrow::<UILocalNotificationHostObject>(this).alert_body;
    retain(env, val);
    release(env, old);
    env.objc.borrow_mut::<UILocalNotificationHostObject>(this).alert_body = val;
}

- (id)alertAction { env.objc.borrow::<UILocalNotificationHostObject>(this).alert_action }
- (())setAlertAction:(id)val {
    let old = env.objc.borrow::<UILocalNotificationHostObject>(this).alert_action;
    retain(env, val);
    release(env, old);
    env.objc.borrow_mut::<UILocalNotificationHostObject>(this).alert_action = val;
}


- (id)alertLaunchImage { env.objc.borrow::<UILocalNotificationHostObject>(this).alert_launch_image }
- (())setAlertLaunchImage:(id)val {
    let old = env.objc.borrow::<UILocalNotificationHostObject>(this).alert_launch_image;
    retain(env, val);
    release(env, old);
    env.objc.borrow_mut::<UILocalNotificationHostObject>(this).alert_launch_image = val;
}

- (bool)hasAction { env.objc.borrow::<UILocalNotificationHostObject>(this).has_action }
- (())setHasAction:(bool)val {
    env.objc.borrow_mut::<UILocalNotificationHostObject>(this).has_action = val;
}

- (id)soundName { env.objc.borrow::<UILocalNotificationHostObject>(this).sound_name }
- (())setSoundName:(id)val {
    let old = env.objc.borrow::<UILocalNotificationHostObject>(this).sound_name;
    retain(env, val);
    release(env, old);
    env.objc.borrow_mut::<UILocalNotificationHostObject>(this).sound_name = val;
}

- (i32)applicationIconBadgeNumber { env.objc.borrow::<UILocalNotificationHostObject>(this).application_icon_badge_number }
- (())setApplicationIconBadgeNumber:(i32)val {
    env.objc.borrow_mut::<UILocalNotificationHostObject>(this).application_icon_badge_number = val;
}

- (id)userInfo { env.objc.borrow::<UILocalNotificationHostObject>(this).user_info }
- (())setUserInfo:(id)val {
    let old = env.objc.borrow::<UILocalNotificationHostObject>(this).user_info;
    retain(env, val);
    release(env, old);
    env.objc.borrow_mut::<UILocalNotificationHostObject>(this).user_info = val;
}

@end

};

// ЭКСПОРТ КОНСТАНТЫ (Решает ошибку линковщика)
pub const CONSTANTS: ConstantExports = &[(
    "_UILocalNotificationDefaultSoundName",
    HostConstant::NSString("UILocalNotificationDefaultSoundName"),
)];
