//! Звук компьютера на Mac: глобальный Core Audio process tap (все процессы, любое устройство
//! вывода) и частное агрегатное устройство поверх него, с которого cpal пишет как с микрофона.
//!
//! Tap, который cpal делает сам для устройства вывода, привязан к этому устройству: звук,
//! идущий мимо него, не попадает в запись. Так теряется Safari в звонке (Телемост и другие
//! встречи в браузере): с включённым эхоподавлением WebKit выводит звук через системное
//! голосовое агрегатное устройство, а не прямо в колонки или наушники. Глобальный tap
//! снимает звук процессов до того, как он разойдётся по устройствам.

use anyhow::{anyhow, bail, Result};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::AnyThread;
use objc2_core_audio::{
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceNameKey, kAudioAggregateDeviceTapAutoStartKey,
    kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey, kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey,
    AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice,
    AudioHardwareDestroyProcessTap, AudioObjectID, CATapDescription, CATapMuteBehavior,
};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use std::ffi::CStr;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Tap и агрегатное устройство; уничтожаются вместе с ним.
pub struct GlobalTap {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    /// Имя агрегатного устройства: по нему cpal находит его среди входов.
    pub name: String,
}

impl GlobalTap {
    pub fn new() -> Result<Self> {
        if AnyClass::get(c"CATapDescription").is_none() {
            bail!("CATapDescription is unavailable");
        }
        let id = format!("{}.{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
        let name = format!("Ultra Transcript system audio {id}");
        let desc = unsafe {
            let desc = CATapDescription::initStereoGlobalTapButExcludeProcesses(CATapDescription::alloc(), &NSArray::new());
            desc.setMuteBehavior(CATapMuteBehavior::Unmuted);
            desc.setPrivate(true);
            desc.setName(&NSString::from_str(&name));
            desc
        };
        let mut tap: AudioObjectID = 0;
        let status = unsafe { AudioHardwareCreateProcessTap(Some(&desc), &mut tap) };
        if status != 0 {
            bail!("AudioHardwareCreateProcessTap: {status}");
        }
        let uid = unsafe { desc.UUID().UUIDString() };

        let sub = dict(&[(kAudioSubTapUIDKey, obj(uid)), (kAudioSubTapDriftCompensationKey, obj(NSNumber::new_bool(true)))]);
        let taps = NSArray::from_retained_slice(&[sub]);
        let props = dict(&[
            (kAudioAggregateDeviceNameKey, obj(NSString::from_str(&name))),
            (kAudioAggregateDeviceUIDKey, obj(NSString::from_str(&format!("app.ultratranscript.systemaudio.{id}")))),
            (kAudioAggregateDeviceTapListKey, obj(taps)),
            (kAudioAggregateDeviceTapAutoStartKey, obj(NSNumber::new_bool(true))),
            (kAudioAggregateDeviceIsPrivateKey, obj(NSNumber::new_bool(true))),
        ]);
        // NSDictionary и CFDictionary — один и тот же объект (toll-free bridging).
        let cf = unsafe { &*(Retained::as_ptr(&props) as *const CFDictionary) };
        let mut aggregate: AudioObjectID = 0;
        let status = unsafe { AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut aggregate)) };
        if status != 0 {
            unsafe { AudioHardwareDestroyProcessTap(tap) };
            return Err(anyhow!("AudioHardwareCreateAggregateDevice: {status}"));
        }
        Ok(Self { tap, aggregate, name })
    }
}

impl Drop for GlobalTap {
    fn drop(&mut self) {
        unsafe {
            AudioHardwareDestroyAggregateDevice(self.aggregate);
            AudioHardwareDestroyProcessTap(self.tap);
        }
    }
}

fn obj<T: objc2::Message>(x: Retained<T>) -> Retained<AnyObject> {
    unsafe { Retained::cast_unchecked(x) }
}

fn dict(pairs: &[(&CStr, Retained<AnyObject>)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<Retained<NSString>> = pairs.iter().map(|(k, _)| NSString::from_str(k.to_str().unwrap_or_default())).collect();
    let keys: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    let values: Vec<&AnyObject> = pairs.iter().map(|(_, v)| &**v).collect();
    NSDictionary::from_slices(&keys, &values)
}

#[cfg(test)]
mod tests {
    /// Живой Core Audio: tap создаётся, а его агрегатное устройство открывается как вход.
    #[test]
    #[ignore]
    fn global_tap_shows_up_as_an_input() {
        use cpal::traits::DeviceTrait;
        let (_tap, dev) = crate::record::global_tap(&cpal::default_host()).unwrap();
        eprintln!("{:?}", dev.default_input_config().unwrap());
    }

    /// Звук, который играет другой процесс, приходит через tap.
    #[test]
    #[ignore]
    fn global_tap_hears_other_processes() {
        use cpal::traits::{DeviceTrait, StreamTrait};
        use std::sync::{Arc, Mutex};
        let (_tap, dev) = crate::record::global_tap(&cpal::default_host()).unwrap();
        let config = dev.default_input_config().unwrap().config();
        let peak = Arc::new(Mutex::new(0f32));
        let p = peak.clone();
        let stream = dev
            .build_input_stream(
                config,
                move |data: &[f32], _: &_| {
                    let mut p = p.lock().unwrap();
                    *p = data.iter().fold(*p, |m, x| m.max(x.abs()));
                },
                |e| eprintln!("{e}"),
                None,
            )
            .unwrap();
        stream.play().unwrap();
        std::process::Command::new("afplay").args(["-v", "0.1", "/System/Library/Sounds/Glass.aiff"]).status().unwrap();
        let peak = *peak.lock().unwrap();
        eprintln!("peak {peak}");
        assert!(peak > 0.001);
    }
}
