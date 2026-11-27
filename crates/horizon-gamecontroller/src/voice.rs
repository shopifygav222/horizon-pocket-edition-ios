//! The microphone on iPhone and iPad, for voice chat: the permission, the audio session's
//! switch between playing and playing-and-recording, where sound goes and comes from, what
//! the system does to the session behind the game's back, and the capture itself.
//!
//! Elsewhere everything here answers "nothing" (the client's own capture, cpal's, is used
//! there), so the game calls it without asking which device it runs on.
//!
//! How the session is set while the microphone is live:
//!
//! - Play and record, defaulting to the speaker (not the earpiece), with Bluetooth
//!   headphones kept as headphones (A2DP): the game and the others' voices stay in full
//!   quality there, and the iPhone's own microphone records. Only when the player chooses the
//!   headset's microphone is the hands-free profile allowed, which lowers both.
//! - Other apps' sound plays on (mix with others): a player's own music is not stopped by the
//!   microphone going live, as it is not stopped by the game (`audio_session`).
//! - Off the microphone, the session goes back to what the game has without it (ambient: the
//!   ring switch silences it, other apps play on), so listening alone changes nothing about
//!   the phone's sound.

/// Whether the player lets the game use the microphone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    /// Not asked yet: [`request_mic_permission`] asks.
    Unknown,
    Denied,
    Granted,
}

/// Where sound comes out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// The device's own loudspeaker: the microphone hears it.
    Speaker,
    /// The earpiece.
    Receiver,
    /// Wired headphones, or a USB or Lightning set.
    Headphones,
    Bluetooth,
    AirPlay,
    Other,
}

impl Output {
    /// The microphone hears what comes out of it.
    pub fn open_air(self) -> bool {
        matches!(self, Self::Speaker | Self::Receiver | Self::Other)
    }

    /// Something worn or plugged in, that can be taken off or out.
    pub fn worn(self) -> bool {
        matches!(self, Self::Headphones | Self::Bluetooth)
    }
}

/// A microphone's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    /// The device's own.
    BuiltIn,
    /// A wired or USB headset's.
    Wired,
    /// A Bluetooth headset's (the hands-free profile).
    Bluetooth,
    Other,
}

/// A microphone the session can record from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub kind: InputKind,
    /// As the system names it ("iPhone Microphone", "AirPods Pro").
    pub name: String,
}

/// The session's route now.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub output: Output,
    pub output_name: String,
    /// The microphone in use, when the session records.
    pub input: Option<Input>,
    /// The hardware's sample rate.
    pub rate: f64,
}

/// Which microphone the player wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Choice {
    /// The system's: a wired headset's when one is plugged in, else the device's own (never
    /// a Bluetooth headset's, which would lower everything heard).
    #[default]
    Automatic,
    BuiltIn,
    Wired,
    Bluetooth,
}

/// What the system did to the session since the last look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A call, Siri, an alarm took the session: nothing records or plays until it ends.
    Interrupted,
    /// The interruption ended.
    Resumed,
    /// Headphones were unplugged or went out of reach: sound comes out somewhere else now.
    OutputLost,
    /// The route changed some other way (headphones plugged in, a category change settled).
    RouteChanged,
    /// The system's audio services restarted: every stream is dead and must be made again.
    Reset,
}

/// The microphone's samples as the device gives them: interleaved, with their channel count
/// and rate. Called on the system's audio thread: it must only copy.
pub type Samples = Box<dyn FnMut(&[f32], usize, u32) + Send>;

/// The permission as it stands.
pub fn mic_permission() -> Permission {
    #[cfg(target_os = "ios")]
    return ios::mic_permission();
    #[cfg(not(target_os = "ios"))]
    Permission::Granted
}

/// Asks the player for the microphone (the system's own prompt, once ever); the answer shows
/// in [`mic_permission`].
pub fn request_mic_permission() {
    #[cfg(target_os = "ios")]
    ios::request_mic_permission();
}

/// The route now. None where there is no session to ask (anything but iOS).
pub fn route() -> Option<Route> {
    #[cfg(target_os = "ios")]
    return ios::route();
    #[cfg(not(target_os = "ios"))]
    None
}

/// The microphones that can be chosen now, while the session records.
pub fn inputs() -> Vec<Input> {
    #[cfg(target_os = "ios")]
    return ios::inputs();
    #[cfg(not(target_os = "ios"))]
    Vec::new()
}

/// Makes the session play and record, from the microphone `choice` asks for.
pub fn begin_recording(choice: Choice) -> Result<(), String> {
    #[cfg(target_os = "ios")]
    return ios::begin_recording(choice);
    #[cfg(not(target_os = "ios"))]
    {
        let _ = choice;
        Err("no audio session on this platform".into())
    }
}

/// Puts the session back as the game always had it: playing only.
pub fn end_recording() {
    #[cfg(target_os = "ios")]
    ios::end_recording();
}

/// What the system did to the session since the last call, in order. The first call starts
/// listening for it.
pub fn events() -> Vec<Event> {
    #[cfg(target_os = "ios")]
    return ios::events();
    #[cfg(not(target_os = "ios"))]
    Vec::new()
}

/// A running capture: dropping it stops the microphone.
pub struct Capture {
    #[cfg(target_os = "ios")]
    unit: coreaudio::audio_unit::AudioUnit,
    /// The system's echo canceller, noise suppression and levelling are in it.
    pub voice_processing: bool,
}

impl Drop for Capture {
    fn drop(&mut self) {
        #[cfg(target_os = "ios")]
        {
            let _ = self.unit.stop();
        }
    }
}

/// Starts the microphone ([`begin_recording`] first). With `voice_processing` the system's
/// echo canceller keeps what the speaker plays out of it.
pub fn start_capture(voice_processing: bool, samples: Samples) -> Result<Capture, String> {
    #[cfg(target_os = "ios")]
    return ios::start_capture(voice_processing, samples);
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (voice_processing, samples);
        Err("the client's own capture is used on this platform".into())
    }
}

/// Goes once through everything above and says how each step went, for the checks in the
/// iOS Simulator (`tools/ci/ios_simulator_smoke.sh`): the calls into the system must at
/// least be ones it knows, whatever a machine with no microphone answers.
pub fn selftest() -> Vec<String> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let mut lines = vec![
        format!("permission {:?}", mic_permission()),
        format!("route {:?}", route()),
    ];
    let _ = events();
    match begin_recording(Choice::Automatic) {
        Ok(()) => lines.push("recording".into()),
        Err(e) => lines.push(format!("not recording: {e}")),
    }
    lines.push(format!("inputs {:?}", inputs()));
    lines.push(format!("route while recording {:?}", route()));
    for system in [false, true] {
        let heard = Arc::new(AtomicUsize::new(0));
        let count = heard.clone();
        let capture = start_capture(
            system,
            Box::new(move |samples, _, _| {
                count.fetch_add(samples.len(), Ordering::Relaxed);
            }),
        );
        match capture {
            Ok(capture) => {
                std::thread::sleep(std::time::Duration::from_millis(300));
                drop(capture);
                lines.push(format!(
                    "capture (voice processing {system}): {} samples",
                    heard.load(Ordering::Relaxed)
                ));
            }
            Err(e) => lines.push(format!("capture (voice processing {system}) failed: {e}")),
        }
    }
    end_recording();
    lines.push(format!("events {:?}", events()));
    lines.push("done".into());
    lines
}

#[cfg(target_os = "ios")]
mod ios {
    use super::{Capture, Choice, Event, Input, InputKind, Output, Permission, Route, Samples};
    use coreaudio::audio_unit::{
        AudioUnit, Element, IOType, SampleFormat, Scope, StreamFormat,
        audio_format::LinearPcmFlags,
        render_callback::{self, data},
    };
    use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
    use objc2_foundation::NSString;
    use std::{
        ptr::NonNull,
        sync::{Mutex, Once},
    };

    // AVAudioSession lives in AVFAudio.
    #[link(name = "AVFAudio", kind = "framework")]
    unsafe extern "C" {}

    // AVAudioSessionCategoryOptions.
    const MIX_WITH_OTHERS: usize = 0x1;
    const ALLOW_BLUETOOTH: usize = 0x4;
    const DEFAULT_TO_SPEAKER: usize = 0x8;
    const ALLOW_BLUETOOTH_A2DP: usize = 0x20;
    // AVAudioSessionRecordPermission ('undt', 'deny', 'grnt').
    const DENIED: usize = 0x6465_6e79;
    const GRANTED: usize = 0x6772_6e74;
    // AVAudioSessionInterruptionType, AVAudioSessionRouteChangeReason.
    const INTERRUPTION_BEGAN: usize = 1;
    const OLD_DEVICE_UNAVAILABLE: usize = 2;
    // kAudioOutputUnitProperty_EnableIO, kAudioUnitProperty_StreamFormat,
    // kAUVoiceIOProperty_OtherAudioDuckingConfiguration.
    const ENABLE_IO: u32 = 2003;
    const STREAM_FORMAT: u32 = 8;
    const OTHER_AUDIO_DUCKING: u32 = 2108;

    fn session() -> Option<Retained<AnyObject>> {
        // SAFETY: a class method returning the shared session; any thread may call it.
        unsafe { msg_send![class!(AVAudioSession), sharedInstance] }
    }

    fn text(string: Option<Retained<NSString>>) -> String {
        string.map(|s| s.to_string()).unwrap_or_default()
    }

    /// The items of an NSArray.
    fn items(array: Option<Retained<AnyObject>>) -> Vec<Retained<AnyObject>> {
        let Some(array) = array else {
            return Vec::new();
        };
        // SAFETY: `count` and `objectAtIndex:` on an NSArray, within its bounds.
        unsafe {
            let count: usize = msg_send![&*array, count];
            (0..count)
                .filter_map(|i| msg_send![&*array, objectAtIndex: i])
                .collect()
        }
    }

    pub fn mic_permission() -> Permission {
        let Some(session) = session() else {
            return Permission::Unknown;
        };
        // SAFETY: a property of the shared session.
        let permission: usize = unsafe { msg_send![&*session, recordPermission] };
        match permission {
            GRANTED => Permission::Granted,
            DENIED => Permission::Denied,
            _ => Permission::Unknown,
        }
    }

    pub fn request_mic_permission() {
        let Some(session) = session() else {
            return;
        };
        // The answer is read from `recordPermission` when the game next looks.
        let answered = block2::RcBlock::new(|granted: objc2::runtime::Bool| {
            log::info!(
                "Horizon: the microphone is {}",
                if granted.as_bool() {
                    "allowed"
                } else {
                    "refused"
                }
            );
        });
        // SAFETY: the session copies the block and calls it once, on any thread.
        let _: () = unsafe { msg_send![&*session, requestRecordPermission: &*answered] };
    }

    fn output_of(kind: &str) -> Output {
        match kind {
            "Speaker" => Output::Speaker,
            "Receiver" => Output::Receiver,
            "Headphones" | "USBAudio" | "LineOut" | "HDMIOutput" => Output::Headphones,
            "BluetoothA2DPOutput" | "BluetoothHFP" | "BluetoothLE" => Output::Bluetooth,
            "AirPlay" | "CarAudio" => Output::AirPlay,
            _ => Output::Other,
        }
    }

    fn input_kind(kind: &str) -> InputKind {
        match kind {
            "MicrophoneBuiltIn" => InputKind::BuiltIn,
            "MicrophoneWired" | "USBAudio" | "LineIn" => InputKind::Wired,
            "BluetoothHFP" | "BluetoothLE" => InputKind::Bluetooth,
            _ => InputKind::Other,
        }
    }

    /// A port's type and name.
    fn port(port: &AnyObject) -> (String, String) {
        // SAFETY: properties of an AVAudioSessionPortDescription, both strings.
        unsafe {
            (
                text(msg_send![port, portType]),
                text(msg_send![port, portName]),
            )
        }
    }

    pub fn route() -> Option<Route> {
        let session = session()?;
        // SAFETY: properties of the shared session and of its route description.
        let (outputs, inputs, rate) = unsafe {
            let route: Option<Retained<AnyObject>> = msg_send![&*session, currentRoute];
            let route = route?;
            let outputs: Option<Retained<AnyObject>> = msg_send![&*route, outputs];
            let inputs: Option<Retained<AnyObject>> = msg_send![&*route, inputs];
            let rate: f64 = msg_send![&*session, sampleRate];
            (items(outputs), items(inputs), rate)
        };
        // With several outputs (never, in practice) the first is where sound goes.
        let (kind, output_name) = outputs.first().map(|p| port(p)).unwrap_or_default();
        let input = inputs.first().map(|p| {
            let (kind, name) = port(p);
            Input {
                kind: input_kind(&kind),
                name,
            }
        });
        Some(Route {
            output: if outputs.is_empty() {
                Output::Other
            } else {
                output_of(&kind)
            },
            output_name,
            input,
            rate,
        })
    }

    fn available() -> Vec<Retained<AnyObject>> {
        let Some(session) = session() else {
            return Vec::new();
        };
        // SAFETY: a property of the shared session, an array of port descriptions or nil.
        items(unsafe { msg_send![&*session, availableInputs] })
    }

    pub fn inputs() -> Vec<Input> {
        available()
            .iter()
            .map(|p| {
                let (kind, name) = port(p);
                Input {
                    kind: input_kind(&kind),
                    name,
                }
            })
            .collect()
    }

    /// Sets the session's category, mode and options; false when the system refuses.
    fn set_category(session: &AnyObject, category: &str, options: usize) -> bool {
        let category = NSString::from_str(category);
        let mode = NSString::from_str("AVAudioSessionModeDefault");
        // SAFETY: `setCategory:mode:options:error:` with two strings, the options' bits and
        // no error wanted (the BOOL says enough).
        unsafe {
            msg_send![
                session,
                setCategory: &*category,
                mode: &*mode,
                options: options,
                error: std::ptr::null_mut::<*mut AnyObject>()
            ]
        }
    }

    pub fn begin_recording(choice: Choice) -> Result<(), String> {
        let session = session().ok_or("no audio session")?;
        // Bluetooth stays headphones (A2DP) unless its own microphone is asked for.
        let mut options = MIX_WITH_OTHERS | DEFAULT_TO_SPEAKER | ALLOW_BLUETOOTH_A2DP;
        if choice == Choice::Bluetooth {
            options |= ALLOW_BLUETOOTH;
        }
        if !set_category(&session, "AVAudioSessionCategoryPlayAndRecord", options) {
            return Err("the system refused to record".into());
        }
        // SAFETY: `setActive:error:`, no error wanted.
        let active: bool = unsafe {
            msg_send![
                &*session,
                setActive: true,
                error: std::ptr::null_mut::<*mut AnyObject>()
            ]
        };
        if !active {
            return Err("the audio session couldn't be taken (a call may be under way)".into());
        }
        // The microphone asked for, when it is there; otherwise (and for Automatic) the
        // system's own choice stands.
        let wanted = match choice {
            Choice::Automatic => None,
            Choice::BuiltIn => Some(InputKind::BuiltIn),
            Choice::Wired => Some(InputKind::Wired),
            Choice::Bluetooth => Some(InputKind::Bluetooth),
        };
        let preferred = wanted.and_then(|kind| {
            available()
                .into_iter()
                .find(|p| input_kind(&port(p).0) == kind)
        });
        // SAFETY: `setPreferredInput:error:` with a port of this session's or nil (which
        // clears an earlier preference), no error wanted.
        let _: bool = unsafe {
            msg_send![
                &*session,
                setPreferredInput: preferred.as_deref(),
                error: std::ptr::null_mut::<*mut AnyObject>()
            ]
        };
        Ok(())
    }

    pub fn end_recording() {
        if let Some(session) = session() {
            // What the game has without the microphone (`audio_session`): playing only,
            // silenced by the ring switch, beside other apps' sound.
            set_category(&session, "AVAudioSessionCategoryAmbient", 0);
        }
    }

    static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
    static LISTEN: Once = Once::new();

    fn push(event: Event) {
        let mut events = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
        // A system gone wild doesn't fill the memory: the newest are what matter.
        if events.len() >= 64 {
            events.remove(0);
        }
        events.push(event);
    }

    /// The number under `key` in a notification's user info.
    fn number(note: NonNull<AnyObject>, key: &str) -> Option<usize> {
        let key = NSString::from_str(key);
        // SAFETY: the notification is alive for the block's call; `userInfo` is a
        // dictionary or nil, and the value under these keys is an NSNumber or absent.
        unsafe {
            let info: Option<Retained<AnyObject>> = msg_send![note.as_ref(), userInfo];
            let value: Option<Retained<AnyObject>> = msg_send![&*info?, objectForKey: &*key];
            Some(msg_send![&*value?, unsignedIntegerValue])
        }
    }

    /// Starts hearing what the system does to the session.
    fn listen() {
        let observe = |name: &str, heard: fn(NonNull<AnyObject>)| {
            let name = NSString::from_str(name);
            let block = block2::RcBlock::new(move |note: NonNull<AnyObject>| heard(note));
            // SAFETY: the default notification centre copies the block and keeps it for as
            // long as the observer it returns lives; that observer is kept for the life of
            // the process (never removed), so it is leaked on purpose.
            unsafe {
                let centre: Option<Retained<AnyObject>> =
                    msg_send![class!(NSNotificationCenter), defaultCenter];
                let Some(centre) = centre else { return };
                let observer: Option<Retained<AnyObject>> = msg_send![
                    &*centre,
                    addObserverForName: &*name,
                    object: Option::<&AnyObject>::None,
                    queue: Option::<&AnyObject>::None,
                    usingBlock: &*block
                ];
                std::mem::forget(observer);
            }
        };
        observe("AVAudioSessionInterruptionNotification", |note| {
            let began = number(note, "AVAudioSessionInterruptionTypeKey");
            push(if began == Some(INTERRUPTION_BEGAN) {
                Event::Interrupted
            } else {
                Event::Resumed
            });
        });
        observe("AVAudioSessionRouteChangeNotification", |note| {
            let reason = number(note, "AVAudioSessionRouteChangeReasonKey");
            push(if reason == Some(OLD_DEVICE_UNAVAILABLE) {
                Event::OutputLost
            } else {
                Event::RouteChanged
            });
        });
        observe("AVAudioSessionMediaServicesWereResetNotification", |_| {
            push(Event::Reset);
        });
    }

    pub fn events() -> Vec<Event> {
        LISTEN.call_once(listen);
        std::mem::take(&mut *EVENTS.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// `AUVoiceIOOtherAudioDuckingConfiguration`: how far the system turns the game's own
    /// sound down while the voice unit runs.
    #[repr(C)]
    #[allow(dead_code)]
    struct Ducking {
        advanced: u8,
        level: u32,
    }

    pub fn start_capture(voice_processing: bool, mut samples: Samples) -> Result<Capture, String> {
        let failed = |what: &str, e: coreaudio::Error| format!("{what}: {e:?}");
        let rate = route().map_or(48_000.0, |r| r.rate);
        let rate = if rate.is_finite() && rate >= 8_000.0 {
            rate
        } else {
            48_000.0
        };
        let mut unit = AudioUnit::new(if voice_processing {
            IOType::VoiceProcessingIO
        } else {
            IOType::RemoteIO
        })
        .map_err(|e| failed("the microphone's unit", e))?;
        unit.uninitialize()
            .map_err(|e| failed("the microphone's unit", e))?;
        // The microphone on, the unit's own output off: the game plays through its own.
        unit.set_property(ENABLE_IO, Scope::Input, Element::Input, Some(&1u32))
            .map_err(|e| failed("turning the microphone on", e))?;
        unit.set_property(ENABLE_IO, Scope::Output, Element::Output, Some(&0u32))
            .map_err(|e| failed("turning the unit's output off", e))?;
        if voice_processing {
            // As little turning down of the game's own sound as the system allows (iOS 17
            // on; before, the property isn't there and the system's default stands).
            let least = Ducking {
                advanced: 0,
                level: 10,
            };
            let _ = unit.set_property(
                OTHER_AUDIO_DUCKING,
                Scope::Global,
                Element::Output,
                Some(&least),
            );
        }
        unit.initialize()
            .map_err(|e| failed("starting the microphone's unit", e))?;
        // One channel of floats at the hardware's rate: what the unit hands the callback.
        let format = StreamFormat {
            sample_rate: rate,
            sample_format: SampleFormat::F32,
            flags: LinearPcmFlags::IS_FLOAT | LinearPcmFlags::IS_PACKED,
            channels: 1,
        };
        unit.set_property(
            STREAM_FORMAT,
            Scope::Output,
            Element::Input,
            Some(&format.to_asbd()),
        )
        .map_err(|e| failed("the microphone's format", e))?;
        let hz = rate.round() as u32;
        type Args = render_callback::Args<data::Interleaved<f32>>;
        unit.set_input_callback(move |args: Args| {
            samples(args.data.buffer, args.data.channels.max(1), hz);
            Ok(())
        })
        .map_err(|e| failed("the microphone's callback", e))?;
        unit.start()
            .map_err(|e| failed("starting the microphone", e))?;
        Ok(Capture {
            unit,
            voice_processing,
        })
    }
}
