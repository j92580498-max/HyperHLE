/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIPasteboard` and `UILocalNotification`.

use crate::frameworks::foundation::ns_string;
use crate::objc::{
    id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject, NSZonePtr,
};
use std::collections::HashMap;

// MARK: - UIPasteboard host object

#[derive(Default)]
struct UIPasteboardHostObject {
    /// Имя буфера обмена (NSString*)
    name: id,
    /// Содержимое буфера в виде строки (NSString*)
    string: id,
    /// Хранилище данных по типам (UTI -> NSData)
    data_by_type: HashMap<String, id>,
    /// Флаг, сохраняется ли буфер обмена после завершения приложения
    persistent: bool,
}

impl HostObject for UIPasteboardHostObject {}

// MARK: - UILocalNotification host object

struct UILocalNotificationHostObject {
    /// Дата и время срабатывания уведомления (NSDate*)
    fire_date: id,
    /// Часовой пояс (NSTimeZone*)
    time_zone: id,
    /// Интервал повтора (NSCalendarUnit as NSUInteger)
    repeat_interval: u32,
    /// Календарь для повтора (NSCalendar*)
    repeat_calendar: id,
    /// Текст уведомления (NSString*)
    alert_body: id,
    /// Заголовок уведомления (NSString*)
    alert_title: id,
    /// Текст кнопки действия (NSString*)
    alert_action: id,
    /// Имя файла изображения для launch image (NSString*)
    alert_launch_image: id,
    /// Название звука (NSString*)
    sound_name: id,
    /// Номер значка приложения (NSInteger)
    application_icon_badge_number: i32,
    /// Пользовательские данные (NSDictionary*)
    user_info: id,
    /// Есть ли кнопка действия
    has_action: bool,
}

impl HostObject for UILocalNotificationHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// =========================================================================
// MARK: - UIPasteboard
// =========================================================================

@implementation UIPasteboard: NSObject

// =========================================================================
// МЕТОДЫ КЛАССА (+)
// =========================================================================

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(UIPasteboardHostObject {
        name: nil,
        string: nil,
        data_by_type: HashMap::new(),
        persistent: false, // По умолчанию буфер не персистентный
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)generalPasteboard {
    // В большинстве игр используется для быстрого копирования текста.
    // Создаем базовый инстанс.
    let instance: id = msg_class![env; UIPasteboard alloc];
    let instance: id = msg![env; instance init];

    // generalPasteboard по умолчанию persistent = YES в iOS
    env.objc.borrow_mut::<UIPasteboardHostObject>(instance).persistent = true;

    instance
}

+ (id)pasteboardWithName:(id)name create:(bool)_create {
    let instance: id = msg_class![env; UIPasteboard alloc];
    let instance: id = msg![env; instance init];

    // 1. Сначала делаем retain, пока env свободен
    retain(env, name);

    // 2. Затем заимствуем и сразу записываем (без сохранения переменной host)
    env.objc.borrow_mut::<UIPasteboardHostObject>(instance).name = name;

    instance
}

// =========================================================================
// МЕТОДЫ ЭКЗЕМПЛЯРА (-)
// =========================================================================

- (id)init {
    this
}

- (())dealloc {
    // Забираем данные, чтобы потом освободить их вне заимствования
    let (name, string, data_by_type) = {
        let host = env.objc.borrow_mut::<UIPasteboardHostObject>(this);
        (
            host.name,
            host.string,
            std::mem::take(&mut host.data_by_type),
        )
    };

    release(env, name);
    release(env, string);
    for (_, data) in data_by_type {
        release(env, data);
    }

    env.objc.dealloc_object(this, &mut env.mem)
}

// MARK: - Свойства (Properties)

- (id)name { // NSString*
    env.objc.borrow::<UIPasteboardHostObject>(this).name
}

- (id)string { // NSString*
    env.objc.borrow::<UIPasteboardHostObject>(this).string
}

- (())setString:(id)string { // NSString*
    // Достаем старое значение, чтобы освободить его
    let old = env.objc.borrow::<UIPasteboardHostObject>(this).string;

    // Выполняем операции управления памятью
    release(env, old);
    retain(env, string);

    // Записываем новое значение
    env.objc.borrow_mut::<UIPasteboardHostObject>(this).string = string;
}

- (bool)isPersistent {
    env.objc.borrow::<UIPasteboardHostObject>(this).persistent
}

- (())setPersistent:(bool)value {
    env.objc.borrow_mut::<UIPasteboardHostObject>(this).persistent = value;
}

// MARK: - Работа с данными (NSData)

- (id)dataForPasteboardType:(id)pasteboard_type { // NSData*, NSString*
    if pasteboard_type == nil {
        return nil;
    }

    let type_str = ns_string::to_rust_string(env, pasteboard_type);
    let host = env.objc.borrow::<UIPasteboardHostObject>(this);

    // Используем .as_ref() чтобы передать &str вместо &Cow
    if let Some(&data) = host.data_by_type.get(type_str.as_ref()) {
        data
    } else {
        nil
    }
}

// valueForPasteboardType: is the older iOS API name for dataForPasteboardType:
- (id)valueForPasteboardType:(id)pasteboard_type {
    msg![env; this dataForPasteboardType:pasteboard_type]
}

// setValue:forPasteboardType: is the older iOS API name for setData:forPasteboardType:
- (())setValue:(id)value forPasteboardType:(id)pasteboard_type {
    () = msg![env; this setData:value forPasteboardType:pasteboard_type];
}

- (())setData:(id)data forPasteboardType:(id)pasteboard_type { // NSData*, NSString*
    if pasteboard_type == nil {
        return;
    }

    let type_str = ns_string::to_rust_string(env, pasteboard_type);

    if data != nil {
        retain(env, data);
    }

    // Безопасно обновляем HashMap и забираем старое значение
    let old_data = {
        let host = env.objc.borrow_mut::<UIPasteboardHostObject>(this);
        if data != nil {
            // Превращаем Cow во владеющий String через .into_owned()
            host.data_by_type.insert(type_str.into_owned(), data)
        } else {
            // Используем .as_ref() чтобы передать &str вместо &Cow
            host.data_by_type.remove(type_str.as_ref())
        }
    };

    // Освобождаем старое значение вне borrow_mut
    if let Some(old) = old_data {
        release(env, old);
    }
}

@end

};
