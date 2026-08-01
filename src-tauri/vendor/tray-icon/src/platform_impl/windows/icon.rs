// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

// taken from https://github.com/rust-windowing/winit/blob/92fdf5ba85f920262a61cee4590f4a11ad5738d1/src/platform_impl/windows/icon.rs

use std::{fmt, io, mem, path::Path, sync::Arc};

use windows_sys::{
    core::PCWSTR,
    Win32::{
        Graphics::Gdi::{
            CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER,
            DIB_RGB_COLORS, HBITMAP,
        },
        UI::WindowsAndMessaging::{
            CreateIconIndirect, DestroyIcon, LoadImageW, HICON, ICONINFO, IMAGE_ICON,
            LR_DEFAULTSIZE, LR_LOADFROMFILE,
        },
    },
};

use crate::icon::*;

use super::util;

impl Pixel {
    fn convert_to_bgra_and_premultiply_alpha(&mut self) {
        mem::swap(&mut self.r, &mut self.b);
        let a = self.a as u16;
        self.r = ((self.r as u16 * a + 127) / 255) as u8;
        self.g = ((self.g as u16 * a + 127) / 255) as u8;
        self.b = ((self.b as u16 * a + 127) / 255) as u8;
    }
}

impl RgbaIcon {
    // NOTE(driftlet): patched — upstream passed a 1-byte-per-pixel buffer where
    // CreateIcon expects a 1bpp monochrome AND mask, so the resulting HICON had
    // a garbage mask. GDI paths (DrawIconEx) ignore the mask for 32bpp icons and
    // looked fine, but mask-aware consumers (Task Manager process icon, tray
    // drag image) rendered striped garbage. Build a proper mask + 32bpp DIB and
    // use CreateIconIndirect instead.
    fn into_windows_icon(self) -> Result<WinIcon, BadIcon> {
        let mut rgba = self.rgba;
        let width = self.width as usize;
        let height = self.height as usize;

        // 1bpp AND mask, rows padded to a 16-bit boundary: bit 1 = transparent.
        let mask_stride = (width + 15) / 16 * 2;
        let mut and_mask = vec![0u8; mask_stride * height];

        let pixel_count = rgba.len() / PIXEL_SIZE;
        let pixels =
            unsafe { std::slice::from_raw_parts_mut(rgba.as_mut_ptr() as *mut Pixel, pixel_count) };
        for (i, pixel) in pixels.iter_mut().enumerate() {
            if pixel.a < 128 {
                let x = i % width;
                and_mask[(i / width) * mask_stride + x / 8] |= 0x80 >> (x % 8);
            }
            pixel.convert_to_bgra_and_premultiply_alpha();
        }

        let hbm_mask = unsafe {
            CreateBitmap(
                self.width as i32,
                self.height as i32,
                1,
                1,
                and_mask.as_ptr() as *const _,
            )
        };
        if hbm_mask.is_null() {
            return Err(BadIcon::OsError(io::Error::last_os_error()));
        }

        // 32bpp top-down DIB section for the color bitmap.
        let mut bmi: BITMAPINFO = unsafe { mem::zeroed() };
        bmi.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = self.width as i32;
        bmi.bmiHeader.biHeight = -(self.height as i32);
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbm_color = unsafe {
            CreateDIBSection(
                std::ptr::null_mut(),
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            )
        };
        if hbm_color.is_null() {
            unsafe { DeleteObject(hbm_mask) };
            return Err(BadIcon::OsError(io::Error::last_os_error()));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), bits as *mut u8, rgba.len());
        }

        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: hbm_mask,
            hbmColor: hbm_color as HBITMAP,
        };
        let handle = unsafe { CreateIconIndirect(&icon_info) };
        unsafe {
            DeleteObject(hbm_mask);
            DeleteObject(hbm_color);
        }
        if !handle.is_null() {
            Ok(WinIcon::from_handle(handle))
        } else {
            Err(BadIcon::OsError(io::Error::last_os_error()))
        }
    }
}

#[derive(Debug)]
struct RaiiIcon {
    handle: HICON,
}

#[derive(Clone)]
pub(crate) struct WinIcon {
    inner: Arc<RaiiIcon>,
}

unsafe impl Send for WinIcon {}

impl WinIcon {
    pub fn as_raw_handle(&self) -> HICON {
        self.inner.handle
    }

    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Self, BadIcon> {
        let rgba_icon = RgbaIcon::from_rgba(rgba, width, height)?;
        rgba_icon.into_windows_icon()
    }

    pub(crate) fn from_handle(handle: HICON) -> Self {
        Self {
            #[allow(clippy::arc_with_non_send_sync)]
            inner: Arc::new(RaiiIcon { handle }),
        }
    }

    pub(crate) fn from_path<P: AsRef<Path>>(
        path: P,
        size: Option<(u32, u32)>,
    ) -> Result<Self, BadIcon> {
        // width / height of 0 along with LR_DEFAULTSIZE tells windows to load the default icon size
        let (width, height) = size.unwrap_or((0, 0));

        let wide_path = util::encode_wide(path.as_ref());

        let handle = unsafe {
            LoadImageW(
                std::ptr::null_mut(),
                wide_path.as_ptr(),
                IMAGE_ICON,
                width as i32,
                height as i32,
                LR_DEFAULTSIZE | LR_LOADFROMFILE,
            )
        };
        if !handle.is_null() {
            Ok(WinIcon::from_handle(handle as HICON))
        } else {
            Err(BadIcon::OsError(io::Error::last_os_error()))
        }
    }

    fn from_resource_inner_name(name: PCWSTR, size: Option<(u32, u32)>) -> Result<Self, BadIcon> {
        // width / height of 0 along with LR_DEFAULTSIZE tells windows to load the default icon size
        let (width, height) = size.unwrap_or((0, 0));
        let handle = unsafe {
            LoadImageW(
                util::get_instance_handle(),
                name,
                IMAGE_ICON,
                width as i32,
                height as i32,
                LR_DEFAULTSIZE,
            )
        };
        if !handle.is_null() {
            Ok(WinIcon::from_handle(handle as HICON))
        } else {
            Err(BadIcon::OsError(io::Error::last_os_error()))
        }
    }

    pub(crate) fn from_resource(
        resource_id: u16,
        size: Option<(u32, u32)>,
    ) -> Result<Self, BadIcon> {
        Self::from_resource_inner_name(resource_id as PCWSTR, size)
    }

    pub(crate) fn from_resource_name(
        resource_name: &str,
        size: Option<(u32, u32)>,
    ) -> Result<Self, BadIcon> {
        let wide_name = util::encode_wide(resource_name);
        Self::from_resource_inner_name(wide_name.as_ptr(), size)
    }
}

impl Drop for RaiiIcon {
    fn drop(&mut self) {
        unsafe { DestroyIcon(self.handle) };
    }
}

impl fmt::Debug for WinIcon {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        (*self.inner).fmt(formatter)
    }
}
