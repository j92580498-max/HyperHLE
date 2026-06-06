/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `AVAudioSession`.

use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::foundation::{ns_string, NSInteger, NSUInteger};
use crate::mem::MutPtr;
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
};

// MARK: - Category constants

pub const AVAudioSessionCategoryAmbient: &str = "AVAudioSessionCategoryAmbient";
pub const AVAudioSessionCategorySoloAmbient: &str = "AVAudioSessionCategorySoloAmbient";
pub const AVAudioSessionCategoryPlayback: &str = "AVAudioSessionCategoryPlayback";
pub const AVAudioSessionCategoryRecord: &str = "AVAudioSessionCategoryRecord";
pub const AVAudioSessionCategoryPlayAndRecord: &str = "AVAudioSessionCategoryPlayAndRecord";
pub const AVAudioSessionCategoryAudioProcessing: &str = "AVAudioSessionCategoryAudioProcessing";
pub const AVAudioSessionCategoryMultiRoute: &str = "AVAudioSessionCategoryMultiRoute";

// MARK: - Mode constants

pub const AVAudioSessionModeDefault: &str = "AVAudioSessionModeDefault";
pub const AVAudioSessionModeVoiceChat: &str = "AVAudioSessionModeVoiceChat";
pub const AVAudioSessionModeGameChat: &str = "AVAudioSessionModeGameChat";
pub const AVAudioSessionModeVideoRecording: &str = "AVAudioSessionModeVideoRecording";
pub const AVAudioSessionModeMeasurement: &str = "AVAudioSessionModeMeasurement";
pub const AVAudioSessionModeMoviePlayback: &str = "AVAudioSessionModeMoviePlayback";
pub const AVAudioSessionModeVideoChat: &str = "AVAudioSessionModeVideoChat";
pub const AVAudioSessionModeSpokenAudio: &str = "AVAudioSessionModeSpokenAudio";

// MARK: - Notification constants

pub const AVAudioSessionInterruptionNotification: &str = "AVAudioSessionInterruptionNotification";
pub const AVAudioSessionRouteChangeNotification: &str = "AVAudioSessionRouteChangeNotification";
pub const AVAudioSessionMediaServicesWereLostNotification: &str =
    "AVAudioSessionMediaServicesWereLostNotification";
pub const AVAudioSessionMediaServicesWereResetNotification: &str =
    "AVAudioSessionMediaServicesWereResetNotification";
pub const AVAudioSessionSilenceSecondaryAudioHintNotification: &str =
    "AVAudioSessionSilenceSecondaryAudioHintNotification";

// MARK: - UserInfo keys

pub const AVAudioSessionInterruptionTypeKey: &str = "AVAudioSessionInterruptionTypeKey";
pub const AVAudioSessionInterruptionOptionKey: &str = "AVAudioSessionInterruptionOptionKey";
pub const AVAudioSessionRouteChangeReasonKey: &str = "AVAudioSessionRouteChangeReasonKey";
pub const AVAudioSessionRouteChangePreviousRouteKey: &str =
    "AVAudioSessionRouteChangePreviousRouteKey";
pub const AVAudioSessionSilenceSecondaryAudioHintTypeKey: &str =
    "AVAudioSessionSilenceSecondaryAudioHintTypeKey";

// MARK: - Category option flags

type AVAudioSessionCategoryOptions = NSUInteger;
const AVAudioSessionCategoryOptionMixWithOthers: AVAudioSessionCategoryOptions = 0x1;
const AVAudioSessionCategoryOptionDuckOthers: AVAudioSessionCategoryOptions = 0x2;
const AVAudioSessionCategoryOptionAllowBluetooth: AVAudioSessionCategoryOptions = 0x4;
const AVAudioSessionCategoryOptionDefaultToSpeaker: AVAudioSessionCategoryOptions = 0x8;
const AVAudioSessionCategoryOptionInterruptSpokenAudioAndMixWithOthers:
    AVAudioSessionCategoryOptions = 0x11;
const AVAudioSessionCategoryOptionAllowBluetoothA2DP: AVAudioSessionCategoryOptions = 0x20;
const AVAudioSessionCategoryOptionAllowAirPlay: AVAudioSessionCategoryOptions = 0x40;

// MARK: - Interruption type

type AVAudioSessionInterruptionType = NSUInteger;
const AVAudioSessionInterruptionTypeBegan: AVAudioSessionInterruptionType = 1;
const AVAudioSessionInterruptionTypeEnded: AVAudioSessionInterruptionType = 0;

// MARK: - Route change reason

type AVAudioSessionRouteChangeReason = NSUInteger;
const AVAudioSessionRouteChangeReasonUnknown: AVAudioSessionRouteChangeReason = 0;
const AVAudioSessionRouteChangeReasonNewDeviceAvailable: AVAudioSessionRouteChangeReason = 1;
const AVAudioSessionRouteChangeReasonOldDeviceUnavailable: AVAudioSessionRouteChangeReason = 2;
const AVAudioSessionRouteChangeReasonCategoryChange: AVAudioSessionRouteChangeReason = 3;
const AVAudioSessionRouteChangeReasonOverride: AVAudioSessionRouteChangeReason = 4;
const AVAudioSessionRouteChangeReasonWakeFromSleep: AVAudioSessionRouteChangeReason = 6;
const AVAudioSessionRouteChangeReasonNoSuitableRouteForCategory: AVAudioSessionRouteChangeReason =
    7;
const AVAudioSessionRouteChangeReasonRouteConfigurationChange: AVAudioSessionRouteChangeReason = 8;

// MARK: - Port types

// Per Apple's
// <https://developer.apple.com/documentation/avfaudio/avaudiosessionport>,
// each `AVAudioSessionPort*` constant is a `NSString *` whose literal value
// is the bare port-type identifier ("Speaker", "Headphones", ...). Apps use
// `[portDescription.portType isEqualToString:AVAudioSessionPortHeadphones]`,
// so the only requirement is that each constant resolves to a distinct,
// non-NULL NSString with the correct iOS-canonical spelling.
pub const AVAudioSessionPortBuiltInSpeaker: &str = "Speaker";
pub const AVAudioSessionPortBuiltInReceiver: &str = "Receiver";
pub const AVAudioSessionPortBuiltInMic: &str = "MicrophoneBuiltIn";
pub const AVAudioSessionPortHeadphones: &str = "Headphones";
pub const AVAudioSessionPortHeadsetMic: &str = "MicrophoneWired";
pub const AVAudioSessionPortLineIn: &str = "LineIn";
pub const AVAudioSessionPortLineOut: &str = "LineOut";
pub const AVAudioSessionPortBluetoothA2DP: &str = "BluetoothA2DPOutput";
pub const AVAudioSessionPortBluetoothLE: &str = "BluetoothLEOutput";
pub const AVAudioSessionPortBluetoothHFP: &str = "BluetoothHFP";
pub const AVAudioSessionPortUSBAudio: &str = "USBAudio";
pub const AVAudioSessionPortHDMI: &str = "HDMI";
pub const AVAudioSessionPortAirPlay: &str = "AirPlay";
pub const AVAudioSessionPortCarAudio: &str = "CarAudio";
pub const AVAudioSessionPortAVB: &str = "AVB";
pub const AVAudioSessionPortDisplayPort: &str = "DisplayPort";
pub const AVAudioSessionPortFireWire: &str = "FireWire";
pub const AVAudioSessionPortPCI: &str = "PCI";
pub const AVAudioSessionPortThunderbolt: &str = "Thunderbolt";
pub const AVAudioSessionPortVirtual: &str = "Virtual";
pub const AVAudioSessionPortContinuityMicrophone: &str = "ContinuityMicrophone";

pub const CONSTANTS: ConstantExports = &[
    // Categories
    (
        "_AVAudioSessionCategoryAmbient",
        HostConstant::NSString(AVAudioSessionCategoryAmbient),
    ),
    (
        "_AVAudioSessionCategorySoloAmbient",
        HostConstant::NSString(AVAudioSessionCategorySoloAmbient),
    ),
    (
        "_AVAudioSessionCategoryPlayback",
        HostConstant::NSString(AVAudioSessionCategoryPlayback),
    ),
    (
        "_AVAudioSessionCategoryRecord",
        HostConstant::NSString(AVAudioSessionCategoryRecord),
    ),
    (
        "_AVAudioSessionCategoryPlayAndRecord",
        HostConstant::NSString(AVAudioSessionCategoryPlayAndRecord),
    ),
    (
        "_AVAudioSessionCategoryAudioProcessing",
        HostConstant::NSString(AVAudioSessionCategoryAudioProcessing),
    ),
    (
        "_AVAudioSessionCategoryMultiRoute",
        HostConstant::NSString(AVAudioSessionCategoryMultiRoute),
    ),
    // Modes
    (
        "_AVAudioSessionModeDefault",
        HostConstant::NSString(AVAudioSessionModeDefault),
    ),
    (
        "_AVAudioSessionModeVoiceChat",
        HostConstant::NSString(AVAudioSessionModeVoiceChat),
    ),
    (
        "_AVAudioSessionModeGameChat",
        HostConstant::NSString(AVAudioSessionModeGameChat),
    ),
    (
        "_AVAudioSessionModeVideoRecording",
        HostConstant::NSString(AVAudioSessionModeVideoRecording),
    ),
    (
        "_AVAudioSessionModeMeasurement",
        HostConstant::NSString(AVAudioSessionModeMeasurement),
    ),
    (
        "_AVAudioSessionModeMoviePlayback",
        HostConstant::NSString(AVAudioSessionModeMoviePlayback),
    ),
    (
        "_AVAudioSessionModeVideoChat",
        HostConstant::NSString(AVAudioSessionModeVideoChat),
    ),
    (
        "_AVAudioSessionModeSpokenAudio",
        HostConstant::NSString(AVAudioSessionModeSpokenAudio),
    ),
    // Notifications
    (
        "_AVAudioSessionInterruptionNotification",
        HostConstant::NSString(AVAudioSessionInterruptionNotification),
    ),
    (
        "_AVAudioSessionRouteChangeNotification",
        HostConstant::NSString(AVAudioSessionRouteChangeNotification),
    ),
    (
        "_AVAudioSessionMediaServicesWereLostNotification",
        HostConstant::NSString(AVAudioSessionMediaServicesWereLostNotification),
    ),
    (
        "_AVAudioSessionMediaServicesWereResetNotification",
        HostConstant::NSString(AVAudioSessionMediaServicesWereResetNotification),
    ),
    (
        "_AVAudioSessionSilenceSecondaryAudioHintNotification",
        HostConstant::NSString(AVAudioSessionSilenceSecondaryAudioHintNotification),
    ),
    // UserInfo keys
    (
        "_AVAudioSessionInterruptionTypeKey",
        HostConstant::NSString(AVAudioSessionInterruptionTypeKey),
    ),
    (
        "_AVAudioSessionInterruptionOptionKey",
        HostConstant::NSString(AVAudioSessionInterruptionOptionKey),
    ),
    (
        "_AVAudioSessionRouteChangeReasonKey",
        HostConstant::NSString(AVAudioSessionRouteChangeReasonKey),
    ),
    (
        "_AVAudioSessionRouteChangePreviousRouteKey",
        HostConstant::NSString(AVAudioSessionRouteChangePreviousRouteKey),
    ),
    (
        "_AVAudioSessionSilenceSecondaryAudioHintTypeKey",
        HostConstant::NSString(AVAudioSessionSilenceSecondaryAudioHintTypeKey),
    ),
    // Port types
    (
        "_AVAudioSessionPortBuiltInSpeaker",
        HostConstant::NSString(AVAudioSessionPortBuiltInSpeaker),
    ),
    (
        "_AVAudioSessionPortBuiltInReceiver",
        HostConstant::NSString(AVAudioSessionPortBuiltInReceiver),
    ),
    (
        "_AVAudioSessionPortBuiltInMic",
        HostConstant::NSString(AVAudioSessionPortBuiltInMic),
    ),
    (
        "_AVAudioSessionPortHeadphones",
        HostConstant::NSString(AVAudioSessionPortHeadphones),
    ),
    (
        "_AVAudioSessionPortBluetoothA2DP",
        HostConstant::NSString(AVAudioSessionPortBluetoothA2DP),
    ),
    (
        "_AVAudioSessionPortBluetoothLE",
        HostConstant::NSString(AVAudioSessionPortBluetoothLE),
    ),
    (
        "_AVAudioSessionPortBluetoothHFP",
        HostConstant::NSString(AVAudioSessionPortBluetoothHFP),
    ),
    (
        "_AVAudioSessionPortHeadsetMic",
        HostConstant::NSString(AVAudioSessionPortHeadsetMic),
    ),
    (
        "_AVAudioSessionPortLineIn",
        HostConstant::NSString(AVAudioSessionPortLineIn),
    ),
    (
        "_AVAudioSessionPortLineOut",
        HostConstant::NSString(AVAudioSessionPortLineOut),
    ),
    (
        "_AVAudioSessionPortUSBAudio",
        HostConstant::NSString(AVAudioSessionPortUSBAudio),
    ),
    (
        "_AVAudioSessionPortHDMI",
        HostConstant::NSString(AVAudioSessionPortHDMI),
    ),
    (
        "_AVAudioSessionPortAirPlay",
        HostConstant::NSString(AVAudioSessionPortAirPlay),
    ),
    (
        "_AVAudioSessionPortCarAudio",
        HostConstant::NSString(AVAudioSessionPortCarAudio),
    ),
    (
        "_AVAudioSessionPortAVB",
        HostConstant::NSString(AVAudioSessionPortAVB),
    ),
    (
        "_AVAudioSessionPortDisplayPort",
        HostConstant::NSString(AVAudioSessionPortDisplayPort),
    ),
    (
        "_AVAudioSessionPortFireWire",
        HostConstant::NSString(AVAudioSessionPortFireWire),
    ),
    (
        "_AVAudioSessionPortPCI",
        HostConstant::NSString(AVAudioSessionPortPCI),
    ),
    (
        "_AVAudioSessionPortThunderbolt",
        HostConstant::NSString(AVAudioSessionPortThunderbolt),
    ),
    (
        "_AVAudioSessionPortVirtual",
        HostConstant::NSString(AVAudioSessionPortVirtual),
    ),
    (
        "_AVAudioSessionPortContinuityMicrophone",
        HostConstant::NSString(AVAudioSessionPortContinuityMicrophone),
    ),
];

#[derive(Default)]
pub struct State {
    shared_instance: Option<id>,
}

// ДОБАВЛЕНО: Полноценный объект состояния (без заглушек)
#[derive(Default)]
pub(super) struct AVAudioSessionHostObject {
    category: id,
    category_options: AVAudioSessionCategoryOptions,
    mode: id,
    active: bool,
    preferred_sample_rate: f64,
    preferred_io_buffer_duration: f64,
    delegate: id,
}
impl HostObject for AVAudioSessionHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation AVAudioSession: NSObject

+ (id)sharedInstance {
    // ИСПРАВЛЕНО: Теперь объект действительно работает как синглтон и сохраняет
    // свое состояние
    if let Some(instance) = env.framework_state.avfoundation.av_audio_session.shared_instance {
        return instance;
    }

    let category = ns_string::get_static_str(env, AVAudioSessionCategorySoloAmbient);
    let mode = ns_string::get_static_str(env, AVAudioSessionModeDefault);

    let host_object = Box::new(AVAudioSessionHostObject {
        category,
        category_options: 0,
        mode,
        active: false,
        preferred_sample_rate: 44100.0,
        preferred_io_buffer_duration: 0.005,
        delegate: nil,
    });

    let instance = env.objc.alloc_static_object(
        this,
        host_object,
        &mut env.mem,
    );

    env.framework_state.avfoundation.av_audio_session.shared_instance = Some(instance);
    instance
}


// MARK: - Category

- (id)category {
    env.objc.borrow::<AVAudioSessionHostObject>(this).category
}

- (bool)setCategory:(id)category error:(MutPtr<id>)_error {
    let old = env.objc.borrow::<AVAudioSessionHostObject>(this).category;
    retain(env, category);
    release(env, old);
    env.objc.borrow_mut::<AVAudioSessionHostObject>(this).category = category;
    true
}

- (bool)setCategory:(id)category
        withOptions:(AVAudioSessionCategoryOptions)options
              error:(MutPtr<id>)_error {
    let old = env.objc.borrow::<AVAudioSessionHostObject>(this).category;
    retain(env, category);
    release(env, old);

    let host = env.objc.borrow_mut::<AVAudioSessionHostObject>(this);
    host.category = category;
    host.category_options = options;
    true
}

- (bool)setCategory:(id)category
               mode:(id)mode
            options:(AVAudioSessionCategoryOptions)options
              error:(MutPtr<id>)_error {
    let old_category = env.objc.borrow::<AVAudioSessionHostObject>(this).category;
    let old_mode = env.objc.borrow::<AVAudioSessionHostObject>(this).mode;

    retain(env, category);
    retain(env, mode);

    let host = env.objc.borrow_mut::<AVAudioSessionHostObject>(this);
    host.category = category;
    host.mode = mode;
    host.category_options = options;

    release(env, old_category);
    release(env, old_mode);
    true
}

- (AVAudioSessionCategoryOptions)categoryOptions {
    env.objc.borrow::<AVAudioSessionHostObject>(this).category_options
}

// MARK: - Mode

- (id)mode {
    env.objc.borrow::<AVAudioSessionHostObject>(this).mode
}

- (bool)setMode:(id)mode error:(MutPtr<id>)_error {
    let old = env.objc.borrow::<AVAudioSessionHostObject>(this).mode;
    retain(env, mode);
    release(env, old);
    env.objc.borrow_mut::<AVAudioSessionHostObject>(this).mode = mode;
    true
}

// MARK: - Activation

- (bool)setActive:(bool)active error:(MutPtr<id>)_error {
    env.objc.borrow_mut::<AVAudioSessionHostObject>(this).active = active;
    true
}

- (bool)setActive:(bool)active
      withOptions:(NSUInteger)_options
            error:(MutPtr<id>)_error {
    env.objc.borrow_mut::<AVAudioSessionHostObject>(this).active = active;
    true
}

// MARK: - Audio properties

- (f64)sampleRate {
    env.objc.borrow::<AVAudioSessionHostObject>(this).preferred_sample_rate
}

- (bool)setPreferredSampleRate:(f64)rate error:(MutPtr<id>)_error {
    env.objc.borrow_mut::<AVAudioSessionHostObject>(this).preferred_sample_rate = rate;
    true
}

- (f64)preferredSampleRate {
    env.objc.borrow::<AVAudioSessionHostObject>(this).preferred_sample_rate
}

- (f64)currentHardwareSampleRate {
    msg![env; this sampleRate]
}

- (f64)preferredHardwareSampleRate {
    msg![env; this preferredSampleRate]
}

- (bool)setPreferredHardwareSampleRate:(f64)rate error:(MutPtr<id>)error {
    msg![env; this setPreferredSampleRate:rate error:error]
}

- (f32)currentHardwareOutputVolume {
    msg![env; this outputVolume]
}

- (f64)currentHardwareInputLatency {
    msg![env; this inputLatency]
}

- (f64)currentHardwareOutputLatency {
    msg![env; this outputLatency]
}

- (NSInteger)currentHardwareInputNumberOfChannels {
    msg![env; this maximumInputNumberOfChannels]
}

- (NSInteger)currentHardwareOutputNumberOfChannels {
    msg![env; this maximumOutputNumberOfChannels]
}

- (f64)outputLatency {
    0.005 // 5ms — typical for built-in speaker
}

- (f64)inputLatency {
    0.005
}

- (f64)IOBufferDuration {
    env.objc.borrow::<AVAudioSessionHostObject>(this).preferred_io_buffer_duration
}

- (bool)setPreferredIOBufferDuration:(f64)duration error:(MutPtr<id>)_error {
    env.objc.borrow_mut::<AVAudioSessionHostObject>(this).preferred_io_buffer_duration = duration;
    true
}

- (f64)preferredIOBufferDuration {
    env.objc.borrow::<AVAudioSessionHostObject>(this).preferred_io_buffer_duration
}

- (NSInteger)maximumInputNumberOfChannels {
    0
}

- (NSInteger)maximumOutputNumberOfChannels {
    2
}

- (f32)inputGain {
    1.0
}

- (bool)isInputGainSettable {
    false
}

- (bool)setInputGain:(f32)_gain error:(MutPtr<id>)_error {
    false
}

- (f32)outputVolume {
    1.0
}

// MARK: - Input / output availability

- (bool)isInputAvailable {
    false
}

- (bool)isOtherAudioPlaying {
    false
}

- (bool)secondaryAudioShouldBeSilencedHint {
    false
}

// MARK: - Route

- (id)currentRoute { // AVAudioSessionRouteDescription*
    // Return a stub route description.
    let route: id = msg_class![env; AVAudioSessionRouteDescription new];
    autorelease(env, route)
}

- (id)availableInputs { // NSArray<AVAudioSessionPortDescription*>*
    msg_class![env; NSArray new]
}

- (bool)setPreferredInput:(id)_input error:(MutPtr<id>)_error {
    false
}

- (id)preferredInput {
    nil
}

// MARK: - Delegate (deprecated pre-iOS 6 API)

- (id)delegate {
    env.objc.borrow::<AVAudioSessionHostObject>(this).delegate
}

- (())setDelegate:(id)delegate {
    // В Objective-C делегаты обычно хранятся как weak ссылки (без
    // retain/release)
    env.objc.borrow_mut::<AVAudioSessionHostObject>(this).delegate = delegate;
}

// MARK: - Interruption observer (deprecated pre-iOS 6 API)

- (bool)setActive:(bool)active
            flags:(NSInteger)_flags
            error:(MutPtr<id>)_error {
    env.objc.borrow_mut::<AVAudioSessionHostObject>(this).active = active;
    true
}

@end

// MARK: - AVAudioSessionRouteDescription stub

@implementation AVAudioSessionRouteDescription: NSObject

- (id)inputs {
    msg_class![env; NSArray new]
}

- (id)outputs {
    // Report built-in speaker as the sole output.
    let port: id = msg_class![env; AVAudioSessionPortDescription new];
    let arr: id  = msg_class![env; NSArray arrayWithObject:port];
    release(env, port);
    arr
}

@end

// MARK: - AVAudioSessionPortDescription stub

@implementation AVAudioSessionPortDescription: NSObject

- (id)portType {
    ns_string::get_static_str(env, AVAudioSessionPortBuiltInSpeaker)
}

- (id)portName {
    ns_string::get_static_str(env, "Speaker")
}

- (id)UID {
    ns_string::get_static_str(env, "Built-In Speaker")
}

- (id)channels { // NSArray<AVAudioSessionChannelDescription*>*
    msg_class![env; NSArray new]
}

- (id)dataSources {
    nil
}

- (id)selectedDataSource {
    nil
}

- (id)preferredDataSource {
    nil
}

- (bool)setPreferredDataSource:(id)_source error:(MutPtr<id>)_error {
    false
}

@end

};
