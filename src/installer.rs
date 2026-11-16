#![windows_subsystem = "windows"]

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
use winreg::RegKey;

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC,
    DeleteObject, DrawTextW, EndPaint, EnumFontFamiliesExW, GdiFlush, GetDC, GetDeviceCaps,
    ReleaseDC, ScreenToClient, SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HFONT,
    LOGFONTW, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::Shell::{
    SHBrowseForFolderW, SHGetFolderPathW, SHGetPathFromIDListW, ShellExecuteW, BROWSEINFOW,
    CSIDL_DESKTOPDIRECTORY, CSIDL_PROGRAMS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetSystemMetrics, GetWindowRect, LoadCursorW, MessageBoxW, PostMessageW, PostQuitMessage,
    RegisterClassExW, SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow, CS_HREDRAW,
    CS_VREDRAW, HTCAPTION, HTCLIENT, IDC_ARROW, IDC_HAND, MB_ICONINFORMATION, MB_OK, MSG,
    SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOW, ULW_ALPHA, WM_APP, WM_CLOSE,
    WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCHITTEST, WM_PAINT, WNDCLASSEXW, WS_EX_LAYERED, WS_POPUP, WS_SYSMENU,
};

#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const GdiplusStartupInput, output: *mut c_void) -> i32;
    fn GdiplusShutdown(token: usize);
    fn GdipCreateBitmapFromScan0(
        width: i32,
        height: i32,
        stride: i32,
        format: i32,
        scan0: *mut u8,
        bitmap: *mut *mut c_void,
    ) -> i32;
    fn GdipGetImageGraphicsContext(image: *mut c_void, graphics: *mut *mut c_void) -> i32;
    fn GdipDisposeImage(image: *mut c_void) -> i32;
    fn GdipDeleteGraphics(graphics: *mut c_void) -> i32;
    fn GdipSetSmoothingMode(graphics: *mut c_void, mode: i32) -> i32;
    fn GdipCreatePath(fill_mode: i32, path: *mut *mut c_void) -> i32;
    fn GdipDeletePath(path: *mut c_void) -> i32;
    fn GdipAddPathArc(
        path: *mut c_void,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        start_angle: f32,
        sweep_angle: f32,
    ) -> i32;
    fn GdipClosePathFigure(path: *mut c_void) -> i32;
    fn GdipCreateSolidFill(color: u32, brush: *mut *mut c_void) -> i32;
    fn GdipDeleteBrush(brush: *mut c_void) -> i32;
    fn GdipFillPath(graphics: *mut c_void, brush: *mut c_void, path: *mut c_void) -> i32;
    fn GdipCreatePen1(color: u32, width: f32, unit: i32, pen: *mut *mut c_void) -> i32;
    fn GdipDeletePen(pen: *mut c_void) -> i32;
    fn GdipDrawPath(graphics: *mut c_void, pen: *mut c_void, path: *mut c_void) -> i32;
    fn GdipDrawLines(
        graphics: *mut c_void,
        pen: *mut c_void,
        points: *const GpPointF,
        count: i32,
    ) -> i32;
    fn GdipSetPenStartCap(pen: *mut c_void, cap: i32) -> i32;
    fn GdipSetPenEndCap(pen: *mut c_void, cap: i32) -> i32;
    fn GdipSetPenLineJoin(pen: *mut c_void, join: i32) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn TrackMouseEvent(lpEventTrack: *mut TrackMouseEventData) -> BOOL;
}

#[link(name = "ole32")]
extern "system" {
    fn CoTaskMemFree(pv: *const c_void);
}

#[repr(C)]
struct GdiplusStartupInput {
    gdiplus_version: u32,
    debug_event_callback: usize,
    suppress_background_thread: i32,
    suppress_external_codecs: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GpPointF {
    x: f32,
    y: f32,
}

#[repr(C)]
struct TrackMouseEventData {
    cb_size: u32,
    dw_flags: u32,
    hwnd_track: HWND,
    dw_hover_time: u32,
}

const CARD_W: f32 = 350.0;
const CARD_H: f32 = 205.0;
const WIN_BLEED: f32 = 2.0;
const TOPBAR_H: f32 = 29.0;
const RADIUS_CARD: f32 = 19.0;
const RADIUS_BTN: f32 = 8.4;
const PIXEL_FORMAT_32BPP_PARGB: i32 = 0x000E_200B;
const WM_DPICHANGED: u32 = 0x02E0;

// Step 1 (Install 1) rects — 1.2x Figma layout
const R_S1_GITHUB: RectF = RectF::new(10.8, 32.4, 328.4, 37.2);
const R_S1_FOLDER: RectF = RectF::new(10.8, 78.0, 37.2, 37.2);
const R_S1_PATH: RectF = RectF::new(55.2, 78.0, 284.0, 37.2);
const R_S1_INSTALL: RectF = RectF::new(10.8, 122.4, 328.4, 72.0);

// Step 2 (Install 2) rects — 1.2x Figma layout
const R_S2_AUTO: RectF = RectF::new(10.8, 31.2, 328.4, 37.2);
const R_S2_AUTO_CB: RectF = RectF::new(20.4, 42.0, 15.6, 15.6);
const R_S2_SHORT: RectF = RectF::new(10.8, 75.6, 328.4, 37.2);
const R_S2_SHORT_CB: RectF = RectF::new(20.4, 86.4, 15.6, 15.6);
const R_S2_INSTALL: RectF = RectF::new(10.8, 120.0, 328.4, 74.4);

// Step 3 (Install 3) & Step 4 (Install 4) rects — 1.2x Figma layout
const R_MID_ROW: RectF = RectF::new(10.8, 75.6, 328.4, 37.2);
const R_BOTTOM_BTN: RectF = RectF::new(10.8, 120.0, 328.4, 74.4);
const R_DONE_STATUS: RectF = RectF::new(10.8, 35.0, 328.4, 30.0);

const DT_LEFT: u32 = 0x0000_0000;
const DT_CENTER: u32 = 0x0000_0001;
const DT_VCENTER: u32 = 0x0000_0004;
const DT_SINGLELINE: u32 = 0x0000_0020;
const DT_PATH_ELLIPSIS: u32 = 0x0000_4000;
const DT_NOPREFIX: u32 = 0x0000_0800;

const WM_MOUSELEAVE: u32 = 0x02A3;
const TME_LEAVE: u32 = 0x0000_0002;
const VK_ESCAPE: usize = 0x1B;

const WM_INSTALL_PROGRESS: u32 = WM_APP + 1;
const WM_INSTALL_DONE: u32 = WM_APP + 2;

const BIF_RETURNONLYFSDIRS: u32 = 0x0000_0001;
const BIF_NEWDIALOGSTYLE: u32 = 0x0000_0040;

const FREEVIEW_EXE_BYTES: &[u8] = include_bytes!("../target/release/freeview.exe");
const WEBVIEW2_LOADER_BYTES: &[u8] = include_bytes!("../target/release/WebView2Loader.dll");
const LOGO_ICO_BYTES: &[u8] = include_bytes!("../logo.ico");

const GITHUB_REPO_URL: &str = "https://github.com/knoblab/freeview-sni";
const UNINSTALL_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\FreeView";
const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// GDI+ ARGB Colors (0xAARRGGBB)
const ARGB_BG: u32 = 0xFFF7_F6F2;
const ARGB_CARD_BORDER: u32 = 0x1400_0000;
const ARGB_BTN_SEC: u32 = 0xFFE7_E6E1;
const ARGB_BTN_SEC_HOVER: u32 = 0xFFDC_DBD5;
const ARGB_BTN_SEC_DOWN: u32 = 0xFFD1_D0C9;
const ARGB_FOLDER_BG: u32 = 0xFFC2_C1BB;
const ARGB_FOLDER_HOVER: u32 = 0xFFB5_B4AE;
const ARGB_FOLDER_DOWN: u32 = 0xFFA8_A7A1;
const ARGB_TEAL: u32 = 0xFF45_707A;
const ARGB_TEAL_HOVER: u32 = 0xFF3C_626B;
const ARGB_TEAL_DOWN: u32 = 0xFF34_565E;
const ARGB_MUTED: u32 = 0xFF88_8783;
const ARGB_CHECK_OFF: u32 = 0xFFC2_C1BB;

// Win32 GDI Text COLORREFs (0x00BBGGRR)
const CREF_BG: u32 = 0x00F2_F6F7;
const CREF_TITLE: u32 = 0x0000_0000;
const CREF_SEC_TEXT: u32 = 0x0056_5353;
const CREF_LIGHT_TEXT: u32 = 0x00F2_F6F7;

struct UiThreadCell<T>(UnsafeCell<Option<T>>);
unsafe impl<T> Sync for UiThreadCell<T> {}

impl<T> UiThreadCell<T> {
    const fn new() -> Self {
        Self(UnsafeCell::new(None))
    }

    unsafe fn set(&self, val: T) {
        *self.0.get() = Some(val);
    }

    unsafe fn as_ref(&self) -> Option<&T> {
        (*self.0.get()).as_ref()
    }

    unsafe fn as_mut(&self) -> Option<&mut T> {
        (*self.0.get()).as_mut()
    }

    unsafe fn take(&self) -> Option<T> {
        (*self.0.get()).take()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WizardStep {
    Welcome,    // Install 1
    Options,    // Install 2
    Installing, // Install 3
    Done,       // Install 4
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HitId {
    None,
    Github,
    BrowseFolder,
    PathBox,
    InstallStep1,
    OptAutostart,
    OptShortcuts,
    InstallStep2,
    DoneClose,
    DoneLaunch,
}

#[derive(Clone, Copy)]
struct RectF {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl RectF {
    const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }

    fn to_rect(&self) -> RECT {
        RECT {
            left: self.x.round() as i32,
            top: self.y.round() as i32,
            right: (self.x + self.w).round() as i32,
            bottom: (self.y + self.h).round() as i32,
        }
    }
}

struct WizardState {
    step: WizardStep,
    install_dir: PathBuf,
    opt_autostart: bool,
    opt_shortcuts: bool,
    progress: u32,
    error_msg: Option<String>,
    hover: HitId,
    pressed: HitId,
    tracking_leave: bool,
}

struct WizardFonts {
    dpi: u32,
    family: String,
    font_14: HFONT,
    font_24: HFONT,
}

static STATE: UiThreadCell<WizardState> = UiThreadCell::new();
static FONTS: UiThreadCell<WizardFonts> = UiThreadCell::new();
static INSTALL_OK: AtomicBool = AtomicBool::new(false);
static PROGRESS_VAL: AtomicU32 = AtomicU32::new(0);
static INSTALL_ERR: UiThreadCell<String> = UiThreadCell::new();

fn main() {
    enable_dpi_awareness();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--uninstall") {
        do_uninstall_gui();
    } else {
        run_install_wizard();
    }
}

fn enable_dpi_awareness() {
    unsafe {
        let user32_name = to_wide_null("user32.dll");
        let h_user32 = GetModuleHandleW(user32_name.as_ptr());
        if !h_user32.is_null() {
            let proc = GetProcAddress(h_user32, b"SetProcessDpiAwarenessContext\0".as_ptr());
            if let Some(f) = proc {
                type FnSetDpiCtx = unsafe extern "system" fn(isize) -> BOOL;
                let set_ctx: FnSetDpiCtx = std::mem::transmute(f);
                // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
                if set_ctx(-4) != 0 {
                    return;
                }
                // Fallback: DPI_AWARENESS_CONTEXT_SYSTEM_AWARE = -2
                if set_ctx(-2) != 0 {
                    return;
                }
            }
            let proc_legacy = GetProcAddress(h_user32, b"SetProcessDPIAware\0".as_ptr());
            if let Some(f) = proc_legacy {
                type FnLegacy = unsafe extern "system" fn() -> BOOL;
                let set_legacy: FnLegacy = std::mem::transmute(f);
                let _ = set_legacy();
            }
        }
    }
}

unsafe fn get_window_dpi(hwnd: HWND) -> u32 {
    let user32_name = to_wide_null("user32.dll");
    let h_user32 = GetModuleHandleW(user32_name.as_ptr());
    if !h_user32.is_null() && !hwnd.is_null() {
        if let Some(f) = GetProcAddress(h_user32, b"GetDpiForWindow\0".as_ptr()) {
            type FnGetDpi = unsafe extern "system" fn(HWND) -> u32;
            let get_dpi: FnGetDpi = std::mem::transmute(f);
            let dpi = get_dpi(hwnd);
            if dpi > 0 {
                return dpi;
            }
        }
    }
    let hdc = GetDC(hwnd);
    let dpi = GetDeviceCaps(hdc, 88) as u32; // LOGPIXELSX
    ReleaseDC(hwnd, hdc);
    if dpi > 0 { dpi } else { 96 }
}

fn scale_rect(r: RectF, s: f32) -> RectF {
    RectF::new((r.x + WIN_BLEED) * s, (r.y + WIN_BLEED) * s, r.w * s, r.h * s)
}

fn phys_win_size(dpi: u32) -> (i32, i32) {
    let s = dpi as f32 / 96.0;
    (
        ((CARD_W + WIN_BLEED * 2.0) * s).ceil() as i32,
        ((CARD_H + WIN_BLEED * 2.0) * s).ceil() as i32,
    )
}

unsafe fn rebuild_fonts_for_dpi(dpi: u32) {
    let s = dpi as f32 / 96.0;
    let family = if let Some(prev) = FONTS.take() {
        DeleteObject(prev.font_14);
        DeleteObject(prev.font_24);
        prev.family
    } else {
        let hdc = GetDC(std::ptr::null_mut());
        let name = if is_font_installed(hdc, "IBM Plex Sans KR") {
            "IBM Plex Sans KR"
        } else {
            "Malgun Gothic"
        };
        ReleaseDC(std::ptr::null_mut(), hdc);
        name.to_string()
    };
    FONTS.set(WizardFonts {
        dpi,
        font_14: create_font((14.0 * s).round() as i32, 400, &family),
        font_24: create_font((24.0 * s).round() as i32, 400, &family),
        family,
    });
}

unsafe extern "system" fn font_enum_proc(
    _lpelfe: *const LOGFONTW,
    _lpntme: *const c_void,
    _font_type: u32,
    lparam: LPARAM,
) -> i32 {
    let found = &mut *(lparam as *mut bool);
    *found = true;
    0
}

unsafe fn is_font_installed(hdc: HDC, name: &str) -> bool {
    let mut lf: LOGFONTW = std::mem::zeroed();
    lf.lfCharSet = 1; // DEFAULT_CHARSET
    let wide = to_wide_null(name);
    let copy_len = wide.len().min(lf.lfFaceName.len());
    lf.lfFaceName[..copy_len].copy_from_slice(&wide[..copy_len]);
    let mut found = false;
    EnumFontFamiliesExW(
        hdc,
        &lf,
        Some(std::mem::transmute(font_enum_proc as *const ())),
        &mut found as *mut bool as LPARAM,
        0,
    );
    found
}

fn run_install_wizard() {
    unsafe {
        let mut gdiplus_token: usize = 0;
        let startup = GdiplusStartupInput {
            gdiplus_version: 1,
            debug_event_callback: 0,
            suppress_background_thread: 0,
            suppress_external_codecs: 0,
        };
        let _ = GdiplusStartup(&mut gdiplus_token, &startup, std::ptr::null_mut());

        let h_instance = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide_null("FreeViewFigmaInstaller");
        let title = to_wide_null("FreeView 3.3.1");
        let brush_bg = CreateSolidBrush(CREF_BG);

        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wizard_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_instance,
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: brush_bg,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        RegisterClassExW(&wnd_class);

        let initial_dpi = get_window_dpi(std::ptr::null_mut());
        let (init_w, init_h) = phys_win_size(initial_dpi);
        let pos_x = ((GetSystemMetrics(SM_CXSCREEN) - init_w) / 2).max(0);
        let pos_y = ((GetSystemMetrics(SM_CYSCREEN) - init_h) / 2).max(0);

        STATE.set(WizardState {
            step: WizardStep::Welcome,
            install_dir: get_default_install_dir(),
            opt_autostart: true,
            opt_shortcuts: true,
            progress: 0,
            error_msg: None,
            hover: HitId::None,
            pressed: HitId::None,
            tracking_leave: false,
        });

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_SYSMENU,
            pos_x,
            pos_y,
            init_w,
            init_h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            h_instance,
            std::ptr::null(),
        );

        let win_dpi = get_window_dpi(hwnd);
        let (phys_w, phys_h) = phys_win_size(win_dpi);
        if phys_w != init_w || phys_h != init_h {
            let cx = ((GetSystemMetrics(SM_CXSCREEN) - phys_w) / 2).max(0);
            let cy = ((GetSystemMetrics(SM_CYSCREEN) - phys_h) / 2).max(0);
            SetWindowPos(hwnd, std::ptr::null_mut(), cx, cy, phys_w, phys_h, SWP_NOZORDER | SWP_NOACTIVATE);
        }

        rebuild_fonts_for_dpi(win_dpi);
        render_layered(hwnd);
        ShowWindow(hwnd, SW_SHOW);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if let Some(f) = FONTS.take() {
            DeleteObject(f.font_14);
            DeleteObject(f.font_24);
        }
        let _ = STATE.take();
        DeleteObject(brush_bg);
        if gdiplus_token != 0 {
            GdiplusShutdown(gdiplus_token);
        }
    }
}

unsafe fn create_font(size: i32, weight: i32, name: &str) -> HFONT {
    CreateFontW(
        -size,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        1,
        4, // OUT_TT_PRECIS
        0,
        5, // CLEARTYPE_QUALITY
        0,
        to_wide_null(name).as_ptr(),
    )
}

fn hit_test_control(state: &WizardState, x: f32, y: f32) -> HitId {
    match state.step {
        WizardStep::Welcome => {
            if R_S1_GITHUB.contains(x, y) {
                HitId::Github
            } else if R_S1_FOLDER.contains(x, y) {
                HitId::BrowseFolder
            } else if R_S1_PATH.contains(x, y) {
                HitId::PathBox
            } else if R_S1_INSTALL.contains(x, y) {
                HitId::InstallStep1
            } else {
                HitId::None
            }
        }
        WizardStep::Options => {
            if R_S2_AUTO.contains(x, y) {
                HitId::OptAutostart
            } else if R_S2_SHORT.contains(x, y) {
                HitId::OptShortcuts
            } else if R_S2_INSTALL.contains(x, y) {
                HitId::InstallStep2
            } else {
                HitId::None
            }
        }
        WizardStep::Installing => HitId::None,
        WizardStep::Done => {
            if R_MID_ROW.contains(x, y) {
                HitId::DoneClose
            } else if R_BOTTOM_BTN.contains(x, y) {
                HitId::DoneLaunch
            } else {
                HitId::None
            }
        }
    }
}

unsafe fn create_rounded_rect_path(r: RectF, radius: f32) -> *mut c_void {
    let mut path: *mut c_void = std::ptr::null_mut();
    GdipCreatePath(0, &mut path);
    let d = (radius * 2.0).min(r.w).min(r.h);
    if d <= 0.5 {
        return path;
    }
    GdipAddPathArc(path, r.x, r.y, d, d, 180.0, 90.0);
    GdipAddPathArc(path, r.x + r.w - d, r.y, d, d, 270.0, 90.0);
    GdipAddPathArc(path, r.x + r.w - d, r.y + r.h - d, d, d, 0.0, 90.0);
    GdipAddPathArc(path, r.x, r.y + r.h - d, d, d, 90.0, 90.0);
    GdipClosePathFigure(path);
    path
}

unsafe fn fill_round_rect(g: *mut c_void, r: RectF, radius: f32, argb: u32) {
    let path = create_rounded_rect_path(r, radius);
    let mut brush: *mut c_void = std::ptr::null_mut();
    GdipCreateSolidFill(argb, &mut brush);
    GdipFillPath(g, brush, path);
    GdipDeleteBrush(brush);
    GdipDeletePath(path);
}

unsafe fn stroke_round_rect(g: *mut c_void, r: RectF, radius: f32, argb: u32, width: f32) {
    let path = create_rounded_rect_path(r, radius);
    let mut pen: *mut c_void = std::ptr::null_mut();
    GdipCreatePen1(argb, width, 2, &mut pen);
    GdipDrawPath(g, pen, path);
    GdipDeletePen(pen);
    GdipDeletePath(path);
}

unsafe fn draw_folder_icon(g: *mut c_void, btn_rect: RectF, s: f32) {
    // 4:20 inside 6:51 svg icon centered in 37.2x37.2 square (1.2x * DPI scale)
    let cx = btn_rect.x + btn_rect.w * 0.5;
    let cy = btn_rect.y + btn_rect.h * 0.5;
    let mut pen: *mut c_void = std::ptr::null_mut();
    GdipCreatePen1(0xFFF7_F6F2, 1.8 * s, 2, &mut pen);
    GdipSetPenStartCap(pen, 2);
    GdipSetPenEndCap(pen, 2);
    GdipSetPenLineJoin(pen, 2);

    let pts = [
        GpPointF { x: cx - 8.0 * s, y: cy - 6.0 * s },
        GpPointF { x: cx - 2.4 * s, y: cy - 6.0 * s },
        GpPointF { x: cx - 0.4 * s, y: cy - 3.6 * s },
        GpPointF { x: cx + 8.0 * s, y: cy - 3.6 * s },
        GpPointF { x: cx + 8.0 * s, y: cy + 6.0 * s },
        GpPointF { x: cx - 8.0 * s, y: cy + 6.0 * s },
        GpPointF { x: cx - 8.0 * s, y: cy - 6.0 * s },
    ];
    GdipDrawLines(g, pen, pts.as_ptr(), pts.len() as i32);
    GdipDeletePen(pen);
}

unsafe fn draw_checkmark(g: *mut c_void, box_rect: RectF, s: f32) {
    let mut pen: *mut c_void = std::ptr::null_mut();
    GdipCreatePen1(0xFFF7_F6F2, 1.8 * s, 2, &mut pen);
    GdipSetPenStartCap(pen, 2);
    GdipSetPenEndCap(pen, 2);
    GdipSetPenLineJoin(pen, 2);

    let pts = [
        GpPointF {
            x: box_rect.x + box_rect.w * 0.24,
            y: box_rect.y + box_rect.h * 0.52,
        },
        GpPointF {
            x: box_rect.x + box_rect.w * 0.44,
            y: box_rect.y + box_rect.h * 0.72,
        },
        GpPointF {
            x: box_rect.x + box_rect.w * 0.78,
            y: box_rect.y + box_rect.h * 0.30,
        },
    ];
    GdipDrawLines(g, pen, pts.as_ptr(), pts.len() as i32);
    GdipDeletePen(pen);
}

struct PendingText<'a> {
    font: HFONT,
    color: u32,
    rect: RectF,
    text: &'a str,
    flags: u32,
}

unsafe fn draw_gdi_text_with_alpha(
    hdc: HDC,
    bits: *mut u32,
    w: i32,
    h: i32,
    item: &PendingText<'_>,
) {
    SelectObject(hdc, item.font as _);
    SetTextColor(hdc, item.color);
    let mut rc = item.rect.to_rect();
    let wide = to_wide_null(item.text);
    DrawTextW(hdc, wide.as_ptr(), -1, &mut rc, item.flags | DT_NOPREFIX);
    GdiFlush();

    // ClearType GDI DrawTextW zeroes the alpha byte of touched pixels on a 32bpp DIB.
    // Restore 0xFF alpha across the strictly interior opaque text box.
    let x0 = rc.left.max(0);
    let x1 = rc.right.min(w);
    let y0 = rc.top.max(0);
    let y1 = rc.bottom.min(h);
    for y in y0..y1 {
        let row = bits.add((y * w) as usize);
        for x in x0..x1 {
            *row.add(x as usize) |= 0xFF00_0000;
        }
    }
}

fn pick_btn_bg(state: &WizardState, id: HitId, base: u32, hover: u32, down: u32) -> u32 {
    if state.pressed == id && state.hover == id {
        down
    } else if state.hover == id {
        hover
    } else {
        base
    }
}

unsafe fn render_layered(hwnd: HWND) {
    let Some(state) = STATE.as_ref() else {
        return;
    };
    let Some(fonts) = FONTS.as_ref() else {
        return;
    };

    let s = fonts.dpi as f32 / 96.0;
    let (w, h) = phys_win_size(fonts.dpi);

    let hdc_screen = GetDC(std::ptr::null_mut());
    let mem_dc = CreateCompatibleDC(hdc_screen);
    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h; // Top-down DIB
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let mut bits_raw: *mut c_void = std::ptr::null_mut();
    let mem_bmp: HBITMAP = CreateDIBSection(
        hdc_screen,
        &bmi,
        DIB_RGB_COLORS,
        &mut bits_raw,
        std::ptr::null_mut(),
        0,
    );
    if mem_bmp.is_null() || bits_raw.is_null() {
        DeleteDC(mem_dc);
        ReleaseDC(std::ptr::null_mut(), hdc_screen);
        return;
    }

    std::ptr::write_bytes(bits_raw, 0, (w * h) as usize * 4);

    // Stage 1: Draw anti-aliased rounded card & controls directly into 32bpp PARGB DIB bytes.
    let mut gp_bmp: *mut c_void = std::ptr::null_mut();
    let mut g: *mut c_void = std::ptr::null_mut();
    if GdipCreateBitmapFromScan0(w, h, w * 4, PIXEL_FORMAT_32BPP_PARGB, bits_raw as *mut u8, &mut gp_bmp) == 0
        && GdipGetImageGraphicsContext(gp_bmp, &mut g) == 0
    {
        GdipSetSmoothingMode(g, 4); // SmoothingModeAntiAlias

        let card = scale_rect(RectF::new(0.0, 0.0, CARD_W, CARD_H), s);
        fill_round_rect(g, card, RADIUS_CARD * s, ARGB_BG);
        stroke_round_rect(
            g,
            RectF::new(
                card.x + 0.5 * s,
                card.y + 0.5 * s,
                card.w - 1.0 * s,
                card.h - 1.0 * s,
            ),
            (RADIUS_CARD - 0.5) * s,
            ARGB_CARD_BORDER,
            1.0 * s,
        );

        let btn_r = RADIUS_BTN * s;
        match state.step {
            WizardStep::Welcome => {
                let bg_gh = pick_btn_bg(state, HitId::Github, ARGB_BTN_SEC, ARGB_BTN_SEC_HOVER, ARGB_BTN_SEC_DOWN);
                fill_round_rect(g, scale_rect(R_S1_GITHUB, s), btn_r, bg_gh);

                let bg_dir = pick_btn_bg(state, HitId::BrowseFolder, ARGB_FOLDER_BG, ARGB_FOLDER_HOVER, ARGB_FOLDER_DOWN);
                let r_dir = scale_rect(R_S1_FOLDER, s);
                fill_round_rect(g, r_dir, btn_r, bg_dir);
                draw_folder_icon(g, r_dir, s);

                let bg_path = pick_btn_bg(state, HitId::PathBox, ARGB_BTN_SEC, ARGB_BTN_SEC_HOVER, ARGB_BTN_SEC_DOWN);
                fill_round_rect(g, scale_rect(R_S1_PATH, s), btn_r, bg_path);

                let bg_inst = pick_btn_bg(state, HitId::InstallStep1, ARGB_TEAL, ARGB_TEAL_HOVER, ARGB_TEAL_DOWN);
                fill_round_rect(g, scale_rect(R_S1_INSTALL, s), btn_r, bg_inst);
            }
            WizardStep::Options => {
                let bg_auto = pick_btn_bg(state, HitId::OptAutostart, ARGB_BTN_SEC, ARGB_BTN_SEC_HOVER, ARGB_BTN_SEC_DOWN);
                fill_round_rect(g, scale_rect(R_S2_AUTO, s), btn_r, bg_auto);
                let cb_auto = scale_rect(R_S2_AUTO_CB, s);
                fill_round_rect(g, cb_auto, 2.4 * s, if state.opt_autostart { ARGB_TEAL } else { ARGB_CHECK_OFF });
                if state.opt_autostart {
                    draw_checkmark(g, cb_auto, s);
                }

                let bg_short = pick_btn_bg(state, HitId::OptShortcuts, ARGB_BTN_SEC, ARGB_BTN_SEC_HOVER, ARGB_BTN_SEC_DOWN);
                fill_round_rect(g, scale_rect(R_S2_SHORT, s), btn_r, bg_short);
                let cb_short = scale_rect(R_S2_SHORT_CB, s);
                fill_round_rect(g, cb_short, 2.4 * s, if state.opt_shortcuts { ARGB_TEAL } else { ARGB_CHECK_OFF });
                if state.opt_shortcuts {
                    draw_checkmark(g, cb_short, s);
                }

                let bg_inst = pick_btn_bg(state, HitId::InstallStep2, ARGB_TEAL, ARGB_TEAL_HOVER, ARGB_TEAL_DOWN);
                fill_round_rect(g, scale_rect(R_S2_INSTALL, s), btn_r, bg_inst);
            }
            WizardStep::Installing => {
                let track = scale_rect(R_MID_ROW, s);
                fill_round_rect(g, track, btn_r, ARGB_BTN_SEC);
                let pct = state.progress.min(100);
                if pct > 0 {
                    let fill_w = (track.w * (pct as f32 / 100.0)).max(16.8 * s);
                    fill_round_rect(g, RectF::new(track.x, track.y, fill_w, track.h), btn_r, ARGB_TEAL);
                }
                fill_round_rect(g, scale_rect(R_BOTTOM_BTN, s), btn_r, ARGB_MUTED);
            }
            WizardStep::Done => {
                let bg_close = pick_btn_bg(state, HitId::DoneClose, ARGB_BTN_SEC, ARGB_BTN_SEC_HOVER, ARGB_BTN_SEC_DOWN);
                fill_round_rect(g, scale_rect(R_MID_ROW, s), btn_r, bg_close);

                let bg_launch = pick_btn_bg(state, HitId::DoneLaunch, ARGB_TEAL, ARGB_TEAL_HOVER, ARGB_TEAL_DOWN);
                fill_round_rect(g, scale_rect(R_BOTTOM_BTN, s), btn_r, bg_launch);
            }
        }
    }
    if !g.is_null() {
        GdipDeleteGraphics(g);
    }
    if !gp_bmp.is_null() {
        GdipDisposeImage(gp_bmp);
    }

    // Stage 2: Draw ClearType GDI text onto the same DIB with alpha restoration inside text rects.
    let old_bmp = SelectObject(mem_dc, mem_bmp as _);
    SetBkMode(mem_dc, 1);
    let bits_u32 = bits_raw as *mut u32;

    let topbar_text_rect = scale_rect(RectF::new(40.0, 1.5, CARD_W - 80.0, TOPBAR_H - 1.5), s);
    draw_gdi_text_with_alpha(
        mem_dc,
        bits_u32,
        w,
        h,
        &PendingText {
            font: fonts.font_14,
            color: CREF_TITLE,
            rect: topbar_text_rect,
            text: "FreeView 3.3.1",
            flags: DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        },
    );

    match state.step {
        WizardStep::Welcome => {
            draw_gdi_text_with_alpha(
                mem_dc,
                bits_u32,
                w,
                h,
                &PendingText {
                    font: fonts.font_14,
                    color: CREF_SEC_TEXT,
                    rect: scale_rect(R_S1_GITHUB, s),
                    text: "깃허브",
                    flags: DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                },
            );
            let path_text = state.install_dir.to_string_lossy();
            let r_path_text = scale_rect(
                RectF::new(R_S1_PATH.x + 12.0, R_S1_PATH.y, R_S1_PATH.w - 24.0, R_S1_PATH.h),
                s,
            );
            draw_gdi_text_with_alpha(
                mem_dc,
                bits_u32,
                w,
                h,
                &PendingText {
                    font: fonts.font_14,
                    color: CREF_SEC_TEXT,
                    rect: r_path_text,
                    text: &path_text,
                    flags: DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS,
                },
            );
            draw_gdi_text_with_alpha(
                mem_dc,
                bits_u32,
                w,
                h,
                &PendingText {
                    font: fonts.font_24,
                    color: CREF_LIGHT_TEXT,
                    rect: scale_rect(R_S1_INSTALL, s),
                    text: "설치",
                    flags: DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                },
            );
        }
        WizardStep::Options => {
            draw_gdi_text_with_alpha(
                mem_dc,
                bits_u32,
                w,
                h,
                &PendingText {
                    font: fonts.font_14,
                    color: CREF_SEC_TEXT,
                    rect: scale_rect(R_S2_AUTO, s),
                    text: "시작 시 자동 실행",
                    flags: DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                },
            );
            draw_gdi_text_with_alpha(
                mem_dc,
                bits_u32,
                w,
                h,
                &PendingText {
                    font: fonts.font_14,
                    color: CREF_SEC_TEXT,
                    rect: scale_rect(R_S2_SHORT, s),
                    text: "바로가기 만들기",
                    flags: DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                },
            );
            draw_gdi_text_with_alpha(
                mem_dc,
                bits_u32,
                w,
                h,
                &PendingText {
                    font: fonts.font_24,
                    color: CREF_LIGHT_TEXT,
                    rect: scale_rect(R_S2_INSTALL, s),
                    text: "설치",
                    flags: DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                },
            );
        }
        WizardStep::Installing => {
            draw_gdi_text_with_alpha(
                mem_dc,
                bits_u32,
                w,
                h,
                &PendingText {
                    font: fonts.font_24,
                    color: CREF_LIGHT_TEXT,
                    rect: scale_rect(R_BOTTOM_BTN, s),
                    text: "설치중...",
                    flags: DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                },
            );
        }
        WizardStep::Done => {
            let status_text = state.error_msg.as_deref().unwrap_or("설치 완료!");
            draw_gdi_text_with_alpha(
                mem_dc,
                bits_u32,
                w,
                h,
                &PendingText {
                    font: fonts.font_14,
                    color: CREF_SEC_TEXT,
                    rect: scale_rect(R_DONE_STATUS, s),
                    text: status_text,
                    flags: DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS,
                },
            );
            draw_gdi_text_with_alpha(
                mem_dc,
                bits_u32,
                w,
                h,
                &PendingText {
                    font: fonts.font_14,
                    color: CREF_SEC_TEXT,
                    rect: scale_rect(R_MID_ROW, s),
                    text: "닫기",
                    flags: DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                },
            );
            let primary_label = if state.error_msg.is_some() { "다시 시도" } else { "실행" };
            draw_gdi_text_with_alpha(
                mem_dc,
                bits_u32,
                w,
                h,
                &PendingText {
                    font: fonts.font_24,
                    color: CREF_LIGHT_TEXT,
                    rect: scale_rect(R_BOTTOM_BTN, s),
                    text: primary_label,
                    flags: DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                },
            );
        }
    }

    let mut rc_win: RECT = std::mem::zeroed();
    GetWindowRect(hwnd, &mut rc_win);
    let pt_dst = POINT { x: rc_win.left, y: rc_win.top };
    let size = SIZE { cx: w, cy: h };
    let pt_src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    UpdateLayeredWindow(
        hwnd,
        std::ptr::null_mut(),
        &pt_dst,
        &size,
        mem_dc,
        &pt_src,
        0,
        &blend,
        ULW_ALPHA,
    );

    SelectObject(mem_dc, old_bmp);
    DeleteObject(mem_bmp as _);
    DeleteDC(mem_dc);
    ReleaseDC(std::ptr::null_mut(), hdc_screen);
}

unsafe fn phys_to_logical(px: i32, py: i32) -> (f32, f32) {
    let s = FONTS.as_ref().map(|f| f.dpi as f32 / 96.0).unwrap_or(1.0);
    ((px as f32) / s - WIN_BLEED, (py as f32) / s - WIN_BLEED)
}

unsafe extern "system" fn wizard_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,

        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            BeginPaint(hwnd, &mut ps);
            EndPaint(hwnd, &ps);
            0
        }

        WM_DPICHANGED => {
            let new_dpi = ((wparam as u32) & 0xFFFF).max(96);
            rebuild_fonts_for_dpi(new_dpi);
            let suggested = &*(lparam as *const RECT);
            let (phys_w, phys_h) = phys_win_size(new_dpi);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                suggested.left,
                suggested.top,
                phys_w,
                phys_h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            render_layered(hwnd);
            0
        }

        WM_NCHITTEST => {
            let mut pt = POINT {
                x: (lparam as i32) as i16 as i32,
                y: ((lparam as i32) >> 16) as i16 as i32,
            };
            ScreenToClient(hwnd, &mut pt);
            let (_, ly) = phys_to_logical(pt.x, pt.y);
            if ly >= -WIN_BLEED && ly < TOPBAR_H {
                HTCAPTION as LRESULT
            } else {
                HTCLIENT as LRESULT
            }
        }

        WM_MOUSEMOVE => {
            let px = ((lparam as i32) & 0xFFFF) as i16 as i32;
            let py = (((lparam as i32) >> 16) & 0xFFFF) as i16 as i32;
            let (x, y) = phys_to_logical(px, py);
            if let Some(state) = STATE.as_mut() {
                if !state.tracking_leave {
                    let mut tme = TrackMouseEventData {
                        cb_size: std::mem::size_of::<TrackMouseEventData>() as u32,
                        dw_flags: TME_LEAVE,
                        hwnd_track: hwnd,
                        dw_hover_time: 0,
                    };
                    TrackMouseEvent(&mut tme);
                    state.tracking_leave = true;
                }
                let next_hover = hit_test_control(state, x, y);
                if next_hover != state.hover {
                    state.hover = next_hover;
                    render_layered(hwnd);
                }
                let cursor_id = if next_hover != HitId::None { IDC_HAND } else { IDC_ARROW };
                windows_sys::Win32::UI::WindowsAndMessaging::SetCursor(LoadCursorW(std::ptr::null_mut(), cursor_id));
            }
            0
        }

        WM_MOUSELEAVE => {
            if let Some(state) = STATE.as_mut() {
                state.tracking_leave = false;
                if state.hover != HitId::None {
                    state.hover = HitId::None;
                    render_layered(hwnd);
                }
            }
            0
        }

        WM_LBUTTONDOWN => {
            let px = ((lparam as i32) & 0xFFFF) as i16 as i32;
            let py = (((lparam as i32) >> 16) & 0xFFFF) as i16 as i32;
            let (x, y) = phys_to_logical(px, py);
            if let Some(state) = STATE.as_mut() {
                let hit = hit_test_control(state, x, y);
                if hit != HitId::None {
                    state.pressed = hit;
                    SetCapture(hwnd);
                    render_layered(hwnd);
                }
            }
            0
        }

        WM_LBUTTONUP => {
            let px = ((lparam as i32) & 0xFFFF) as i16 as i32;
            let py = (((lparam as i32) >> 16) & 0xFFFF) as i16 as i32;
            let (x, y) = phys_to_logical(px, py);
            ReleaseCapture();

            let mut action = HitId::None;
            if let Some(state) = STATE.as_mut() {
                let hit = hit_test_control(state, x, y);
                if state.pressed != HitId::None && state.pressed == hit {
                    action = hit;
                }
                state.pressed = HitId::None;
                state.hover = hit;
            }

            if action != HitId::None {
                handle_click(hwnd, action);
            }
            render_layered(hwnd);
            0
        }

        WM_KEYDOWN => {
            if wparam == VK_ESCAPE {
                if let Some(state) = STATE.as_ref() {
                    if state.step != WizardStep::Installing {
                        DestroyWindow(hwnd);
                    }
                }
            }
            0
        }

        WM_INSTALL_PROGRESS => {
            if let Some(state) = STATE.as_mut() {
                state.progress = PROGRESS_VAL.load(Ordering::SeqCst);
            }
            render_layered(hwnd);
            0
        }

        WM_INSTALL_DONE => {
            if let Some(state) = STATE.as_mut() {
                state.step = WizardStep::Done;
                state.progress = 100;
                state.error_msg = if INSTALL_OK.load(Ordering::SeqCst) {
                    None
                } else {
                    Some(
                        INSTALL_ERR
                            .take()
                            .unwrap_or_else(|| "설치 중 오류가 발생했습니다.".to_string()),
                    )
                };
                // Refresh hover after screen transition
                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);
                ScreenToClient(hwnd, &mut pt);
                let (lx, ly) = phys_to_logical(pt.x, pt.y);
                state.hover = hit_test_control(state, lx, ly);
            }
            render_layered(hwnd);
            0
        }

        WM_CLOSE => {
            if let Some(state) = STATE.as_ref() {
                if state.step == WizardStep::Installing {
                    return 0;
                }
            }
            DestroyWindow(hwnd);
            0
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn handle_click(hwnd: HWND, id: HitId) {
    match id {
        HitId::Github => {
            shell_open(GITHUB_REPO_URL);
        }
        HitId::BrowseFolder | HitId::PathBox => {
            if let Some(chosen) = browse_for_folder(hwnd) {
                if let Some(state) = STATE.as_mut() {
                    state.install_dir = chosen;
                }
            }
        }
        HitId::InstallStep1 => {
            if let Some(state) = STATE.as_mut() {
                state.step = WizardStep::Options;
                state.hover = HitId::None;
                state.pressed = HitId::None;
            }
        }
        HitId::OptAutostart => {
            if let Some(state) = STATE.as_mut() {
                state.opt_autostart = !state.opt_autostart;
            }
        }
        HitId::OptShortcuts => {
            if let Some(state) = STATE.as_mut() {
                state.opt_shortcuts = !state.opt_shortcuts;
            }
        }
        HitId::InstallStep2 => {
            let (install_dir, opt_autostart, opt_shortcuts) = if let Some(state) = STATE.as_mut() {
                state.step = WizardStep::Installing;
                state.progress = 10;
                state.error_msg = None;
                state.hover = HitId::None;
                state.pressed = HitId::None;
                (state.install_dir.clone(), state.opt_autostart, state.opt_shortcuts)
            } else {
                return;
            };
            PROGRESS_VAL.store(10, Ordering::SeqCst);
            let hwnd_val = hwnd as usize;
            std::thread::spawn(move || {
                perform_installation_worker(hwnd_val as HWND, install_dir, opt_autostart, opt_shortcuts);
            });
        }
        HitId::DoneClose => {
            DestroyWindow(hwnd);
        }
        HitId::DoneLaunch => {
            let (is_err, install_dir) = if let Some(state) = STATE.as_ref() {
                (state.error_msg.is_some(), state.install_dir.clone())
            } else {
                (false, get_default_install_dir())
            };
            if is_err {
                if let Some(state) = STATE.as_mut() {
                    state.step = WizardStep::Welcome;
                    state.error_msg = None;
                }
            } else {
                let target_exe = install_dir.join("freeview.exe");
                let _ = Command::new(target_exe).spawn();
                DestroyWindow(hwnd);
            }
        }
        HitId::None => {}
    }
}

unsafe fn browse_for_folder(hwnd: HWND) -> Option<PathBuf> {
    let mut display = [0u16; 260];
    let title = to_wide_null("FreeView를 설치할 폴더를 선택하세요");
    let bi = BROWSEINFOW {
        hwndOwner: hwnd,
        pidlRoot: std::ptr::null_mut(),
        pszDisplayName: display.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };
    let pidl = SHBrowseForFolderW(&bi);
    if pidl.is_null() {
        return None;
    }
    let mut path_buf = [0u16; 260];
    let ok = SHGetPathFromIDListW(pidl, path_buf.as_mut_ptr());
    CoTaskMemFree(pidl as *const c_void);
    if ok != 0 {
        let len = path_buf.iter().position(|&c| c == 0).unwrap_or(path_buf.len());
        let p = PathBuf::from(String::from_utf16_lossy(&path_buf[..len]));
        let chosen = if p.file_name().and_then(|n| n.to_str()).map(|s| s.eq_ignore_ascii_case("FreeView")).unwrap_or(false) {
            p
        } else {
            p.join("FreeView")
        };
        return Some(chosen);
    }
    None
}

fn post_progress(hwnd: HWND, pct: u32) {
    PROGRESS_VAL.store(pct, Ordering::SeqCst);
    unsafe {
        PostMessageW(hwnd, WM_INSTALL_PROGRESS, 0, 0);
    }
}

fn perform_installation_worker(
    hwnd: HWND,
    install_dir: PathBuf,
    opt_autostart: bool,
    opt_shortcuts: bool,
) {
    let target_exe = install_dir.join("freeview.exe");
    let target_loader = install_dir.join("WebView2Loader.dll");
    let target_uninst = install_dir.join("uninstall.exe");
    let target_ico = install_dir.join("logo.ico");

    post_progress(hwnd, 20);

    // 1. 기존 실행 중인 프로세스의 프록시 설정 선복원 후 종료
    if target_exe.exists() {
        let _ = Command::new(&target_exe)
            .creation_flags(CREATE_NO_WINDOW)
            .arg("--restore")
            .output();
    }
    let _ = Command::new("taskkill")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["/F", "/IM", "freeview.exe"])
        .output();

    post_progress(hwnd, 40);

    // 2. 디렉터리 생성 및 파일 쓰기
    if let Err(e) = fs::create_dir_all(&install_dir) {
        INSTALL_OK.store(false, Ordering::SeqCst);
        unsafe {
            INSTALL_ERR.set(format!("폴더 생성 실패: {}", e));
            PostMessageW(hwnd, WM_INSTALL_DONE, 0, 0);
        }
        return;
    }

    if let Err(e) = fs::write(&target_exe, FREEVIEW_EXE_BYTES) {
        INSTALL_OK.store(false, Ordering::SeqCst);
        unsafe {
            INSTALL_ERR.set(format!("freeview.exe 쓰기 실패: {}", e));
            PostMessageW(hwnd, WM_INSTALL_DONE, 0, 0);
        }
        return;
    }

    post_progress(hwnd, 65);

    if let Err(e) = fs::write(&target_loader, WEBVIEW2_LOADER_BYTES) {
        INSTALL_OK.store(false, Ordering::SeqCst);
        unsafe {
            INSTALL_ERR.set(format!("WebView2Loader.dll 쓰기 실패: {}", e));
            PostMessageW(hwnd, WM_INSTALL_DONE, 0, 0);
        }
        return;
    }

    let _ = fs::write(&target_ico, LOGO_ICO_BYTES);

    if let Ok(setup_exe) = std::env::current_exe() {
        let _ = fs::copy(setup_exe, &target_uninst);
    }

    post_progress(hwnd, 82);

    // 3. 자동 시작 등록/해제
    set_autostart_registry(&target_exe, opt_autostart);

    // 4. 바로가기 생성
    if opt_shortcuts {
        let mut shortcuts = Vec::new();
        if let Some(desktop) = get_special_folder(CSIDL_DESKTOPDIRECTORY as i32) {
            shortcuts.push(desktop.join("FreeView.lnk"));
        }
        if let Some(programs) = get_special_folder(CSIDL_PROGRAMS as i32) {
            shortcuts.push(programs.join("FreeView.lnk"));
        }
        if !shortcuts.is_empty() {
            create_shortcuts_batch(&target_exe, &target_ico, &shortcuts);
        }
    }

    post_progress(hwnd, 95);

    // 5. 제어판 언인스톨러 등록
    register_uninstaller(&target_exe, &target_uninst, &target_ico);

    post_progress(hwnd, 100);
    std::thread::sleep(std::time::Duration::from_millis(120));

    INSTALL_OK.store(true, Ordering::SeqCst);
    unsafe {
        PostMessageW(hwnd, WM_INSTALL_DONE, 0, 0);
    }
}

fn set_autostart_registry(target_exe: &Path, enable: bool) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((key, _)) = hkcu.create_subkey(RUN_KEY_PATH) {
        if enable {
            let cmd = format!("\"{}\" --startup", target_exe.to_string_lossy());
            let _ = key.set_value("FreeView", &cmd);
        } else {
            let _ = key.delete_value("FreeView");
        }
    }
}

fn do_uninstall_gui() {
    let install_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(get_default_install_dir);
    let target_exe = install_dir.join("freeview.exe");

    // 1. 네트워크 프록시 복원
    if target_exe.exists() {
        let _ = Command::new(&target_exe)
            .creation_flags(CREATE_NO_WINDOW)
            .arg("--restore")
            .output();
    }

    // 2. 프로세스 종료
    let _ = Command::new("taskkill")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["/F", "/IM", "freeview.exe"])
        .output();

    // 3. 바로가기 삭제
    if let Some(desktop) = get_special_folder(CSIDL_DESKTOPDIRECTORY as i32) {
        let _ = fs::remove_file(desktop.join("FreeView.lnk"));
    }
    if let Some(programs) = get_special_folder(CSIDL_PROGRAMS as i32) {
        let _ = fs::remove_file(programs.join("FreeView.lnk"));
    }

    // 4. 제어판 및 자동실행 레지스트리 키 삭제
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Uninstall", KEY_WRITE) {
        let _ = key.delete_subkey_all("FreeView");
    }

    if let Ok(run_key) = hkcu.open_subkey_with_flags(RUN_KEY_PATH, KEY_WRITE) {
        let _ = run_key.delete_value("FreeView");
    }

    // 5. 완료 안내를 먼저 표시해 사용자가 확인을 누른 뒤 uninstall.exe 잠금 없이 폴더를 삭제
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            to_wide_null("FreeView가 시스템에서 완전히 제거되었습니다.").as_ptr(),
            to_wide_null("제거 완료").as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }

    let pid = std::process::id();
    let escaped_dir = install_dir.to_string_lossy().replace('\'', "''");
    let app_data_dir = std::env::var_os("LOCALAPPDATA")
        .map(|v| PathBuf::from(v).join("FreeView"))
        .unwrap_or_default()
        .to_string_lossy()
        .replace('\'', "''");

    let delete_cmd = if app_data_dir.is_empty() {
        format!(
            "Wait-Process -Id {} -Timeout 10 -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 300; Remove-Item -LiteralPath '{}' -Recurse -Force -ErrorAction SilentlyContinue",
            pid, escaped_dir
        )
    } else {
        format!(
            "Wait-Process -Id {} -Timeout 10 -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 300; Remove-Item -LiteralPath '{}' -Recurse -Force -ErrorAction SilentlyContinue; Remove-Item -LiteralPath '{}' -Recurse -Force -ErrorAction SilentlyContinue",
            pid, escaped_dir, app_data_dir
        )
    };

    let _ = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", &delete_cmd])
        .spawn();
}

fn get_default_install_dir() -> PathBuf {
    let base = match std::env::var_os("LOCALAPPDATA") {
        Some(val) => PathBuf::from(val),
        None => std::env::temp_dir(),
    };
    base.join("Programs").join("FreeView")
}

fn get_special_folder(csidl: i32) -> Option<PathBuf> {
    let mut buf = [0u16; 260];
    let hr = unsafe { SHGetFolderPathW(std::ptr::null_mut(), csidl, std::ptr::null_mut(), 0, buf.as_mut_ptr()) };
    if hr == 0 {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let path_str = String::from_utf16_lossy(&buf[..len]);
        if !path_str.is_empty() {
            return Some(PathBuf::from(path_str));
        }
    }
    None
}

fn create_shortcuts_batch(target_exe: &Path, icon_path: &Path, shortcut_paths: &[PathBuf]) {
    let exe_escaped = target_exe.to_string_lossy().replace('\'', "''");
    let ico_escaped = icon_path.to_string_lossy().replace('\'', "''");
    let work_escaped = target_exe
        .parent()
        .unwrap_or(target_exe)
        .to_string_lossy()
        .replace('\'', "''");

    let mut script = String::from("$ws = New-Object -ComObject WScript.Shell; ");
    for sc in shortcut_paths {
        let sc_escaped = sc.to_string_lossy().replace('\'', "''");
        script.push_str(&format!(
            "$s = $ws.CreateShortcut('{}'); $s.TargetPath = '{}'; $s.IconLocation = '{}'; $s.WorkingDirectory = '{}'; $s.Save(); ",
            sc_escaped, exe_escaped, ico_escaped, work_escaped
        ));
    }

    let _ = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output();
}

fn register_uninstaller(target_exe: &Path, target_uninst: &Path, target_ico: &Path) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((key, _)) = hkcu.create_subkey(UNINSTALL_KEY_PATH) {
        let _ = key.set_value("DisplayName", &"FreeView 3.3");
        let _ = key.set_value("DisplayVersion", &"3.3.1");
        let _ = key.set_value("Publisher", &"QuarterPi & knoblab");
        let _ = key.set_value("DisplayIcon", &target_ico.to_string_lossy().as_ref());
        let uninst_cmd = format!("\"{}\" --uninstall", target_uninst.to_string_lossy());
        let _ = key.set_value("UninstallString", &uninst_cmd);
        let _ = key.set_value("InstallLocation", &target_exe.parent().unwrap_or(target_exe).to_string_lossy().as_ref());
        let _ = key.set_value("NoModify", &1u32);
        let _ = key.set_value("NoRepair", &1u32);
    }
}

fn shell_open(target: &str) {
    let op = to_wide_null("open");
    let file = to_wide_null(target);
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOW,
        );
    }
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
