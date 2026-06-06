/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UITextView`.

use crate::frameworks::core_graphics::cg_context::CGContextSetRGBFillColor;
use crate::frameworks::core_graphics::cg_geometry::CGPointZero;
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_string::{get_static_str, to_rust_string};
use crate::frameworks::foundation::{NSInteger, NSRange, NSUInteger};
use crate::frameworks::uikit::ui_color;
use crate::frameworks::uikit::ui_font::{
    break_lines_with_font, UILineBreakModeTailTruncation, UILineBreakModeWordWrap, UITextAlignment,
    UITextAlignmentLeft,
};
use crate::frameworks::uikit::ui_graphics::UIGraphicsGetCurrentContext;
use crate::frameworks::uikit::ui_view::ui_control::ui_text_field::UIReturnKeyType;
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_class, msg_super, nil, objc_classes, release,
    retain, ClassExports, NSZonePtr,
};
use crate::Environment;

type UIDataDetectorTypes = NSUInteger;
type UIKeyboardType = NSInteger;
type UIKeyboardAppearance = NSInteger;
type UITextAutocapitalizationType = NSInteger;
type UITextAutocorrectionType = NSInteger;

pub struct UITextViewHostObject {
    superclass: super::UIScrollViewHostObject,
    editable: bool,
    text: id,
    font: id,
    text_color: id,
    text_alignment: UITextAlignment,
    /// Per UITextInputTraits — these properties are read/write on every
    /// concrete UITextView. touchHLE doesn't yet route keystrokes through
    /// the iOS keyboard subsystem, but storing the values is sufficient
    /// for round-trip get/set fidelity per Apple's reference docs.
    return_key_type: NSInteger,
    keyboard_type: NSInteger,
    keyboard_appearance: NSInteger,
    autocapitalization_type: NSInteger,
    autocorrection_type: NSInteger,
    secure_text_entry: bool,
    data_detector_types: NSUInteger,
    /// Per Apple's
    /// <https://developer.apple.com/documentation/uikit/uiresponder/1621119-inputaccessoryview>
    /// and <https://developer.apple.com/documentation/uikit/uiresponder/1621072-inputview>,
    /// every `UIResponder` (and therefore every UITextView) supports
    /// `inputView` / `inputAccessoryView` — strong-ish references to
    /// auxiliary views shown above the keyboard. We store the pointer
    /// the app hands us so the getter returns the same object the
    /// setter took, exactly like the documented contract.
    input_accessory_view: id,
    input_view: id,
}
impl_HostObject_with_superclass!(UITextViewHostObject);

impl Default for UITextViewHostObject {
    fn default() -> Self {
        UITextViewHostObject {
            superclass: Default::default(),
            editable: false,
            font: nil,
            text: nil,
            text_color: nil,
            text_alignment: UITextAlignmentLeft,
            return_key_type: 0,
            keyboard_type: 0,
            keyboard_appearance: 0,
            autocapitalization_type: 0,
            autocorrection_type: 0,
            secure_text_entry: false,
            data_detector_types: 0,
            input_accessory_view: nil,
            input_view: nil,
        }
    }
}

fn update_scroll(env: &mut Environment, this: id) {
    let bounds: CGRect = msg![env; this bounds];
    let bound_size = bounds.size;
    let font: id = msg![env; this font];
    let text: id = msg![env; this text];
    if text == nil || font == nil {
        return;
    }

    let calculated_size: CGSize = msg![env; text sizeWithFont:font constrainedToSize:bound_size];
    () = msg![env; this setContentSize:calculated_size];

    let current_content_offset: CGPoint = msg![env; this contentOffset];
    if current_content_offset.x > calculated_size.width - bounds.size.width
        || current_content_offset.y > calculated_size.height - bounds.size.height
    {
        () = msg![env; this setContentOffset:(CGPoint { x: 0.0, y: 0.0 })];
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);
@implementation UITextView: UIScrollView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UITextViewHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithFrame:(CGRect)frame {
    let this: id = msg_super![env; this initWithFrame:frame];
    () = msg![env; this setFont:nil];
    () = msg![env; this setTextColor:nil];
    this
}

- (id)initWithCoder:(id)coder {
    let this: id = msg_super![env; this initWithCoder:coder];

    let key_text = get_static_str(env, "UIText");
    if msg![env; coder containsValueForKey:key_text] {
        let text: id = msg![env; coder decodeObjectForKey:key_text];
        () = msg![env; this setText:text];
    }

    let key_font = get_static_str(env, "UIFont");
    if msg![env; coder containsValueForKey:key_font] {
        let font: id = msg![env; coder decodeObjectForKey:key_font];
        () = msg![env; this setFont:font];
    } else {
        () = msg![env; this setFont:nil];
    }

    let key_color = get_static_str(env, "UITextColor");
    if msg![env; coder containsValueForKey:key_color] {
        let text_color: id = msg![env; coder decodeObjectForKey:key_color];
        () = msg![env; this setTextColor:text_color];
    } else {
        () = msg![env; this setTextColor:nil];
    }

    let key_align = get_static_str(env, "UITextAlignment");
    if msg![env; coder containsValueForKey:key_align] {
        let align: UITextAlignment = msg![env; coder decodeIntegerForKey:key_align];
        () = msg![env; this setTextAlignment:align];
    }

    this
}

- (())dealloc {
    let UITextViewHostObject { font, text, text_color, .. } = std::mem::take(env.objc.borrow_mut(this));
    release(env, font);
    release(env, text_color);
    release(env, text);
    msg_super![env; this dealloc]
}

- (id)text { env.objc.borrow::<UITextViewHostObject>(this).text }
- (())setText:(id)new_text {
    let new_text: id = if new_text != nil { msg![env; new_text copy] } else { nil };
    let hostobj  = env.objc.borrow_mut::<UITextViewHostObject>(this);
    let old_text = std::mem::replace(&mut hostobj.text, new_text);
    release(env, old_text);
    update_scroll(env,this);
    () = msg![env; this setNeedsDisplay];
}

- (id)textColor { env.objc.borrow::<UITextViewHostObject>(this).text_color }
- (())setTextColor:(id)new_text_color {
    let new_text_color: id = if new_text_color == nil {
        msg_class![env; UIColor blackColor]
    } else { new_text_color };
    let hostobj  = env.objc.borrow_mut::<UITextViewHostObject>(this);
    let old_text_color = std::mem::replace(&mut hostobj.text_color, new_text_color);
    retain(env, new_text_color);
    release(env, old_text_color);
    () = msg![env; this setNeedsDisplay];
}

- (UITextAlignment)textAlignment { env.objc.borrow::<UITextViewHostObject>(this).text_alignment }
- (())setTextAlignment:(UITextAlignment)new_text_alignment {
    env.objc.borrow_mut::<UITextViewHostObject>(this).text_alignment = new_text_alignment;
    () = msg![env; this setNeedsDisplay];
}

- (id)font { env.objc.borrow::<UITextViewHostObject>(this).font }
- (())setFont:(id)new_font {
    let new_font: id = if new_font == nil {
        let size: CGFloat = 17.0;
        msg_class![env; UIFont systemFontOfSize:size]
    } else { new_font };
    let hostobj  = env.objc.borrow_mut::<UITextViewHostObject>(this);
    let old_font = std::mem::replace(&mut hostobj.font, new_font);
    retain(env, new_font);
    release(env, old_font);
    update_scroll(env,this);
    () = msg![env; this setNeedsDisplay];
}

- (())flashScrollIndicators { }

- (bool)isEditable { env.objc.borrow::<UITextViewHostObject>(this).editable }
- (())setEditable:(bool)editable { env.objc.borrow_mut::<UITextViewHostObject>(this).editable = editable; }

- (())scrollRangeToVisible:(NSRange)range {
    let &mut UITextViewHostObject { font, text, .. } = env.objc.borrow_mut(this);
    if text == nil || font == nil || range.location > msg![env; text length] { return; }

    let bounds: CGRect = msg![env; this bounds];
    let bound_size = bounds.size;

    let text_str = to_rust_string(env, text);
    let lines = break_lines_with_font(env, font, &text_str, Some((bound_size, UILineBreakModeWordWrap)));

    let mut line_count = 0;
    let mut current_position = 0;
    for (_, line) in lines {
        current_position += line.len();
        if let Some(offset) = text_str[current_position..].find(|c: char| !c.is_whitespace()) {
            current_position += offset;
        } else {
            current_position = text_str.len();
        }

        if range.location <= current_position as u32 { break; }
        line_count += 1;
    }

    let line_height: CGFloat = msg![env; font lineHeight];
    let leading: CGFloat = msg![env; font leading];
    let height_to_range_start = (line_count + 1) as f32 * line_height - leading;
    let content_offset: CGPoint = msg![env; this contentOffset];

    if height_to_range_start - line_height < content_offset.y {
        let new_scroll_y = CGPoint {x: 0.0, y: line_count as f32 * line_height};
        () = msg![env; this setContentOffset:new_scroll_y];
    } else if height_to_range_start > content_offset.y + bound_size.height {
        let new_scroll_y = CGPoint {x: 0.0, y: height_to_range_start- bound_size.height};
        () = msg![env; this setContentOffset:new_scroll_y];
    }
    update_scroll(env, this);
}

// UITextInputTraits getters/setters (Apple reference:
// https://developer.apple.com/documentation/uikit/uitextinputtraits).
// We persist the values so apps reading them back observe what they set.
- (UIReturnKeyType)returnKeyType {
    env.objc.borrow::<UITextViewHostObject>(this).return_key_type
}
- (())setReturnKeyType:(UIReturnKeyType)type_ {
    env.objc.borrow_mut::<UITextViewHostObject>(this).return_key_type = type_;
}
- (UIKeyboardType)keyboardType {
    env.objc.borrow::<UITextViewHostObject>(this).keyboard_type
}
- (())setKeyboardType:(UIKeyboardType)type_ {
    env.objc.borrow_mut::<UITextViewHostObject>(this).keyboard_type = type_;
}
- (UIKeyboardAppearance)keyboardAppearance {
    env.objc.borrow::<UITextViewHostObject>(this).keyboard_appearance
}
- (())setKeyboardAppearance:(UIKeyboardAppearance)appearance {
    env.objc.borrow_mut::<UITextViewHostObject>(this).keyboard_appearance = appearance;
}
- (UITextAutocapitalizationType)autocapitalizationType {
    env.objc.borrow::<UITextViewHostObject>(this).autocapitalization_type
}
- (())setAutocapitalizationType:(UITextAutocapitalizationType)type_ {
    env.objc.borrow_mut::<UITextViewHostObject>(this).autocapitalization_type = type_;
}
- (UITextAutocorrectionType)autocorrectionType {
    env.objc.borrow::<UITextViewHostObject>(this).autocorrection_type
}
- (())setAutocorrectionType:(UITextAutocorrectionType)type_ {
    env.objc.borrow_mut::<UITextViewHostObject>(this).autocorrection_type = type_;
}
// Apple docs: https://developer.apple.com/documentation/uikit/uitextinputtraits/1624427-issecuretextentry
// Identifies whether the text object should hide the text being entered.
- (bool)isSecureTextEntry {
    env.objc.borrow::<UITextViewHostObject>(this).secure_text_entry
}
- (())setSecureTextEntry:(bool)secure {
    env.objc.borrow_mut::<UITextViewHostObject>(this).secure_text_entry = secure;
}
- (UIDataDetectorTypes)dataDetectorTypes {
    env.objc.borrow::<UITextViewHostObject>(this).data_detector_types
}
- (())setDataDetectorTypes:(UIDataDetectorTypes)types {
    env.objc.borrow_mut::<UITextViewHostObject>(this).data_detector_types = types;
}

- (())drawRect:(CGRect)_rect {
    let bounds: CGRect = msg![env; this bounds];
    let context = UIGraphicsGetCurrentContext(env);
    let &mut UITextViewHostObject { font, text, text_color, text_alignment, .. } = env.objc.borrow_mut(this);

    if text == nil || font == nil || text_color == nil { return; }
    let len: NSUInteger = msg![env; text length];
    if len == 0 { return; }

    let (r, g, b, a) = ui_color::get_rgba(&env.objc, text_color);
    CGContextSetRGBFillColor(env, context, r, g, b, a);

    let content_offset: CGPoint = msg![env; this contentOffset];
    let rect = CGRect {
        origin: CGPointZero,
        size: CGSize {
            width: bounds.size.width + content_offset.x,
            height: bounds.size.height + content_offset.y,
        }
    };
    let _size: CGSize = msg![env; text drawInRect:rect
                                         withFont:font
                                    lineBreakMode:UILineBreakModeTailTruncation
                                        alignment:text_alignment];
}

// MARK: - UIResponder input-accessory / input-view properties.

- (id)inputAccessoryView {
    env.objc.borrow::<UITextViewHostObject>(this).input_accessory_view
}
- (())setInputAccessoryView:(id)view {
    let old = env.objc.borrow::<UITextViewHostObject>(this).input_accessory_view;
    if old != view {
        retain(env, view);
        if old != nil { release(env, old); }
        env.objc.borrow_mut::<UITextViewHostObject>(this).input_accessory_view = view;
    }
}
- (id)inputView {
    env.objc.borrow::<UITextViewHostObject>(this).input_view
}
- (())setInputView:(id)view {
    let old = env.objc.borrow::<UITextViewHostObject>(this).input_view;
    if old != view {
        retain(env, view);
        if old != nil { release(env, old); }
        env.objc.borrow_mut::<UITextViewHostObject>(this).input_view = view;
    }
}

@end

};
