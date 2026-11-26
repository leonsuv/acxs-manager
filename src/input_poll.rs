//! Live controller/wheel/pedal polling abstraction for Controls binding & calibration.
//!
//! - On Windows (`#[cfg(windows)]`), uses pure `windows-sys` (`XInputGetState` for X360/Xbox
//!   controllers and dynamic `dinput8.dll` COM vtable with a static `DIDATAFORMAT` for
//!   DirectInput wheels, pedals, shifters, and handbrakes) — zero C/C++ build scripts.
//! - On non-Windows hosts (`#[cfg(not(windows))]`), provides [`StubInputBackend`] so
//!   `cargo test --lib` compiles and passes cleanly on macOS/Linux.
//! - [`MockInputBackend`] allows deterministic unit testing of binding detection and axis math.

use serde::{Deserialize, Serialize};

/// Maximum axes tracked per device (X, Y, Z, Rx, Ry, Rz, Slider0, Slider1).
pub const MAX_AXES: usize = 8;
/// Maximum buttons tracked per device (DirectInput DIJOYSTATE2 supports 128).
pub const MAX_BUTTONS: usize = 128;

/// Metadata for one connected game controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub index: u32,
    pub name: String,
    /// Product GUID formatted as `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` for `controls.ini`.
    pub product_guid: String,
    pub is_xinput: bool,
}

/// Live normalized state of one device.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceState {
    /// Axes normalized to `-1.0..=1.0`.
    pub axes: [f32; MAX_AXES],
    pub buttons: [bool; MAX_BUTTONS],
    /// POV hats in hundredths of a degree, or `-1` when centered.
    pub povs: [i32; 4],
}

impl Default for DeviceState {
    fn default() -> Self {
        Self {
            axes: [0.0; MAX_AXES],
            buttons: [false; MAX_BUTTONS],
            povs: [-1; 4],
        }
    }
}

/// Detected user input while listening for a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindEvent {
    Axis { device: u32, axis: u8, inverted: bool },
    Button { device: u32, button: u8 },
}

/// Platform-agnostic live input polling backend.
pub trait InputBackend: Send {
    fn enumerate(&mut self) -> Vec<DeviceInfo>;
    fn poll(&mut self, index: u32) -> Option<DeviceState>;
}

/// Detects the first significant axis movement (|delta| >= `axis_threshold`) or newly pressed
/// button between `before` and `after` for `device`.
pub fn detect_binding(
    device: u32,
    before: &DeviceState,
    after: &DeviceState,
    axis_threshold: f32,
) -> Option<BindEvent> {
    for (btn, (&was, &now)) in before.buttons.iter().zip(after.buttons.iter()).enumerate() {
        if !was && now {
            return Some(BindEvent::Button {
                device,
                button: btn as u8,
            });
        }
    }
    let mut best_axis: Option<(u8, f32)> = None;
    for (idx, (&a, &b)) in before.axes.iter().zip(after.axes.iter()).enumerate() {
        let delta = b - a;
        if delta.abs() >= axis_threshold {
            let replace = best_axis.is_none_or(|(_, best)| delta.abs() > best.abs());
            if replace {
                best_axis = Some((idx as u8, delta));
            }
        }
    }
    best_axis.map(|(axis, delta)| BindEvent::Axis {
        device,
        axis,
        inverted: delta < 0.0,
    })
}

/// Applies AC-style pedal calibration: maps raw `-1.0..=1.0` through `min`/`max` bounds,
/// optional inversion, `range_from..=range_to` deadzone/saturation trim, and `gamma` curve.
/// Returns `0.0..=1.0`.
pub fn apply_pedal_curve(
    raw: f32,
    min: f32,
    max: f32,
    gamma: f32,
    range_from: f32,
    range_to: f32,
    inverted: bool,
) -> f32 {
    let span = max - min;
    if !raw.is_finite() || span.abs() < 1e-6 {
        return 0.0;
    }
    let mut norm = ((raw - min) / span).clamp(0.0, 1.0);
    if inverted {
        norm = 1.0 - norm;
    }
    let r_from = range_from.clamp(0.0, 0.99);
    let r_to = range_to.clamp(r_from + 0.01, 1.0);
    let trimmed = ((norm - r_from) / (r_to - r_from)).clamp(0.0, 1.0);
    let g = if gamma.is_finite() && gamma > 0.0 {
        gamma.clamp(0.2, 5.0)
    } else {
        1.0
    };
    trimmed.powf(g).clamp(0.0, 1.0)
}

/// Deterministic mock backend for headless unit tests and UI previews.
#[derive(Debug, Default)]
pub struct MockInputBackend {
    pub devices: Vec<DeviceInfo>,
    pub states: Vec<DeviceState>,
}

impl InputBackend for MockInputBackend {
    fn enumerate(&mut self) -> Vec<DeviceInfo> {
        self.devices.clone()
    }

    fn poll(&mut self, index: u32) -> Option<DeviceState> {
        self.states.get(index as usize).cloned()
    }
}

/// Non-Windows stub backend (always returns no physical devices).
#[cfg(not(windows))]
#[derive(Debug, Default)]
pub struct StubInputBackend;

#[cfg(not(windows))]
impl InputBackend for StubInputBackend {
    fn enumerate(&mut self) -> Vec<DeviceInfo> {
        Vec::new()
    }

    fn poll(&mut self, _index: u32) -> Option<DeviceState> {
        None
    }
}

/// Creates the default platform input backend.
pub fn default_backend() -> Box<dyn InputBackend> {
    #[cfg(windows)]
    {
        Box::new(WinInputBackend::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(StubInputBackend)
    }
}

// ---------------------------------------------------------------------------
// Windows DirectInput8 + XInput Backend (pure windows-sys, zero C build deps)
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod win_backend {
    use super::*;
    use std::ffi::c_void;
    use std::ptr;
    use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};

    const DIRECTINPUT_VERSION: u32 = 0x0800;
    const DI8DEVCLASS_GAMECTRL: u32 = 4;
    const DIEDFL_ATTACHEDONLY: u32 = 0x0000_0001;
    const DIDF_ABSAXIS: u32 = 0x0000_0001;
    const DIDFT_ANYINSTANCE: u32 = 0x00FF_FF00;
    const DIDFT_AXIS: u32 = 0x0000_0003;
    const DIDFT_BUTTON: u32 = 0x0000_000C;
    const DIDFT_POV: u32 = 0x0000_0010;
    const DIDFT_OPTIONAL: u32 = 0x8000_0000;
    const DISCL_NONEXCLUSIVE: u32 = 0x0000_0002;
    const DISCL_BACKGROUND: u32 = 0x0000_0008;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    impl Guid {
        fn to_braced_string(&self) -> String {
            format!(
                "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
                self.data1,
                self.data2,
                self.data3,
                self.data4[0],
                self.data4[1],
                self.data4[2],
                self.data4[3],
                self.data4[4],
                self.data4[5],
                self.data4[6],
                self.data4[7],
            )
        }
    }

    const IID_IDIRECTINPUT8W: Guid = Guid {
        data1: 0xBF79_8031,
        data2: 0x483A,
        data3: 0x4DA2,
        data4: [0xAA, 0x99, 0x5D, 0x64, 0xED, 0x36, 0x97, 0x00],
    };

    #[repr(C)]
    struct DiDeviceInstanceW {
        dw_size: u32,
        guid_instance: Guid,
        guid_product: Guid,
        dw_dev_type: u32,
        tsz_instance_name: [u16; 260],
        tsz_product_name: [u16; 260],
        guid_ff_driver: Guid,
        w_usage_page: u16,
        w_usage: u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct DiObjectDataFormat {
        pguid: *const Guid,
        dw_ofs: u32,
        dw_type: u32,
        dw_flags: u32,
    }
    unsafe impl Sync for DiObjectDataFormat {}

    #[repr(C)]
    struct DiDataFormat {
        dw_size: u32,
        dw_obj_size: u32,
        dw_flags: u32,
        dw_data_size: u32,
        dw_num_objs: u32,
        rgodf: *const DiObjectDataFormat,
    }
    unsafe impl Sync for DiDataFormat {}

    /// Compact joystick state layout matching our static `DIDATAFORMAT`:
    /// 8 i32 axes (32 B) + 4 i32 POVs (16 B) + 128 u8 buttons (128 B) = 176 bytes.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct JoyStateRaw {
        axes: [i32; 8],
        povs: [i32; 4],
        buttons: [u8; 128],
    }

    const fn make_objects() -> [DiObjectDataFormat; 140] {
        let mut objs = [DiObjectDataFormat {
            pguid: ptr::null(),
            dw_ofs: 0,
            dw_type: 0,
            dw_flags: 0,
        }; 140];
        let mut i = 0;
        while i < 8 {
            objs[i] = DiObjectDataFormat {
                pguid: ptr::null(),
                dw_ofs: (i * 4) as u32,
                dw_type: DIDFT_AXIS | DIDFT_ANYINSTANCE | DIDFT_OPTIONAL,
                dw_flags: 0,
            };
            i += 1;
        }
        let mut p = 0;
        while p < 4 {
            objs[8 + p] = DiObjectDataFormat {
                pguid: ptr::null(),
                dw_ofs: (32 + p * 4) as u32,
                dw_type: DIDFT_POV | DIDFT_ANYINSTANCE | DIDFT_OPTIONAL,
                dw_flags: 0,
            };
            p += 1;
        }
        let mut b = 0;
        while b < 128 {
            objs[12 + b] = DiObjectDataFormat {
                pguid: ptr::null(),
                dw_ofs: (48 + b) as u32,
                dw_type: DIDFT_BUTTON | DIDFT_ANYINSTANCE | DIDFT_OPTIONAL,
                dw_flags: 0,
            };
            b += 1;
        }
        objs
    }

    static JOY_OBJECTS: [DiObjectDataFormat; 140] = make_objects();
    static JOY_FORMAT: DiDataFormat = DiDataFormat {
        dw_size: std::mem::size_of::<DiDataFormat>() as u32,
        dw_obj_size: std::mem::size_of::<DiObjectDataFormat>() as u32,
        dw_flags: DIDF_ABSAXIS,
        dw_data_size: std::mem::size_of::<JoyStateRaw>() as u32,
        dw_num_objs: JOY_OBJECTS.len() as u32,
        rgodf: JOY_OBJECTS.as_ptr(),
    };

    type DirectInput8CreateFn = unsafe extern "system" fn(
        hinst: *mut c_void,
        dw_version: u32,
        riidltf: *const Guid,
        ppv_out: *mut *mut c_void,
        punk_outer: *mut c_void,
    ) -> i32;

    type EnumCallbackFn = unsafe extern "system" fn(*const DiDeviceInstanceW, *mut c_void) -> i32;

    #[repr(C)]
    struct IDirectInput8WVtbl {
        _query_interface: *const c_void,
        _add_ref: *const c_void,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        create_device: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void, *mut c_void) -> i32,
        enum_devices: unsafe extern "system" fn(*mut c_void, u32, EnumCallbackFn, *mut c_void, u32) -> i32,
    }

    #[repr(C)]
    struct IDirectInputDevice8WVtbl {
        _query_interface: *const c_void,
        _add_ref: *const c_void,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        _get_capabilities: *const c_void,
        _enum_objects: *const c_void,
        _get_property: *const c_void,
        _set_property: *const c_void,
        acquire: unsafe extern "system" fn(*mut c_void) -> i32,
        unacquire: unsafe extern "system" fn(*mut c_void) -> i32,
        get_device_state: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> i32,
        _get_device_data: *const c_void,
        set_data_format: unsafe extern "system" fn(*mut c_void, *const DiDataFormat) -> i32,
        _set_event_notification: *const c_void,
        set_cooperative_level: unsafe extern "system" fn(*mut c_void, *mut c_void, u32) -> i32,
        _get_object_info: *const c_void,
        _get_device_info: *const c_void,
        _run_control_panel: *const c_void,
        _initialize: *const c_void,
        _create_effect: *const c_void,
        _enum_effects: *const c_void,
        _get_effect_info: *const c_void,
        _get_force_feedback_state: *const c_void,
        _send_force_feedback_command: *const c_void,
        _enum_created_effect_objects: *const c_void,
        _escape: *const c_void,
        poll: unsafe extern "system" fn(*mut c_void) -> i32,
    }

    struct DiDevice {
        info: DeviceInfo,
        ptr: *mut c_void,
    }
    unsafe impl Send for DiDevice {}

    pub struct WinInputBackend {
        dll: HMODULE,
        di8: *mut c_void,
        di_devices: Vec<DiDevice>,
        xinput_slots: Vec<u32>,
    }
    unsafe impl Send for WinInputBackend {}

    impl WinInputBackend {
        pub fn new() -> Self {
            let mut backend = Self {
                dll: ptr::null_mut(),
                di8: ptr::null_mut(),
                di_devices: Vec::new(),
                xinput_slots: Vec::new(),
            };
            unsafe { backend.init_dinput() };
            backend
        }

        unsafe fn init_dinput(&mut self) {
            let name: Vec<u16> = "dinput8.dll\0".encode_utf16().collect();
            let dll = LoadLibraryW(name.as_ptr());
            if dll.is_null() {
                return;
            }
            let sym = b"DirectInput8Create\0";
            let proc = GetProcAddress(dll, sym.as_ptr());
            let Some(proc) = proc else {
                FreeLibrary(dll);
                return;
            };
            let create_fn: DirectInput8CreateFn = std::mem::transmute(proc);
            let hinst = GetModuleHandleW(ptr::null());
            let mut di8: *mut c_void = ptr::null_mut();
            let hr = create_fn(hinst, DIRECTINPUT_VERSION, &IID_IDIRECTINPUT8W, &mut di8, ptr::null_mut());
            if hr >= 0 && !di8.is_null() {
                self.dll = dll;
                self.di8 = di8;
            } else {
                FreeLibrary(dll);
            }
        }

        fn release_devices(&mut self) {
            for dev in self.di_devices.drain(..) {
                unsafe {
                    let vtbl = *(dev.ptr as *const *const IDirectInputDevice8WVtbl);
                    ((*vtbl).unacquire)(dev.ptr);
                    ((*vtbl).release)(dev.ptr);
                }
            }
        }
    }

    impl Drop for WinInputBackend {
        fn drop(&mut self) {
            self.release_devices();
            unsafe {
                if !self.di8.is_null() {
                    let vtbl = *(self.di8 as *const *const IDirectInput8WVtbl);
                    ((*vtbl).release)(self.di8);
                }
                if !self.dll.is_null() {
                    FreeLibrary(self.dll);
                }
            }
        }
    }

    unsafe extern "system" fn enum_cb(inst: *const DiDeviceInstanceW, ctx: *mut c_void) -> i32 {
        let list = &mut *(ctx as *mut Vec<(Guid, String, String)>);
        let inst = &*inst;
        let len = inst.tsz_product_name.iter().position(|&c| c == 0).unwrap_or(260);
        let name = String::from_utf16_lossy(&inst.tsz_product_name[..len]);
        list.push((inst.guid_instance, name, inst.guid_product.to_braced_string()));
        1 // DIENUM_CONTINUE
    }

    impl InputBackend for WinInputBackend {
        fn enumerate(&mut self) -> Vec<DeviceInfo> {
            self.release_devices();
            self.xinput_slots.clear();
            let mut out = Vec::new();

            if !self.di8.is_null() {
                let mut found: Vec<(Guid, String, String)> = Vec::new();
                unsafe {
                    let vtbl = *(self.di8 as *const *const IDirectInput8WVtbl);
                    ((*vtbl).enum_devices)(
                        self.di8,
                        DI8DEVCLASS_GAMECTRL,
                        enum_cb,
                        &mut found as *mut _ as *mut c_void,
                        DIEDFL_ATTACHEDONLY,
                    );
                    for (guid, name, pguid) in found {
                        let mut dev_ptr: *mut c_void = ptr::null_mut();
                        if ((*vtbl).create_device)(self.di8, &guid, &mut dev_ptr, ptr::null_mut()) >= 0
                            && !dev_ptr.is_null()
                        {
                            let dvtbl = *(dev_ptr as *const *const IDirectInputDevice8WVtbl);
                            if ((*dvtbl).set_data_format)(dev_ptr, &JOY_FORMAT) >= 0 {
                                let _ = ((*dvtbl).set_cooperative_level)(
                                    dev_ptr,
                                    ptr::null_mut(),
                                    DISCL_BACKGROUND | DISCL_NONEXCLUSIVE,
                                );
                                let _ = ((*dvtbl).acquire)(dev_ptr);
                                let info = DeviceInfo {
                                    index: out.len() as u32,
                                    name,
                                    product_guid: pguid,
                                    is_xinput: false,
                                };
                                out.push(info.clone());
                                self.di_devices.push(DiDevice { info, ptr: dev_ptr });
                            } else {
                                ((*dvtbl).release)(dev_ptr);
                            }
                        }
                    }
                }
            }

            // Also probe XInput slots 0..4.
            for slot in 0..4u32 {
                let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };
                if unsafe { XInputGetState(slot, &mut state) } == 0 {
                    let info = DeviceInfo {
                        index: out.len() as u32,
                        name: format!("Xbox Controller {}", slot + 1),
                        product_guid: format!("XINPUT_{slot}"),
                        is_xinput: true,
                    };
                    self.xinput_slots.push(slot);
                    out.push(info);
                }
            }
            out
        }

        fn poll(&mut self, index: u32) -> Option<DeviceState> {
            let idx = index as usize;
            if let Some(dev) = self.di_devices.get(idx) {
                let mut raw = JoyStateRaw {
                    axes: [32768; 8],
                    povs: [-1; 4],
                    buttons: [0; 128],
                };
                unsafe {
                    let dvtbl = *(dev.ptr as *const *const IDirectInputDevice8WVtbl);
                    let _ = ((*dvtbl).poll)(dev.ptr);
                    if ((*dvtbl).get_device_state)(
                        dev.ptr,
                        std::mem::size_of::<JoyStateRaw>() as u32,
                        &mut raw as *mut _ as *mut c_void,
                    ) < 0
                    {
                        let _ = ((*dvtbl).acquire)(dev.ptr);
                        return None;
                    }
                }
                let mut state = DeviceState::default();
                for i in 0..MAX_AXES {
                    state.axes[i] = ((raw.axes[i] as f32 / 32767.5) - 1.0).clamp(-1.0, 1.0);
                }
                state.povs = raw.povs;
                for i in 0..MAX_BUTTONS {
                    state.buttons[i] = (raw.buttons[i] & 0x80) != 0;
                }
                return Some(state);
            }
            let x_idx = idx.checked_sub(self.di_devices.len())?;
            let &slot = self.xinput_slots.get(x_idx)?;
            let mut xstate: XINPUT_STATE = unsafe { std::mem::zeroed() };
            if unsafe { XInputGetState(slot, &mut xstate) } != 0 {
                return None;
            }
            let gp = xstate.Gamepad;
            let mut state = DeviceState::default();
            state.axes[0] = (gp.sThumbLX as f32 / 32767.0).clamp(-1.0, 1.0);
            state.axes[1] = (gp.sThumbLY as f32 / 32767.0).clamp(-1.0, 1.0);
            state.axes[2] = (gp.bRightTrigger as f32 / 127.5 - 1.0).clamp(-1.0, 1.0);
            state.axes[3] = (gp.bLeftTrigger as f32 / 127.5 - 1.0).clamp(-1.0, 1.0);
            state.axes[4] = (gp.sThumbRX as f32 / 32767.0).clamp(-1.0, 1.0);
            state.axes[5] = (gp.sThumbRY as f32 / 32767.0).clamp(-1.0, 1.0);
            for bit in 0..16 {
                state.buttons[bit] = (gp.wButtons & (1 << bit)) != 0;
            }
            Some(state)
        }
    }
}

#[cfg(windows)]
pub use win_backend::WinInputBackend;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_input_detects_axis_move_and_button_press() {
        let mut mock = MockInputBackend {
            devices: vec![DeviceInfo {
                index: 0,
                name: "Sim Wheel".into(),
                product_guid: "{12345678-0000-0000-0000-000000000000}".into(),
                is_xinput: false,
            }],
            states: vec![DeviceState::default()],
        };
        assert_eq!(mock.enumerate().len(), 1);
        let before = mock.poll(0).unwrap();

        let mut after_btn = before.clone();
        after_btn.buttons[5] = true;
        assert_eq!(
            detect_binding(0, &before, &after_btn, 0.25),
            Some(BindEvent::Button { device: 0, button: 5 })
        );

        let mut after_axis = before.clone();
        after_axis.axes[2] = -0.6;
        assert_eq!(
            detect_binding(0, &before, &after_axis, 0.25),
            Some(BindEvent::Axis {
                device: 0,
                axis: 2,
                inverted: true
            })
        );
    }

    #[test]
    fn pedal_invert_deadzone_gamma_math() {
        // Uninverted linear: raw 0.0 in [-1, 1] -> 0.5
        let mid = apply_pedal_curve(0.0, -1.0, 1.0, 1.0, 0.0, 1.0, false);
        assert!((mid - 0.5).abs() < 1e-5);

        // Inverted: raw -1.0 -> 1.0
        let inv = apply_pedal_curve(-1.0, -1.0, 1.0, 1.0, 0.0, 1.0, true);
        assert!((inv - 1.0).abs() < 1e-5);

        // Deadzone range_from=0.1, range_to=0.9: raw at 5% maps to 0.0, at 95% maps to 1.0
        assert_eq!(apply_pedal_curve(-0.9, -1.0, 1.0, 1.0, 0.1, 0.9, false), 0.0);
        assert_eq!(apply_pedal_curve(0.9, -1.0, 1.0, 1.0, 0.1, 0.9, false), 1.0);

        // Gamma 2.0 squares the normalized 0.5 -> 0.25
        let curved = apply_pedal_curve(0.0, -1.0, 1.0, 2.0, 0.0, 1.0, false);
        assert!((curved - 0.25).abs() < 1e-5);
    }
}
