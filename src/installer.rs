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

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
    CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint,
    EnumFontFamiliesExW, FillRect, InvalidateRect, ScreenToClient, SelectObject, SetBkMode,
    SetTextColor, SetWindowRgn, HBITMAP, HDC, HFONT, LOGFONTW, PAINTSTRUCT, SRCCOPY,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::Shell::{
    SHBrowseForFolderW, SHGetFolderPathW, SHGetPathFromIDListW, ShellExecuteW, BROWSEINFOW,
    CSIDL_DESKTOPDIRECTORY, CSIDL_PROGRAMS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos,
    GetMessageW, GetSystemMetrics, LoadCursorW, MessageBoxW, PostMessageW, PostQuitMessage,
    RegisterClassExW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, HTCAPTION, HTCLIENT,
    IDC_ARROW, IDC_HAND, MB_ICONINFORMATION, MB_OK, MSG, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW,
    WM_APP, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT, WNDCLASSEXW, WS_POPUP, WS_SYSMENU,
};

#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const GdiplusStartupInput, output: *mut c_void) -> i32;
    fn GdiplusShutdown(token: usize);
    fn GdipCreateFromHDC(hdc: HDC, graphics: *mut *mut c_void) -> i32;
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

const WIN_W: i32 = 350;
const WIN_H: i32 = 205;
const TOPBAR_H: i32 = 29;
const RADIUS_CARD: f32 = 19.2;
const RADIUS_BTN: f32 = 8.4;

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
    font_14: HFONT,
    font_24: HFONT,
}

static STATE: UiThreadCell<WizardState> = UiThreadCell::new();
static FONTS: UiThreadCell<WizardFonts> = UiThreadCell::new();
static INSTALL_OK: AtomicBool = AtomicBool::new(false);
static PROGRESS_VAL: AtomicU32 = AtomicU32::new(0);
static INSTALL_ERR: UiThreadCell<String> = UiThreadCell::new();

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--uninstall") {
        do_uninstall_gui();
    } else {
        run_install_wizard();
    }
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
        let title = to_wide_null("FreeView 3.3.0");
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

        let pos_x = ((GetSystemMetrics(SM_CXSCREEN) - WIN_W) / 2).max(0);
        let pos_y = ((GetSystemMetrics(SM_CYSCREEN) - WIN_H) / 2).max(0);

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
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_SYSMENU,
            pos_x,
            pos_y,
            WIN_W,
            WIN_H,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            h_instance,
            std::ptr::null(),
        );

        let hdc = windows_sys::Win32::Graphics::Gdi::GetDC(hwnd);
        let font_family = if is_font_installed(hdc, "IBM Plex Sans KR") {
            "IBM Plex Sans KR"
        } else {
            "Malgun Gothic"
        };
        windows_sys::Win32::Graphics::Gdi::ReleaseDC(hwnd, hdc);

        FONTS.set(WizardFonts {
            font_14: create_font(14, 400, font_family),
            font_24: create_font(24, 400, font_family),
        });

        let rgn = CreateRoundRectRgn(0, 0, WIN_W + 1, WIN_H + 1, 38, 38);
        SetWindowRgn(hwnd, rgn, 1);

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
        0,
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

unsafe fn draw_folder_icon(g: *mut c_void, btn_rect: RectF) {
    // 4:20 inside 6:51 svg icon centered in 37.2x37.2 square (1.2x scale)
    let cx = btn_rect.x + btn_rect.w * 0.5;
    let cy = btn_rect.y + btn_rect.h * 0.5;
    let mut pen: *mut c_void = std::ptr::null_mut();
    GdipCreatePen1(0xFFF7_F6F2, 1.8, 2, &mut pen);
    GdipSetPenStartCap(pen, 2);
    GdipSetPenEndCap(pen, 2);
    GdipSetPenLineJoin(pen, 2);

    let pts = [
        GpPointF { x: cx - 8.0, y: cy - 6.0 },
        GpPointF { x: cx - 2.4, y: cy - 6.0 },
        GpPointF { x: cx - 0.4, y: cy - 3.6 },
        GpPointF { x: cx + 8.0, y: cy - 3.6 },
        GpPointF { x: cx + 8.0, y: cy + 6.0 },
        GpPointF { x: cx - 8.0, y: cy + 6.0 },
        GpPointF { x: cx - 8.0, y: cy - 6.0 },
    ];
    GdipDrawLines(g, pen, pts.as_ptr(), pts.len() as i32);
    GdipDeletePen(pen);
}

unsafe fn draw_checkmark(g: *mut c_void, box_rect: RectF) {
    let mut pen: *mut c_void = std::ptr::null_mut();
    GdipCreatePen1(0xFFF7_F6F2, 1.8, 2, &mut pen);
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

unsafe fn draw_gdi_text(hdc: HDC, font: HFONT, color: u32, r: RectF, text: &str, flags: u32) {
    SelectObject(hdc, font as _);
    SetTextColor(hdc, color);
    let mut rc = r.to_rect();
    let wide = to_wide_null(text);
    DrawTextW(hdc, wide.as_ptr(), -1, &mut rc, flags | DT_NOPREFIX);
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

unsafe fn paint_wizard(hwnd: HWND, hdc_dst: HDC) {
    let mut rc_client: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc_client);
    let w = rc_client.right - rc_client.left;
    let h = rc_client.bottom - rc_client.top;

    let mem_dc = CreateCompatibleDC(hdc_dst);
    let mem_bmp: HBITMAP = CreateCompatibleBitmap(hdc_dst, w, h);
    let old_bmp = SelectObject(mem_dc, mem_bmp as _);

    let bg_brush = CreateSolidBrush(CREF_BG);
    FillRect(mem_dc, &rc_client, bg_brush);
    DeleteObject(bg_brush);

    let mut g: *mut c_void = std::ptr::null_mut();
    GdipCreateFromHDC(mem_dc, &mut g);
    GdipSetSmoothingMode(g, 4); // SmoothingModeAntiAlias

    // Outer 19.2px card border
    fill_round_rect(g, RectF::new(0.0, 0.0, WIN_W as f32, WIN_H as f32), RADIUS_CARD, ARGB_BG);
    stroke_round_rect(
        g,
        RectF::new(0.5, 0.5, (WIN_W - 1) as f32, (WIN_H - 1) as f32),
        RADIUS_CARD - 0.5,
        ARGB_CARD_BORDER,
        1.0,
    );

    SetBkMode(mem_dc, 1);

    let Some(state) = STATE.as_ref() else {
        GdipDeleteGraphics(g);
        BitBlt(hdc_dst, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
        SelectObject(mem_dc, old_bmp);
        DeleteObject(mem_bmp as _);
        DeleteDC(mem_dc);
        return;
    };

    let Some(fonts) = FONTS.as_ref() else {
        GdipDeleteGraphics(g);
        BitBlt(hdc_dst, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
        SelectObject(mem_dc, old_bmp);
        DeleteObject(mem_bmp as _);
        DeleteDC(mem_dc);
        return;
    };

    // Top bar title: "FreeView 3.3.0"
    draw_gdi_text(
        mem_dc,
        fonts.font_14,
        CREF_TITLE,
        RectF::new(0.0, 1.5, WIN_W as f32, TOPBAR_H as f32),
        "FreeView 3.3.0",
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );

    match state.step {
        WizardStep::Welcome => {
            // 1. "깃허브" button
            let bg_gh = pick_btn_bg(state, HitId::Github, ARGB_BTN_SEC, ARGB_BTN_SEC_HOVER, ARGB_BTN_SEC_DOWN);
            fill_round_rect(g, R_S1_GITHUB, RADIUS_BTN, bg_gh);
            draw_gdi_text(mem_dc, fonts.font_14, CREF_SEC_TEXT, R_S1_GITHUB, "깃허브", DT_CENTER | DT_VCENTER | DT_SINGLELINE);

            // 2. Folder picker button
            let bg_dir = pick_btn_bg(state, HitId::BrowseFolder, ARGB_FOLDER_BG, ARGB_FOLDER_HOVER, ARGB_FOLDER_DOWN);
            fill_round_rect(g, R_S1_FOLDER, RADIUS_BTN, bg_dir);
            draw_folder_icon(g, R_S1_FOLDER);

            // 3. Path box
            let bg_path = pick_btn_bg(state, HitId::PathBox, ARGB_BTN_SEC, ARGB_BTN_SEC_HOVER, ARGB_BTN_SEC_DOWN);
            fill_round_rect(g, R_S1_PATH, RADIUS_BTN, bg_path);
            let path_text = state.install_dir.to_string_lossy();
            draw_gdi_text(
                mem_dc,
                fonts.font_14,
                CREF_SEC_TEXT,
                RectF::new(R_S1_PATH.x + 12.0, R_S1_PATH.y, R_S1_PATH.w - 24.0, R_S1_PATH.h),
                &path_text,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS,
            );

            // 4. Primary "설치" button
            let bg_inst = pick_btn_bg(state, HitId::InstallStep1, ARGB_TEAL, ARGB_TEAL_HOVER, ARGB_TEAL_DOWN);
            fill_round_rect(g, R_S1_INSTALL, RADIUS_BTN, bg_inst);
            draw_gdi_text(mem_dc, fonts.font_24, CREF_LIGHT_TEXT, R_S1_INSTALL, "설치", DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }

        WizardStep::Options => {
            // 1. "시작 시 자동 실행"
            let bg_auto = pick_btn_bg(state, HitId::OptAutostart, ARGB_BTN_SEC, ARGB_BTN_SEC_HOVER, ARGB_BTN_SEC_DOWN);
            fill_round_rect(g, R_S2_AUTO, RADIUS_BTN, bg_auto);
            fill_round_rect(g, R_S2_AUTO_CB, 2.4, if state.opt_autostart { ARGB_TEAL } else { ARGB_CHECK_OFF });
            if state.opt_autostart {
                draw_checkmark(g, R_S2_AUTO_CB);
            }
            draw_gdi_text(mem_dc, fonts.font_14, CREF_SEC_TEXT, R_S2_AUTO, "시작 시 자동 실행", DT_CENTER | DT_VCENTER | DT_SINGLELINE);

            // 2. "바로가기 만들기"
            let bg_short = pick_btn_bg(state, HitId::OptShortcuts, ARGB_BTN_SEC, ARGB_BTN_SEC_HOVER, ARGB_BTN_SEC_DOWN);
            fill_round_rect(g, R_S2_SHORT, RADIUS_BTN, bg_short);
            fill_round_rect(g, R_S2_SHORT_CB, 2.4, if state.opt_shortcuts { ARGB_TEAL } else { ARGB_CHECK_OFF });
            if state.opt_shortcuts {
                draw_checkmark(g, R_S2_SHORT_CB);
            }
            draw_gdi_text(mem_dc, fonts.font_14, CREF_SEC_TEXT, R_S2_SHORT, "바로가기 만들기", DT_CENTER | DT_VCENTER | DT_SINGLELINE);

            // 3. Primary "설치" button
            let bg_inst = pick_btn_bg(state, HitId::InstallStep2, ARGB_TEAL, ARGB_TEAL_HOVER, ARGB_TEAL_DOWN);
            fill_round_rect(g, R_S2_INSTALL, RADIUS_BTN, bg_inst);
            draw_gdi_text(mem_dc, fonts.font_24, CREF_LIGHT_TEXT, R_S2_INSTALL, "설치", DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }

        WizardStep::Installing => {
            // Progress track & fill
            fill_round_rect(g, R_MID_ROW, RADIUS_BTN, ARGB_BTN_SEC);
            let pct = state.progress.min(100);
            if pct > 0 {
                let fill_w = (R_MID_ROW.w * (pct as f32 / 100.0)).max(16.8);
                fill_round_rect(g, RectF::new(R_MID_ROW.x, R_MID_ROW.y, fill_w, R_MID_ROW.h), RADIUS_BTN, ARGB_TEAL);
            }

            // Disabled "설치중..." button
            fill_round_rect(g, R_BOTTOM_BTN, RADIUS_BTN, ARGB_MUTED);
            draw_gdi_text(mem_dc, fonts.font_24, CREF_LIGHT_TEXT, R_BOTTOM_BTN, "설치중...", DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }

        WizardStep::Done => {
            let status_text = state
                .error_msg
                .as_deref()
                .unwrap_or("설치 완료!");
            draw_gdi_text(
                mem_dc,
                fonts.font_14,
                CREF_SEC_TEXT,
                R_DONE_STATUS,
                status_text,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS,
            );

            // "닫기" button
            let bg_close = pick_btn_bg(state, HitId::DoneClose, ARGB_BTN_SEC, ARGB_BTN_SEC_HOVER, ARGB_BTN_SEC_DOWN);
            fill_round_rect(g, R_MID_ROW, RADIUS_BTN, bg_close);
            draw_gdi_text(mem_dc, fonts.font_14, CREF_SEC_TEXT, R_MID_ROW, "닫기", DT_CENTER | DT_VCENTER | DT_SINGLELINE);

            // "실행" (or "다시 시도" if error)
            let bg_launch = pick_btn_bg(state, HitId::DoneLaunch, ARGB_TEAL, ARGB_TEAL_HOVER, ARGB_TEAL_DOWN);
            fill_round_rect(g, R_BOTTOM_BTN, RADIUS_BTN, bg_launch);
            let primary_label = if state.error_msg.is_some() { "다시 시도" } else { "실행" };
            draw_gdi_text(mem_dc, fonts.font_24, CREF_LIGHT_TEXT, R_BOTTOM_BTN, primary_label, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
    }

    GdipDeleteGraphics(g);
    BitBlt(hdc_dst, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
    SelectObject(mem_dc, old_bmp);
    DeleteObject(mem_bmp as _);
    DeleteDC(mem_dc);
}

unsafe extern "system" fn wizard_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,

        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint_wizard(hwnd, hdc);
            EndPaint(hwnd, &ps);
            0
        }

        WM_NCHITTEST => {
            let mut pt = POINT {
                x: (lparam as i32) as i16 as i32,
                y: ((lparam as i32) >> 16) as i16 as i32,
            };
            ScreenToClient(hwnd, &mut pt);
            if pt.y >= 0 && pt.y < TOPBAR_H {
                HTCAPTION as LRESULT
            } else {
                HTCLIENT as LRESULT
            }
        }

        WM_MOUSEMOVE => {
            let x = ((lparam as i32) & 0xFFFF) as i16 as f32;
            let y = (((lparam as i32) >> 16) & 0xFFFF) as i16 as f32;
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
                    InvalidateRect(hwnd, std::ptr::null(), 0);
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
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            0
        }

        WM_LBUTTONDOWN => {
            let x = ((lparam as i32) & 0xFFFF) as i16 as f32;
            let y = (((lparam as i32) >> 16) & 0xFFFF) as i16 as f32;
            if let Some(state) = STATE.as_mut() {
                let hit = hit_test_control(state, x, y);
                if hit != HitId::None {
                    state.pressed = hit;
                    SetCapture(hwnd);
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            0
        }

        WM_LBUTTONUP => {
            let x = ((lparam as i32) & 0xFFFF) as i16 as f32;
            let y = (((lparam as i32) >> 16) & 0xFFFF) as i16 as f32;
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
            InvalidateRect(hwnd, std::ptr::null(), 0);
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
            InvalidateRect(hwnd, std::ptr::null(), 0);
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
                state.hover = hit_test_control(state, pt.x as f32, pt.y as f32);
            }
            InvalidateRect(hwnd, std::ptr::null(), 0);
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
        let _ = key.set_value("DisplayVersion", &"3.3.0");
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
