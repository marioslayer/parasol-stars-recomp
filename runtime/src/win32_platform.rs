#![cfg(windows)]

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use crate::ocs::{AUDIO_RATE, JoyState, SCREEN_H, SCREEN_W};

type HWND = *mut c_void;
type HINSTANCE = *mut c_void;
type HMODULE = *mut c_void;
type HMONITOR = *mut c_void;
type HICON = *mut c_void;
type HCURSOR = *mut c_void;
type HBRUSH = *mut c_void;
type HMENU = *mut c_void;
type HDC = *mut c_void;
type HWAVEOUT = *mut c_void;
type WPARAM = usize;
type LPARAM = isize;
type LRESULT = isize;

const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
const WS_POPUP: u32 = 0x8000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const GWL_STYLE: i32 = -16;
const CW_USEDEFAULT: i32 = -0x8000_0000;
const PM_REMOVE: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;
const DIB_RGB_COLORS: u32 = 0;
const SRCCOPY: u32 = 0x00CC_0020;
const COLORONCOLOR: i32 = 3;
const IDC_ARROW: usize = 32512;
const CS_OWNDC: u32 = 0x0020;
const MONITOR_DEFAULTTONEAREST: u32 = 2;
const SWP_NOOWNERZORDER: u32 = 0x0200;
const SWP_FRAMECHANGED: u32 = 0x0020;

const WAVE_MAPPER: u32 = 0xFFFF_FFFF;
const WAVE_FORMAT_PCM: u16 = 1;
const WHDR_DONE: u32 = 0x0000_0001;

// XInput button bitmasks
const XINPUT_DPAD_UP: u16 = 0x0001;
const XINPUT_DPAD_DOWN: u16 = 0x0002;
const XINPUT_DPAD_LEFT: u16 = 0x0004;
const XINPUT_DPAD_RIGHT: u16 = 0x0008;
const XINPUT_START: u16 = 0x0010;
const XINPUT_BACK: u16 = 0x0020;
const XINPUT_RIGHT_SHOULDER: u16 = 0x0200;
const XINPUT_A: u16 = 0x1000;
const XINPUT_B: u16 = 0x2000;
const XINPUT_X: u16 = 0x4000;
const XINPUT_Y: u16 = 0x8000;
const STICK_DEADZONE: i16 = 10_000;

#[repr(C)]
struct WndClassW {
    style: u32,
    lpfn_wnd_proc: extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: HINSTANCE,
    h_icon: HICON,
    h_cursor: HCURSOR,
    hbr_background: HBRUSH,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
}

#[repr(C)]
#[derive(Default)]
struct Point { x: i32, y: i32 }

#[repr(C)]
#[derive(Default)]
struct Msg {
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    time: u32,
    pt: Point,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Rect { left: i32, top: i32, right: i32, bottom: i32 }

#[repr(C)]
struct MonitorInfo {
    cb_size: u32,
    rc_monitor: Rect,
    rc_work: Rect,
    dw_flags: u32,
}

#[repr(C)]
struct BitmapInfoHeader {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_x_pels_per_meter: i32,
    bi_y_pels_per_meter: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    bmi_header: BitmapInfoHeader,
    bmi_colors: [u32; 1],
}

#[repr(C)]
struct WaveFormatEx {
    w_format_tag: u16,
    n_channels: u16,
    n_samples_per_sec: u32,
    n_avg_bytes_per_sec: u32,
    n_block_align: u16,
    w_bits_per_sample: u16,
    cb_size: u16,
}

#[repr(C)]
struct WaveHdr {
    lp_data: *mut u8,
    dw_buffer_length: u32,
    dw_bytes_recorded: u32,
    dw_user: usize,
    dw_flags: u32,
    dw_loops: u32,
    lp_next: *mut WaveHdr,
    reserved: usize,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct XInputGamepad {
    w_buttons: u16,
    b_left_trigger: u8,
    b_right_trigger: u8,
    s_thumb_lx: i16,
    s_thumb_ly: i16,
    s_thumb_rx: i16,
    s_thumb_ry: i16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct XInputState {
    dw_packet_number: u32,
    gamepad: XInputGamepad,
}

type XInputGetStateFn = unsafe extern "system" fn(u32, *mut XInputState) -> u32;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    fn LoadLibraryW(name: *const u16) -> HMODULE;
    fn GetProcAddress(module: HMODULE, proc_name: *const u8) -> *const c_void;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(wc: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex_style: u32, class_name: *const u16, window_name: *const u16, style: u32,
        x: i32, y: i32, w: i32, h: i32, parent: HWND, menu: HMENU, inst: HINSTANCE, param: *mut c_void,
    ) -> HWND;
    fn AdjustWindowRect(rect: *mut Rect, style: u32, menu: i32) -> i32;
    fn GetClientRect(hwnd: HWND, rect: *mut Rect) -> i32;
    fn GetWindowRect(hwnd: HWND, rect: *mut Rect) -> i32;
    fn GetWindowLongW(hwnd: HWND, index: i32) -> i32;
    fn SetWindowLongW(hwnd: HWND, index: i32, new_long: i32) -> i32;
    fn SetWindowPos(hwnd: HWND, after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    fn MonitorFromWindow(hwnd: HWND, flags: u32) -> HMONITOR;
    fn GetMonitorInfoW(hmon: HMONITOR, lpmi: *mut MonitorInfo) -> i32;
    fn ShowCursor(show: i32) -> i32;
    fn SetWindowTextW(hwnd: HWND, text: *const u16) -> i32;
    fn FillRect(hdc: HDC, lprc: *const Rect, hbr: HBRUSH) -> i32;
    fn GetDC(hwnd: HWND) -> HDC;
    fn PeekMessageW(msg: *mut Msg, hwnd: HWND, min: u32, max: u32, remove: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> LRESULT;
    fn DefWindowProcW(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    fn LoadCursorW(inst: HINSTANCE, name: *const u16) -> HCURSOR;
    fn PostQuitMessage(code: i32);
    fn GetAsyncKeyState(vkey: i32) -> i16;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn SetStretchBltMode(hdc: HDC, mode: i32) -> i32;
    fn GetStockObject(i: i32) -> HBRUSH;
    fn StretchDIBits(
        hdc: HDC, x_dest: i32, y_dest: i32, dest_w: i32, dest_h: i32,
        x_src: i32, y_src: i32, src_w: i32, src_h: i32,
        bits: *const c_void, bmi: *const BitmapInfo, usage: u32, rop: u32,
    ) -> i32;
}

#[link(name = "winmm")]
unsafe extern "system" {
    fn timeBeginPeriod(period: u32) -> u32;
    fn timeEndPeriod(period: u32) -> u32;
    fn waveOutOpen(
        phwo: *mut HWAVEOUT, device_id: u32, pwfx: *const WaveFormatEx,
        callback: usize, instance: usize, flags: u32,
    ) -> u32;
    fn waveOutPrepareHeader(hwo: HWAVEOUT, pwh: *mut WaveHdr, cbwh: u32) -> u32;
    fn waveOutWrite(hwo: HWAVEOUT, pwh: *mut WaveHdr, cbwh: u32) -> u32;
    fn waveOutUnprepareHeader(hwo: HWAVEOUT, pwh: *mut WaveHdr, cbwh: u32) -> u32;
    fn waveOutReset(hwo: HWAVEOUT) -> u32;
    fn waveOutClose(hwo: HWAVEOUT) -> u32;
}

static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CLOSE | WM_DESTROY => {
            QUIT_REQUESTED.store(true, Ordering::SeqCst);
            unsafe { PostQuitMessage(0); }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CrtMode {
    Off,
    Subtle,
    Strong,
}

impl CrtMode {
    fn next(self) -> Self {
        match self {
            CrtMode::Off => CrtMode::Subtle,
            CrtMode::Subtle => CrtMode::Strong,
            CrtMode::Strong => CrtMode::Off,
        }
    }
    fn label(self) -> &'static str {
        match self {
            CrtMode::Off => "CRT: Off",
            CrtMode::Subtle => "CRT: Subtle",
            CrtMode::Strong => "CRT: Classic",
        }
    }
}

struct XInput {
    get_state: Option<XInputGetStateFn>,
    prev_start: [bool; 2],
    prev_back: [bool; 2],
}

impl XInput {
    fn load() -> Self {
        let mut get_state = None;
        for dll in ["xinput1_4.dll", "xinput1_3.dll", "xinput9_1_0.dll"] {
            let w = to_wide(dll);
            let h = unsafe { LoadLibraryW(w.as_ptr()) };
            if !h.is_null() {
                let proc = unsafe { GetProcAddress(h, b"XInputGetState\0".as_ptr()) };
                if !proc.is_null() {
                    get_state = Some(unsafe { std::mem::transmute::<*const c_void, XInputGetStateFn>(proc) });
                    break;
                }
            }
        }
        Self { get_state, prev_start: [false; 2], prev_back: [false; 2] }
    }

    fn poll_pad(&mut self, index: u32, key_cb: &mut dyn FnMut(u8, bool)) -> JoyState {
        let Some(func) = self.get_state else {
            return JoyState::default();
        };
        let mut state = XInputState::default();
        let res = unsafe { func(index, &mut state) };
        if res != 0 {
            return JoyState::default();
        }
        let g = state.gamepad;
        let b = g.w_buttons;

        // Edge-triggered Start -> Amiga P (Pause), Back -> Amiga ESC
        let i = (index as usize).min(1);
        let start_now = (b & XINPUT_START) != 0;
        if start_now != self.prev_start[i] {
            self.prev_start[i] = start_now;
            key_cb(0x19, start_now); // Amiga P
        }
        let back_now = (b & XINPUT_BACK) != 0;
        if back_now != self.prev_back[i] {
            self.prev_back[i] = back_now;
            key_cb(0x45, back_now); // Amiga ESC
        }

        let up = (b & XINPUT_DPAD_UP) != 0 || g.s_thumb_ly > STICK_DEADZONE || (b & (XINPUT_A | XINPUT_Y)) != 0;
        let down = (b & XINPUT_DPAD_DOWN) != 0 || g.s_thumb_ly < -STICK_DEADZONE;
        let left = (b & XINPUT_DPAD_LEFT) != 0 || g.s_thumb_lx < -STICK_DEADZONE;
        let right = (b & XINPUT_DPAD_RIGHT) != 0 || g.s_thumb_lx > STICK_DEADZONE;
        let fire = (b & (XINPUT_X | XINPUT_B | XINPUT_RIGHT_SHOULDER)) != 0 || g.b_right_trigger > 64;

        JoyState { up, down, left, right, fire, fire2: false }
    }
}

pub struct Win32Audio {
    hwo: HWAVEOUT,
    headers: Vec<Box<WaveHdr>>,
    buffers: Vec<Box<[i16; 1764]>>, // 1 PAL frame = 882 stereo frames = 1764 i16 samples
    next_idx: usize,
}

impl Win32Audio {
    pub fn new() -> Option<Self> {
        let wf = WaveFormatEx {
            w_format_tag: WAVE_FORMAT_PCM,
            n_channels: 2,
            n_samples_per_sec: AUDIO_RATE,
            n_avg_bytes_per_sec: AUDIO_RATE * 4,
            n_block_align: 4,
            w_bits_per_sample: 16,
            cb_size: 0,
        };
        let mut hwo: HWAVEOUT = null_mut();
        let res = unsafe { waveOutOpen(&mut hwo, WAVE_MAPPER, &wf, 0, 0, 0) };
        if res != 0 || hwo.is_null() {
            return None;
        }
        let num_bufs = 6;
        let mut headers = Vec::with_capacity(num_bufs);
        let mut buffers = Vec::with_capacity(num_bufs);
        for _ in 0..num_bufs {
            let mut buf = Box::new([0i16; 1764]);
            let mut hdr = Box::new(WaveHdr {
                lp_data: buf.as_mut_ptr() as *mut u8,
                dw_buffer_length: (1764 * 2) as u32,
                dw_bytes_recorded: 0,
                dw_user: 0,
                dw_flags: WHDR_DONE,
                dw_loops: 0,
                lp_next: null_mut(),
                reserved: 0,
            });
            unsafe {
                waveOutPrepareHeader(hwo, &mut *hdr, std::mem::size_of::<WaveHdr>() as u32);
            }
            hdr.dw_flags |= WHDR_DONE;
            headers.push(hdr);
            buffers.push(buf);
        }
        Some(Self { hwo, headers, buffers, next_idx: 0 })
    }

    pub fn submit(&mut self, samples: &[i16]) {
        if samples.is_empty() { return; }
        let hdr = &mut *self.headers[self.next_idx];
        if (hdr.dw_flags & WHDR_DONE) == 0 {
            return;
        }
        let n = samples.len().min(1764);
        self.buffers[self.next_idx][..n].copy_from_slice(&samples[..n]);
        hdr.dw_buffer_length = (n * 2) as u32;
        hdr.dw_flags &= !WHDR_DONE;
        unsafe {
            waveOutWrite(self.hwo, hdr, std::mem::size_of::<WaveHdr>() as u32);
        }
        self.next_idx = (self.next_idx + 1) % self.headers.len();
    }
}

impl Drop for Win32Audio {
    fn drop(&mut self) {
        unsafe {
            waveOutReset(self.hwo);
            for hdr in &mut self.headers {
                waveOutUnprepareHeader(self.hwo, &mut **hdr, std::mem::size_of::<WaveHdr>() as u32);
            }
            waveOutClose(self.hwo);
        }
    }
}

pub struct Win32Window {
    hwnd: HWND,
    hdc: HDC,
    black_brush: HBRUSH,
    audio: Option<Win32Audio>,
    xinput: XInput,
    base_title: String,
    fullscreen: bool,
    windowed_rect: Rect,
    windowed_style: u32,
    crt_mode: CrtMode,
    crt_fb: Vec<u32>,
    last_cw: i32,
    last_ch: i32,
}

impl Win32Window {
    pub fn new(title: &str, scale: i32, start_fullscreen: bool, start_crt: CrtMode) -> Result<Self, String> {
        unsafe { timeBeginPeriod(1); }
        let hinst = unsafe { GetModuleHandleW(null()) };
        let black_brush = unsafe { GetStockObject(4) }; // BLACK_BRUSH
        let class_name = to_wide("ParasolStarsRecompClass");
        let wc = WndClassW {
            style: CS_OWNDC,
            lpfn_wnd_proc: wnd_proc,
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: hinst,
            h_icon: null_mut(),
            h_cursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW as *const u16) },
            hbr_background: black_brush,
            lpsz_menu_name: null(),
            lpsz_class_name: class_name.as_ptr(),
        };
        unsafe { RegisterClassW(&wc); }

        let mut rect = Rect { left: 0, top: 0, right: SCREEN_W as i32 * scale, bottom: SCREEN_H as i32 * scale };
        unsafe { AdjustWindowRect(&mut rect, WS_OVERLAPPEDWINDOW, 0); }
        let title_w = to_wide(title);
        let hwnd = unsafe {
            CreateWindowExW(
                0, class_name.as_ptr(), title_w.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT, CW_USEDEFAULT,
                rect.right - rect.left, rect.bottom - rect.top,
                null_mut(), null_mut(), hinst, null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err("CreateWindowExW failed".into());
        }
        let hdc = unsafe { GetDC(hwnd) };
        let mut wr = Rect::default();
        unsafe { GetWindowRect(hwnd, &mut wr); }

        let mut win = Self {
            hwnd,
            hdc,
            black_brush,
            audio: Win32Audio::new(),
            xinput: XInput::load(),
            base_title: title.to_string(),
            fullscreen: false,
            windowed_rect: wr,
            windowed_style: WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            crt_mode: start_crt,
            crt_fb: vec![0u32; SCREEN_W * SCREEN_H * 2],
            last_cw: 0,
            last_ch: 0,
        };
        win.update_title();
        if start_fullscreen {
            win.toggle_fullscreen();
        }
        Ok(win)
    }

    pub fn has_xinput(&self) -> bool {
        self.xinput.get_state.is_some()
    }

    fn update_title(&self) {
        let full = format!("{} [{} | F11: Fullscreen | F9: Scanlines]", self.base_title, self.crt_mode.label());
        let w = to_wide(&full);
        unsafe { SetWindowTextW(self.hwnd, w.as_ptr()); }
    }

    pub fn toggle_fullscreen(&mut self) {
        if !self.fullscreen {
            unsafe {
                self.windowed_style = GetWindowLongW(self.hwnd, GWL_STYLE) as u32;
                GetWindowRect(self.hwnd, &mut self.windowed_rect);
                let hmon = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
                let mut mi = MonitorInfo {
                    cb_size: std::mem::size_of::<MonitorInfo>() as u32,
                    rc_monitor: Rect::default(),
                    rc_work: Rect::default(),
                    dw_flags: 0,
                };
                if GetMonitorInfoW(hmon, &mut mi) != 0 {
                    SetWindowLongW(self.hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE) as i32);
                    let r = mi.rc_monitor;
                    SetWindowPos(
                        self.hwnd, null_mut(),
                        r.left, r.top, r.right - r.left, r.bottom - r.top,
                        SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
                    );
                    ShowCursor(0);
                    self.fullscreen = true;
                    self.last_cw = 0;
                    self.last_ch = 0;
                }
            }
        } else {
            unsafe {
                SetWindowLongW(self.hwnd, GWL_STYLE, self.windowed_style as i32);
                let r = self.windowed_rect;
                SetWindowPos(
                    self.hwnd, null_mut(),
                    r.left, r.top, r.right - r.left, r.bottom - r.top,
                    SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
                );
                ShowCursor(1);
            }
            self.fullscreen = false;
            self.last_cw = 0;
            self.last_ch = 0;
        }
    }

    pub fn cycle_crt(&mut self) {
        self.crt_mode = self.crt_mode.next();
        self.update_title();
    }

    pub fn poll_events(&mut self, key_cb: &mut dyn FnMut(u8, bool)) -> bool {
        let mut msg = Msg::default();
        unsafe {
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN {
                    let is_repeat = (msg.lparam & (1 << 30)) != 0;
                    let alt_down = (msg.lparam & (1 << 29)) != 0;
                    let vk = msg.wparam as i32;
                    if !is_repeat {
                        if vk == 0x7A || (vk == 0x0D && alt_down) {
                            // F11 or Alt+Enter -> toggle borderless fullscreen
                            self.toggle_fullscreen();
                            continue;
                        }
                        if vk == 0x78 {
                            // F9 -> cycle CRT scanlines (Off / Subtle / Classic)
                            self.cycle_crt();
                            continue;
                        }
                        if let Some(sc) = vk_to_amiga_scancode(vk) {
                            key_cb(sc, true);
                        }
                    }
                } else if msg.message == WM_KEYUP || msg.message == WM_SYSKEYUP {
                    let vk = msg.wparam as i32;
                    if vk != 0x7A && vk != 0x78 {
                        if let Some(sc) = vk_to_amiga_scancode(vk) {
                            key_cb(sc, false);
                        }
                    }
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        !QUIT_REQUESTED.load(Ordering::SeqCst)
    }

    pub fn poll_joysticks(&mut self, key_cb: &mut dyn FnMut(u8, bool)) -> (JoyState, JoyState) {
        let down = |vk: i32| unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 };
        let alt_down = down(0x12);

        // Player 1 = Amiga Joystick Port 2 (JOY1DAT) — Keyboard + Xbox Controller 0
        let kb1 = JoyState {
            up: down(0x26) || down(b'W' as i32) || down(b'X' as i32),
            down: down(0x28) || down(b'S' as i32),
            left: down(0x25) || down(b'A' as i32),
            right: down(0x27) || down(b'D' as i32),
            fire: down(b'Z' as i32) || down(0x20) || down(0x11) || (down(0x0D) && !alt_down),
            fire2: down(b'C' as i32),
        };
        let pad1 = self.xinput.poll_pad(0, key_cb);
        let joy1 = merge_joy(kb1, pad1);

        // Player 2 = Amiga Joystick Port 1 (JOY0DAT) — Keyboard + Xbox Controller 1
        let kb0 = JoyState {
            up: down(b'I' as i32),
            down: down(b'K' as i32),
            left: down(b'J' as i32),
            right: down(b'L' as i32),
            fire: down(b'N' as i32) || down(0xA1),
            fire2: down(b'M' as i32),
        };
        let pad0 = self.xinput.poll_pad(1, key_cb);
        let joy0 = merge_joy(kb0, pad0);

        (joy0, joy1)
    }

    pub fn present(&mut self, fb: &[u32]) {
        let mut rect = Rect::default();
        unsafe { GetClientRect(self.hwnd, &mut rect); }
        let cw = (rect.right - rect.left).max(1);
        let ch = (rect.bottom - rect.top).max(1);

        // Clear black letterbox bars whenever window dimensions change
        if cw != self.last_cw || ch != self.last_ch {
            unsafe { FillRect(self.hdc, &rect, self.black_brush); }
            self.last_cw = cw;
            self.last_ch = ch;
        }

        // Preserve 320:256 aspect ratio centered inside window
        let (dw, dh) = if cw * SCREEN_H as i32 > ch * SCREEN_W as i32 {
            ((ch * SCREEN_W as i32) / SCREEN_H as i32, ch)
        } else {
            (cw, (cw * SCREEN_H as i32) / SCREEN_W as i32)
        };
        let dx = (cw - dw) / 2;
        let dy = (ch - dh) / 2;

        let (src_ptr, src_h) = match self.crt_mode {
            CrtMode::Off => (fb.as_ptr(), SCREEN_H as i32),
            CrtMode::Subtle | CrtMode::Strong => {
                let dark_mul: u32 = if self.crt_mode == CrtMode::Subtle { 172 } else { 104 };
                for y in 0..SCREEN_H {
                    let src_row = &fb[y * SCREEN_W..(y + 1) * SCREEN_W];
                    let dst_bright = (y * 2) * SCREEN_W;
                    let dst_dark = dst_bright + SCREEN_W;
                    self.crt_fb[dst_bright..dst_bright + SCREEN_W].copy_from_slice(src_row);
                    for x in 0..SCREEN_W {
                        let px = src_row[x];
                        let r = (((px >> 16) & 0xFF) * dark_mul) >> 8;
                        let g = (((px >> 8) & 0xFF) * dark_mul) >> 8;
                        let b = ((px & 0xFF) * dark_mul) >> 8;
                        self.crt_fb[dst_dark + x] = (r << 16) | (g << 8) | b;
                    }
                }
                (self.crt_fb.as_ptr(), (SCREEN_H * 2) as i32)
            }
        };

        let bmi = BitmapInfo {
            bmi_header: BitmapInfoHeader {
                bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                bi_width: SCREEN_W as i32,
                bi_height: -src_h, // top-down DIB
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: 0, // BI_RGB
                bi_size_image: 0,
                bi_x_pels_per_meter: 0,
                bi_y_pels_per_meter: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            },
            bmi_colors: [0],
        };
        unsafe {
            SetStretchBltMode(self.hdc, COLORONCOLOR);
            StretchDIBits(
                self.hdc, dx, dy, dw, dh,
                0, 0, SCREEN_W as i32, src_h,
                src_ptr as *const c_void, &bmi, DIB_RGB_COLORS, SRCCOPY,
            );
        }
    }

    pub fn submit_audio(&mut self, samples: &[i16]) {
        if let Some(a) = &mut self.audio {
            a.submit(samples);
        }
    }
}

impl Drop for Win32Window {
    fn drop(&mut self) {
        if self.fullscreen {
            unsafe { ShowCursor(1); }
        }
        unsafe { timeEndPeriod(1); }
    }
}

fn merge_joy(a: JoyState, b: JoyState) -> JoyState {
    JoyState {
        up: a.up || b.up,
        down: a.down || b.down,
        left: a.left || b.left,
        right: a.right || b.right,
        fire: a.fire || b.fire,
        fire2: a.fire2 || b.fire2,
    }
}

fn vk_to_amiga_scancode(vk: i32) -> Option<u8> {
    match vk {
        0x1B => Some(0x45), // ESC
        0x0D => Some(0x44), // RETURN
        0x20 => Some(0x40), // SPACE
        0x08 => Some(0x41), // BACKSPACE
        0x70 => Some(0x50), // F1
        0x71 => Some(0x51), // F2
        0x72 => Some(0x52), // F3
        0x73 => Some(0x53), // F4
        0x74 => Some(0x54), // F5
        0x31 => Some(0x01), // 1
        0x32 => Some(0x02), // 2
        0x33 => Some(0x03), // 3
        0x34 => Some(0x04), // 4
        0x35 => Some(0x05), // 5
        0x50 => Some(0x19), // P (Pause)
        0x4D => Some(0x37), // M (Music toggle)
        0x51 => Some(0x10), // Q
        0x26 => Some(0x4C), // Up
        0x28 => Some(0x4D), // Down
        0x27 => Some(0x4E), // Right
        0x25 => Some(0x4F), // Left
        _ => None,
    }
}
