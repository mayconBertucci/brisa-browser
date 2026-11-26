use std::{ffi::c_void, ptr::null_mut};

use base64::{Engine, engine::general_purpose::STANDARD};
use windows_core::{IUnknown, Interface};
use windows_sys::Win32::{
    Foundation::RECT,
    Graphics::{
        Gdi::HDC,
        GdiPlus::{GdipCreateHICONFromBitmap, GdipDisposeImage, GpBitmap},
    },
    UI::WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx, HICON},
};

#[link(name = "shlwapi")]
unsafe extern "system" {
    fn SHCreateMemStream(initial: *const u8, size: u32) -> *mut c_void;
}

#[link(name = "gdiplus")]
unsafe extern "system" {
    fn GdipCreateBitmapFromStream(stream: *mut c_void, bitmap: *mut *mut GpBitmap) -> i32;
}

const MAX_PNG_BYTES: usize = 64 * 1024;
const MAX_STORED_BYTES: usize = 6 * 1024;
const MAX_STORED_TEXT: usize = MAX_STORED_BYTES / 3 * 4;

pub struct Favicon {
    png: Vec<u8>,
    bitmap: HICON,
}

impl Favicon {
    pub fn from_png(png: Vec<u8>) -> Option<Self> {
        if !valid_png(&png) {
            return None;
        }
        // SAFETY: SHCreateMemStream copies the input and returns one owned
        // IStream reference. IUnknown owns that reference until decoding ends.
        let stream = unsafe {
            let stream = SHCreateMemStream(png.as_ptr(), png.len() as u32);
            if stream.is_null() {
                return None;
            }
            IUnknown::from_raw(stream)
        };
        let mut bitmap = null_mut();
        let mut icon = null_mut();
        // SAFETY: The stream outlives the GDI+ image. The output handles are
        // initialized and only the returned image is disposed after decoding.
        unsafe {
            if GdipCreateBitmapFromStream(stream.as_raw(), &mut bitmap) != 0 || bitmap.is_null() {
                return None;
            }
            let status = GdipCreateHICONFromBitmap(bitmap, &mut icon);
            GdipDisposeImage(bitmap.cast());
            if status != 0 || icon.is_null() {
                return None;
            }
        }
        Some(Self { png, bitmap: icon })
    }

    pub fn from_stored(text: &str) -> Option<Self> {
        Self::from_png(decode(text)?)
    }

    pub fn stored(&self) -> Option<String> {
        (self.png.len() <= MAX_STORED_BYTES).then(|| STANDARD.encode(&self.png))
    }

    pub fn same_image(&self, other: &Self) -> bool {
        self.png == other.png
    }

    pub fn draw(&self, dc: HDC, rect: RECT, size: i32) {
        let x = rect.left + (rect.right - rect.left - size) / 2;
        let y = rect.top + (rect.bottom - rect.top - size) / 2;
        // SAFETY: Both the icon and the caller's paint DC live through this call.
        unsafe { DrawIconEx(dc, x, y, self.bitmap, size, size, 0, null_mut(), DI_NORMAL) };
    }
}

impl Drop for Favicon {
    fn drop(&mut self) {
        // SAFETY: GDI+ gives this instance the only owned icon reference.
        unsafe { DestroyIcon(self.bitmap) };
    }
}

fn valid_png(png: &[u8]) -> bool {
    if png.len() > MAX_PNG_BYTES || !png.starts_with(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR") {
        return false;
    }
    let Some(size) = png.get(16..24) else {
        return false;
    };
    size.as_chunks::<4>()
        .0
        .iter()
        .all(|bytes| (1..=256).contains(&u32::from_be_bytes(*bytes)))
}

pub fn decode(text: &str) -> Option<Vec<u8>> {
    if text.len() > MAX_STORED_TEXT {
        return None;
    }
    let decoded = STANDARD.decode(text).ok()?;
    valid_png(&decoded).then_some(decoded)
}
