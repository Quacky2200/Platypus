// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A GLES 3 context through EGL: ANGLE on Windows (`libEGL.dll`, which draws through Direct3D
//! 11), the system's EGL (Mesa) elsewhere. The library is loaded here, not linked, so a program
//! that never asks for it needs no EGL, and ANGLE can come from a folder of the caller's choice.
//!
//! With the `static-angle` feature (Windows) ANGLE is compiled into the program instead
//! (`mozangle`) and `angle_dir` is ignored. mozangle links ANGLE whole-archive, so every binary
//! of a crate built with the feature carries it (~7 MB), whether it draws with GLES or not.

use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;

use khronos_egl as egl;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

// With `static-angle` the `egl*` calls go straight to the linked ANGLE.
#[cfg(all(windows, feature = "static-angle"))]
type Instance = egl::Instance<egl::Static>;
#[cfg(not(all(windows, feature = "static-angle")))]
type Instance = egl::DynamicInstance<egl::EGL1_4>;

// Nothing names mozangle's items; this keeps its static libraries on the link line.
#[cfg(all(windows, feature = "static-angle"))]
use mozangle as _;

/// `EGL_OPENGL_ES3_BIT` (EGL_KHR_create_context; not in EGL 1.4's constants).
const OPENGL_ES3_BIT: egl::Int = 0x0040;

/// A current GLES 3 context and its surface.
pub struct EglContext {
    egl: Arc<Instance>,
    display: egl::Display,
    context: egl::Context,
    surface: egl::Surface,
}

impl EglContext {
    /// A context drawing into a small off-screen surface, for rendering to framebuffers
    /// (tests, screenshots). `angle_dir`: the folder with ANGLE's `libEGL.dll` and
    /// `libGLESv2.dll`; without one the usual library search applies (next to the program).
    pub fn headless(angle_dir: Option<&Path>) -> Result<Self, String> {
        let egl = load(angle_dir)?;
        let display = default_display(&egl)?;
        let config = choose_config(&egl, display, egl::PBUFFER_BIT)?;
        let context = create_context(&egl, display, config)?;
        let surface = egl
            .create_pbuffer_surface(display, config, &[egl::WIDTH, 1, egl::HEIGHT, 1, egl::NONE])
            .map_err(|e| format!("EGL create_pbuffer_surface: {e}"))?;
        Self::finish(egl, display, context, surface)
    }

    /// A context presenting to `window` (Win32 and Xlib; any other windowing system is an
    /// error, so the caller can fall back to another renderer).
    pub fn window<W>(angle_dir: Option<&Path>, window: &W) -> Result<Self, String>
    where
        W: HasWindowHandle + HasDisplayHandle,
    {
        let win = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        let disp = window.display_handle().map_err(|e| e.to_string())?.as_raw();
        let (native_display, native_window) = match (disp, win) {
            #[cfg(windows)]
            (_, RawWindowHandle::Win32(h)) => (egl::DEFAULT_DISPLAY, h.hwnd.get() as *mut c_void),
            (RawDisplayHandle::Xlib(d), RawWindowHandle::Xlib(h)) => (
                d.display.map_or(egl::DEFAULT_DISPLAY, |p| p.as_ptr()),
                h.window as *mut c_void,
            ),
            _ => return Err("EGL: this windowing system isn't supported".into()),
        };
        let egl = load(angle_dir)?;
        let display = unsafe { display_for(&egl, native_display) }.ok_or("EGL: no display")?;
        let config = choose_config(&egl, display, egl::WINDOW_BIT)?;
        let context = create_context(&egl, display, config)?;
        // A plain surface: ANGLE's sRGB one would encode on write, but reading it back
        // returned the unencoded bytes, so the blit encodes itself (`Target::srgb: false`).
        let surface = unsafe { egl.create_window_surface(display, config, native_window, None) }
            .map_err(|e| format!("EGL create_window_surface: {e}"))?;
        Self::finish(egl, display, context, surface)
    }

    fn finish(
        egl: Instance,
        display: egl::Display,
        context: egl::Context,
        surface: egl::Surface,
    ) -> Result<Self, String> {
        egl.make_current(display, Some(surface), Some(surface), Some(context))
            .map_err(|e| format!("EGL make_current: {e}"))?;
        Ok(Self {
            egl: Arc::new(egl),
            display,
            context,
            surface,
        })
    }

    /// Shows the frame.
    pub fn swap_buffers(&self) -> Result<(), String> {
        self.egl
            .swap_buffers(self.display, self.surface)
            .map_err(|e| format!("EGL swap_buffers: {e}"))
    }

    /// Waits for the display's refresh in `swap_buffers` (or not).
    pub fn set_vsync(&self, on: bool) {
        let _ = self.egl.swap_interval(self.display, i32::from(on));
    }

    /// The surface's size in pixels (it follows the window).
    pub fn size(&self) -> (u32, u32) {
        let q = |a| {
            self.egl
                .query_surface(self.display, self.surface, a)
                .unwrap_or(0)
                .max(0) as u32
        };
        (q(egl::WIDTH), q(egl::HEIGHT))
    }

    /// Asks the Direct3D 11 driver under ANGLE to give back the memory it keeps for itself after
    /// the first draws (`IDXGIDevice3::Trim`, what ANGLE calls when a UWP app is suspended; on
    /// `css`, NVIDIA, ~14 MB of private bytes that stay free while frames go on). Returns whether
    /// it could: `false` without ANGLE's D3D11 device (another EGL, not Windows).
    pub fn trim_device(&self) -> bool {
        #[cfg(windows)]
        {
            let Some(device) = self.d3d11_device() else {
                return false;
            };
            // SAFETY: `device` is ANGLE's live ID3D11Device (a COM object).
            unsafe { trim_dxgi(device) }
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// ANGLE's `ID3D11Device` (EGL_EXT_device_query + EGL_ANGLE_device_d3d), not AddRef'd.
    #[cfg(windows)]
    fn d3d11_device(&self) -> Option<*mut c_void> {
        type QueryDisplay = unsafe extern "system" fn(egl::EGLDisplay, egl::Int, *mut isize) -> u32;
        type QueryDevice = unsafe extern "system" fn(*mut c_void, egl::Int, *mut isize) -> u32;
        const DEVICE_EXT: egl::Int = 0x322C;
        const D3D11_DEVICE_ANGLE: egl::Int = 0x33A1;
        let qd = self.egl.get_proc_address("eglQueryDisplayAttribEXT")?;
        let qv = self.egl.get_proc_address("eglQueryDeviceAttribEXT")?;
        // SAFETY: the EGL_EXT_device_query entry points, with their signatures.
        type Proc = extern "system" fn();
        let (qd, qv) = unsafe {
            (
                std::mem::transmute::<Proc, QueryDisplay>(qd),
                std::mem::transmute::<Proc, QueryDevice>(qv),
            )
        };
        let mut dev: isize = 0;
        if unsafe { qd(self.display.as_ptr(), DEVICE_EXT, &mut dev) } == 0 || dev == 0 {
            return None;
        }
        let mut d3d: isize = 0;
        if unsafe { qv(dev as *mut c_void, D3D11_DEVICE_ANGLE, &mut d3d) } == 0 || d3d == 0 {
            return None;
        }
        Some(d3d as *mut c_void)
    }

    /// The GL entry points of the current context.
    pub fn gl(&self) -> glow::Context {
        let egl = self.egl.clone();
        unsafe {
            glow::Context::from_loader_function(move |name| {
                egl.get_proc_address(name)
                    .map_or(std::ptr::null(), |f| f as *const c_void)
            })
        }
    }
}

impl Drop for EglContext {
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_surface(self.display, self.surface);
        let _ = self.egl.destroy_context(self.display, self.context);
        let _ = self.egl.terminate(self.display);
    }
}

/// `IDXGIDevice3::Trim` on the DXGI device of `device` (an `ID3D11Device`).
///
/// # Safety
/// `device` is a live COM object.
#[cfg(windows)]
unsafe fn trim_dxgi(device: *mut c_void) -> bool {
    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);
    // IID_IDXGIDevice3 {6007896c-3244-4afd-bf18-a6d3beda5023}
    const IID: Guid = Guid(
        0x6007_896c,
        0x3244,
        0x4afd,
        [0xbf, 0x18, 0xa6, 0xd3, 0xbe, 0xda, 0x50, 0x23],
    );
    type QueryInterface =
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32;
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;
    type Trim = unsafe extern "system" fn(*mut c_void);
    // Vtable slots: IUnknown 0..3 (Release is 2), IDXGIObject 3..7, IDXGIDevice 7..12,
    // IDXGIDevice1 12..14, IDXGIDevice2 14..17, IDXGIDevice3 17 (Trim).
    unsafe {
        let vtable = *(device as *const *const usize);
        let qi: QueryInterface = std::mem::transmute(*vtable);
        let mut dxgi: *mut c_void = std::ptr::null_mut();
        if qi(device, &IID, &mut dxgi) < 0 || dxgi.is_null() {
            return false;
        }
        let dv = *(dxgi as *const *const usize);
        let trim: Trim = std::mem::transmute(*dv.add(17));
        trim(dxgi);
        let release: Release = std::mem::transmute(*dv.add(2));
        release(dxgi);
        true
    }
}

fn default_display(egl: &Instance) -> Result<egl::Display, String> {
    unsafe { display_for(egl, egl::DEFAULT_DISPLAY) }
        .ok_or_else(|| "EGL: no default display".into())
}

// ANGLE's `EGL_ANGLE_platform_angle` attributes (`eglext_angle.h`). The platform itself is only asked
// for on Windows (`display_for`).
#[cfg(windows)]
const PLATFORM_ANGLE_ANGLE: u32 = 0x3202;
const PLATFORM_ANGLE_TYPE_ANGLE: egl::Int = 0x3203;
const PLATFORM_ANGLE_TYPE_D3D11_ANGLE: egl::Int = 0x3208;
const PLATFORM_ANGLE_DEVICE_TYPE_ANGLE: egl::Int = 0x3209;
const PLATFORM_ANGLE_DEVICE_TYPE_HARDWARE_ANGLE: egl::Int = 0x320A;
const PLATFORM_ANGLE_DEBUG_LAYERS_ENABLED_ANGLE: egl::Int = 0x3451;

/// The attributes of ANGLE's display: Direct3D 11 on the hardware device, without the D3D debug
/// layer. ANGLE turns that layer on whenever its asserts are compiled in, and the `cc` build of
/// mozangle defines no `NDEBUG`, so a release program got a debug device where the Graphics Tools
/// are installed (`D3D11_3SDKLayers.dll`, every resource zeroed and tracked: on `css` ~12 MB more
/// private bytes and ~10 MB more private working set). `PLATYPUS_ANGLE_DEBUG=1` leaves the choice
/// to ANGLE (the layer is on where it is installed).
pub fn angle_display_attribs(debug: bool) -> Vec<egl::Int> {
    let mut a = vec![
        PLATFORM_ANGLE_TYPE_ANGLE,
        PLATFORM_ANGLE_TYPE_D3D11_ANGLE,
        PLATFORM_ANGLE_DEVICE_TYPE_ANGLE,
        PLATFORM_ANGLE_DEVICE_TYPE_HARDWARE_ANGLE,
    ];
    if !debug {
        a.extend([
            PLATFORM_ANGLE_DEBUG_LAYERS_ENABLED_ANGLE,
            egl::FALSE as egl::Int,
        ]);
    }
    a.push(egl::NONE);
    a
}

/// The display for `native`: on Windows ANGLE's, through `eglGetPlatformDisplayEXT` with
/// [`angle_display_attribs`] (plain `eglGetDisplay` when that entry point is missing), elsewhere
/// the plain one.
///
/// # Safety
/// `native` is `EGL_DEFAULT_DISPLAY` or a live native display.
unsafe fn display_for(egl: &Instance, native: egl::NativeDisplayType) -> Option<egl::Display> {
    #[cfg(windows)]
    {
        type GetPlatformDisplayExt =
            unsafe extern "system" fn(u32, *mut c_void, *const egl::Int) -> egl::EGLDisplay;
        if let Some(f) = egl.get_proc_address("eglGetPlatformDisplayEXT") {
            let debug = std::env::var("PLATYPUS_ANGLE_DEBUG").is_ok_and(|v| v == "1");
            let attribs = angle_display_attribs(debug);
            // SAFETY: the entry point of EGL_EXT_platform_base, with its signature.
            let f: GetPlatformDisplayExt = unsafe { std::mem::transmute(f) };
            let d = unsafe { f(PLATFORM_ANGLE_ANGLE, native, attribs.as_ptr()) };
            if !d.is_null() {
                return Some(unsafe { egl::Display::from_ptr(d) });
            }
        }
    }
    unsafe { egl.get_display(native) }
}

fn choose_config(
    egl: &Instance,
    display: egl::Display,
    surface_type: egl::Int,
) -> Result<egl::Config, String> {
    egl.initialize(display)
        .map_err(|e| format!("EGL initialize: {e}"))?;
    egl.bind_api(egl::OPENGL_ES_API)
        .map_err(|e| format!("EGL bind_api: {e}"))?;
    let config = egl
        .choose_first_config(
            display,
            &[
                egl::SURFACE_TYPE,
                surface_type,
                egl::RENDERABLE_TYPE,
                OPENGL_ES3_BIT,
                egl::RED_SIZE,
                8,
                egl::GREEN_SIZE,
                8,
                egl::BLUE_SIZE,
                8,
                egl::ALPHA_SIZE,
                8,
                // The UI draws into its own canvases and only blits to the surface: no depth,
                // stencil or multisampled buffers behind it.
                egl::DEPTH_SIZE,
                0,
                egl::STENCIL_SIZE,
                0,
                egl::SAMPLE_BUFFERS,
                0,
                egl::NONE,
            ],
        )
        .map_err(|e| format!("EGL choose_config: {e}"))?
        .ok_or_else(|| "EGL: no GLES 3 config".to_string())?;
    let q = |a| egl.get_config_attrib(display, config, a).unwrap_or(-1);
    tracing::info!(
        "EGL config: {} bits, depth {}, stencil {}, {} samples",
        q(egl::BUFFER_SIZE),
        q(egl::DEPTH_SIZE),
        q(egl::STENCIL_SIZE),
        q(egl::SAMPLES)
    );
    Ok(config)
}

fn create_context(
    egl: &Instance,
    display: egl::Display,
    config: egl::Config,
) -> Result<egl::Context, String> {
    egl.create_context(
        display,
        config,
        None,
        &[egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE],
    )
    .map_err(|e| format!("EGL create_context: {e}"))
}

#[cfg(all(windows, feature = "static-angle"))]
fn load(_angle_dir: Option<&Path>) -> Result<Instance, String> {
    Ok(egl::Instance::new(egl::Static))
}

#[cfg(not(all(windows, feature = "static-angle")))]
fn load(angle_dir: Option<&Path>) -> Result<Instance, String> {
    #[cfg(windows)]
    {
        if let Some(dir) = angle_dir {
            // libEGL.dll finds libGLESv2.dll through the DLL search path.
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe { windows_sys::Win32::System::LibraryLoader::SetDllDirectoryW(wide.as_ptr()) };
        }
        unsafe { Instance::load_required_from_filename("libEGL.dll") }
            .map_err(|e| format!("EGL: can't load libEGL.dll (ANGLE): {e}"))
    }
    #[cfg(not(windows))]
    {
        let _ = angle_dir;
        unsafe { Instance::load_required() }.map_err(|e| format!("EGL: can't load libEGL: {e}"))
    }
}
