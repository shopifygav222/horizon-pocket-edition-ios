//! Horizon PE: the game's sound beside other apps', on iPhone and iPad.
//!
//! An app that never names its audio session's category gets "solo ambient": the moment it makes
//! a sound, the system stops whatever else was playing. That is why music from another app (a
//! player's own, a podcast) went silent when the game was opened. "Ambient" is the same in every
//! other way (the ring switch still silences the game, it plays nothing in the background) and
//! lets the other app play on, with the game's sound over it.
//!
//! How loud the two are is the player's: the game's own sliders (Options > Music & Sounds) set
//! the game's side, and the other app's volume is that app's and the phone's buttons'. One app
//! may not set another's volume.
//!
//! Elsewhere this answers "nothing": only an iPhone has a session to set.

/// Lets other apps' sound play on while the game does. Called once, before the game opens its
/// own sound. False where there is no session to set, or the system refused.
pub fn play_beside_others() -> bool {
    #[cfg(target_os = "ios")]
    {
        ios::play_beside_others()
    }
    #[cfg(not(target_os = "ios"))]
    {
        false
    }
}

#[cfg(target_os = "ios")]
mod ios {
    use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
    use objc2_foundation::NSString;

    // AVAudioSession lives in AVFAudio.
    #[link(name = "AVFAudio", kind = "framework")]
    unsafe extern "C" {}

    pub fn play_beside_others() -> bool {
        // SAFETY: a class method returning the shared session; any thread may call it.
        let session: Option<Retained<AnyObject>> =
            unsafe { msg_send![class!(AVAudioSession), sharedInstance] };
        let Some(session) = session else {
            return false;
        };
        let category = NSString::from_str("AVAudioSessionCategoryAmbient");
        // SAFETY: `setCategory:error:` with a category's name and no error wanted (the BOOL
        // says enough).
        unsafe {
            msg_send![
                &*session,
                setCategory: &*category,
                error: std::ptr::null_mut::<*mut AnyObject>()
            ]
        }
    }
}
