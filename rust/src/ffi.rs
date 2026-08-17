//! Helpers sobre a API de propriedades do Core Audio. Toda travessia de FFI passa por aqui,
//! para que o resto do código não repita o padrão de ponteiro e tamanho.

use coreaudio_sys::*;
use std::mem::{size_of, MaybeUninit};

pub fn property_address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

pub fn get_property<T>(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
) -> Result<T, OSStatus> {
    let mut value = MaybeUninit::<T>::uninit();
    let mut size = size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            address,
            0,
            std::ptr::null(),
            &mut size,
            value.as_mut_ptr() as *mut _,
        )
    };
    if status == 0 {
        Ok(unsafe { value.assume_init() })
    } else {
        Err(status)
    }
}

pub fn set_property<T>(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    value: &T,
) -> OSStatus {
    unsafe {
        AudioObjectSetPropertyData(
            object,
            address,
            0,
            std::ptr::null(),
            size_of::<T>() as u32,
            value as *const T as *const _,
        )
    }
}

pub fn get_property_array<T: Clone + Default>(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
) -> Result<Vec<T>, OSStatus> {
    let mut size: u32 = 0;
    let status =
        unsafe { AudioObjectGetPropertyDataSize(object, address, 0, std::ptr::null(), &mut size) };
    if status != 0 {
        return Err(status);
    }

    let count = size as usize / size_of::<T>();
    let mut out = vec![T::default(); count];
    if count == 0 {
        return Ok(out);
    }

    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            address,
            0,
            std::ptr::null(),
            &mut size,
            out.as_mut_ptr() as *mut _,
        )
    };
    if status == 0 {
        Ok(out)
    } else {
        Err(status)
    }
}

pub fn has_property(object: AudioObjectID, address: &AudioObjectPropertyAddress) -> bool {
    unsafe { AudioObjectHasProperty(object, address) != 0 }
}

pub fn is_property_settable(object: AudioObjectID, address: &AudioObjectPropertyAddress) -> bool {
    let mut settable: Boolean = 0;
    let status = unsafe { AudioObjectIsPropertySettable(object, address, &mut settable) };
    status == 0 && settable != 0
}

/// Formata um OSStatus como fourcc legível quando possível, senão como número.
pub fn os_status_text(status: OSStatus) -> String {
    let value = status as u32;
    let chars = [
        ((value >> 24) & 0xFF) as u8,
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
    ];
    if chars.iter().all(|c| c.is_ascii_graphic()) {
        format!("{} ({})", String::from_utf8_lossy(&chars), status)
    } else {
        format!("{status}")
    }
}

/// Converte uma CFStringRef para String e a libera.
pub fn cf_string_into_owned(reference: CFStringRef) -> String {
    if reference.is_null() {
        return String::new();
    }
    let mut buffer = [0i8; 512];
    let ok = unsafe {
        CFStringGetCString(
            reference,
            buffer.as_mut_ptr(),
            buffer.len() as CFIndex,
            kCFStringEncodingUTF8,
        )
    };
    unsafe { CFRelease(reference as *const _) };
    if ok == 0 {
        return String::new();
    }
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
