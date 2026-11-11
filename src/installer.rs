#![windows_subsystem = "windows"]

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
use winreg::RegKey;

use windows_sys::Win32::Foundation::{BOOL, HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, EndPaint, FillRect,
    InvalidateRect, LineTo, MoveToEx, RoundRect, SelectObject, SetBkColor, SetBkMode, SetTextColor,
    HBRUSH, HDC, HFONT, PAINTSTRUCT, PS_SOLID,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
    GetMessageW, GetWindowTextW, LoadCursorW, PostQuitMessage, RegisterClassExW, SendMessageW,
    SetWindowTextW, ShowWindow, TranslateMessage, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX,
    BS_OWNERDRAW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, HMENU, IDC_ARROW, MSG, SW_HIDE, SW_SHOW,
    WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_PAINT,
    WM_SETFONT, WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

#[repr(C)]
#[allow(non_snake_case)]
struct DRAWITEMSTRUCT {
    pub CtlType: u32,
    pub CtlID: u32,
    pub itemID: u32,
    pub itemAction: u32,
    pub itemState: u32,
    pub hwndItem: HWND,
    pub hDC: HDC,
    pub rcItem: RECT,
    pub itemData: usize,
}

#[link(name = "user32")]
extern "system" {
    fn EnableWindow(hWnd: HWND, bEnable: BOOL) -> BOOL;
    fn DrawTextW(hDC: HDC, lpchText: *const u16, cchText: i32, lprc: *mut RECT, format: u32) -> i32;
}

const ODS_SELECTED: u32 = 0x0001;
const ODS_DISABLED: u32 = 0x0004;

const DT_LEFT: u32 = 0x00000000;
const DT_CENTER: u32 = 0x00000001;
const DT_VCENTER: u32 = 0x00000004;
const DT_SINGLELINE: u32 = 0x00000020;

const BST_CHECKED: usize = 1;
const PBM_SETPOS: u32 = 0x0402;

const FREEVIEW_EXE_BYTES: &[u8] = include_bytes!("../target/release/freeview.exe");
const WEBVIEW2_LOADER_BYTES: &[u8] = include_bytes!("../target/release/WebView2Loader.dll");
const LOGO_ICO_BYTES: &[u8] = include_bytes!("../logo.ico");

const UNINSTALL_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\FreeView";
const CREATE_NO_WINDOW: u32 = 0x08000000;

const IDC_BTN_INSTALL: usize = 1001;
const IDC_BTN_CANCEL: usize = 1002;
const IDC_CHK_DESKTOP: usize = 1003;
const IDC_CHK_STARTMENU: usize = 1004;
const IDC_CHK_LAUNCH: usize = 1005;
const IDC_PROGRESS: usize = 1006;
const IDC_LBL_STATUS: usize = 1007;

// PDF-DS Color Palette (COLORREF format: 0x00BBGGRR)
const COLOR_BG_PRIMARY: u32 = 0x000B0909;     // #09090b
const COLOR_BG_SECONDARY: u32 = 0x001B1818;   // #18181b
const COLOR_BORDER_DEFAULT: u32 = 0x002A2727; // #27272a
const COLOR_BORDER_HOVER: u32 = 0x00463F3F;   // #3f3f46
const COLOR_TEXT_PRIMARY: u32 = 0x00FFFFFF;   // #ffffff
const COLOR_TEXT_SECONDARY: u32 = 0x00AAA1A1; // #a1a1aa
const COLOR_FUNCTIONAL_RED: u32 = 0x004444EF; // #ef4444
const COLOR_RED_HOVER: u32 = 0x002626DC;      // #dc2626

static IS_FINISHED: AtomicBool = AtomicBool::new(false);
static PROGRESS_POS: AtomicU32 = AtomicU32::new(0);

struct WizardFonts {
    font_title: HFONT,
    font_badge: HFONT,
    font_subhead: HFONT,
    font_body: HFONT,
    font_mono: HFONT,
    font_btn: HFONT,
}

static mut FONTS: Option<WizardFonts> = None;

#[allow(dead_code)]
struct WizardControls {
    h_lbl_status: HWND,
    h_lbl_path: HWND,
    h_chk_desktop: HWND,
    h_chk_startmenu: HWND,
    h_chk_launch: HWND,
    h_progress: HWND,
    h_btn_install: HWND,
    h_btn_cancel: HWND,
    brush_bg: HBRUSH,
    brush_card: HBRUSH,
    brush_chip: HBRUSH,
}

static mut CONTROLS: Option<WizardControls> = None;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--uninstall") {
        do_uninstall_gui();
    } else {
        run_install_wizard();
    }
}

fn run_install_wizard() {
    unsafe {
        let h_instance = GetModuleHandleW(std::ptr::null());
        init_fonts();

        // 1. 커스텀 프로그레스 바 클래스 등록
        register_progress_class(h_instance);

        // 2. 메인 위저드 윈도우 클래스 등록
        let class_name: Vec<u16> = to_wide_null("FreeViewInstallerWizard");
        let title: Vec<u16> = to_wide_null("FreeView 3.1 Setup");

        let brush_bg = CreateSolidBrush(COLOR_BG_PRIMARY);
        let brush_card = CreateSolidBrush(COLOR_BG_SECONDARY);
        let brush_chip = CreateSolidBrush(COLOR_BG_PRIMARY);

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

        // 윈도우 크기: 500 x 400
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            500,
            400,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            h_instance,
            std::ptr::null(),
        );

        // Windows 10/11 다크 모드 타이틀바 적용
        apply_dark_mode(hwnd);

        create_wizard_controls(hwnd, h_instance, brush_bg, brush_card, brush_chip);

        ShowWindow(hwnd, SW_SHOW);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if let Some(ctrls) = CONTROLS.take() {
            DeleteObject(ctrls.brush_bg);
            DeleteObject(ctrls.brush_card);
            DeleteObject(ctrls.brush_chip);
        }

        if let Some(f) = FONTS.take() {
            DeleteObject(f.font_title);
            DeleteObject(f.font_badge);
            DeleteObject(f.font_subhead);
            DeleteObject(f.font_body);
            DeleteObject(f.font_mono);
            DeleteObject(f.font_btn);
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
        1, // DEFAULT_CHARSET
        0,
        0,
        5, // CLEARTYPE_QUALITY
        0,
        to_wide_null(name).as_ptr(),
    )
}

unsafe fn init_fonts() {
    FONTS = Some(WizardFonts {
        font_title: create_font(20, 800, "Segoe UI"),
        font_badge: create_font(9, 700, "Consolas"),
        font_subhead: create_font(10, 400, "Segoe UI"),
        font_body: create_font(10, 500, "Segoe UI"),
        font_mono: create_font(9, 500, "Consolas"),
        font_btn: create_font(11, 700, "Segoe UI"),
    });
}

unsafe fn register_progress_class(h_instance: HMODULE) {
    let class_name: Vec<u16> = to_wide_null("FreeViewProgressBar");
    let wnd_class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(progress_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: h_instance,
        hIcon: std::ptr::null_mut(),
        hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
        hbrBackground: 0 as HBRUSH,
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: std::ptr::null_mut(),
    };
    RegisterClassExW(&wnd_class);
}

unsafe extern "system" fn progress_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);

            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rc);

            // 1. Dark track background (#27272a)
            let track_brush = CreateSolidBrush(COLOR_BORDER_DEFAULT);
            let track_pen = CreatePen(PS_SOLID as i32, 1, COLOR_BORDER_DEFAULT);
            let old_brush = SelectObject(hdc, track_brush);
            let old_pen = SelectObject(hdc, track_pen);
            RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, 6, 6);

            // 2. Red progress fill (#ef4444)
            let pos = PROGRESS_POS.load(Ordering::SeqCst).min(100);
            if pos > 0 {
                let fill_width = ((rc.right - rc.left) as f32 * (pos as f32 / 100.0)) as i32;
                if fill_width > 4 {
                    let fill_brush = CreateSolidBrush(COLOR_FUNCTIONAL_RED);
                    let fill_pen = CreatePen(PS_SOLID as i32, 1, COLOR_FUNCTIONAL_RED);
                    SelectObject(hdc, fill_brush);
                    SelectObject(hdc, fill_pen);
                    RoundRect(hdc, rc.left, rc.top, rc.left + fill_width, rc.bottom, 6, 6);
                    DeleteObject(fill_brush);
                    DeleteObject(fill_pen);
                }
            }

            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(track_brush);
            DeleteObject(track_pen);

            EndPaint(hwnd, &ps);
            0
        }
        PBM_SETPOS => {
            PROGRESS_POS.store(wparam as u32, Ordering::SeqCst);
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn create_wizard_controls(
    hwnd: HWND,
    h_instance: HMODULE,
    brush_bg: HBRUSH,
    brush_card: HBRUSH,
    brush_chip: HBRUSH,
) {
    let static_cls = to_wide_null("STATIC");
    let btn_cls = to_wide_null("BUTTON");
    let progress_cls = to_wide_null("FreeViewProgressBar");

    let f_body = FONTS.as_ref().map(|f| f.font_body).unwrap_or(std::ptr::null_mut());
    let f_mono = FONTS.as_ref().map(|f| f.font_mono).unwrap_or(std::ptr::null_mut());

    // 1. 설치 경로 텍스트 (옵션 카드 내부 칩)
    let install_dir_str = format!("설치 경로: {}", get_install_dir().to_string_lossy());
    let h_lbl_path = CreateWindowExW(
        0,
        static_cls.as_ptr(),
        to_wide_null(&install_dir_str).as_ptr(),
        WS_CHILD | WS_VISIBLE,
        50,
        100,
        390,
        18,
        hwnd,
        std::ptr::null_mut(),
        h_instance,
        std::ptr::null(),
    );
    SendMessageW(h_lbl_path, WM_SETFONT, f_mono as WPARAM, 1);

    // 2. 바로가기 체크박스들
    let h_chk_desktop = CreateWindowExW(
        0,
        btn_cls.as_ptr(),
        to_wide_null("바탕화면에 바로가기 아이콘 생성").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        45,
        134,
        390,
        22,
        hwnd,
        IDC_CHK_DESKTOP as HMENU,
        h_instance,
        std::ptr::null(),
    );
    SendMessageW(h_chk_desktop, WM_SETFONT, f_body as WPARAM, 1);
    SendMessageW(h_chk_desktop, BM_SETCHECK, BST_CHECKED as WPARAM, 0);
    apply_dark_theme(h_chk_desktop);

    let h_chk_startmenu = CreateWindowExW(
        0,
        btn_cls.as_ptr(),
        to_wide_null("시작 메뉴에 바로가기 등록").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        45,
        162,
        390,
        22,
        hwnd,
        IDC_CHK_STARTMENU as HMENU,
        h_instance,
        std::ptr::null(),
    );
    SendMessageW(h_chk_startmenu, WM_SETFONT, f_body as WPARAM, 1);
    SendMessageW(h_chk_startmenu, BM_SETCHECK, BST_CHECKED as WPARAM, 0);
    apply_dark_theme(h_chk_startmenu);

    let h_chk_launch = CreateWindowExW(
        0,
        btn_cls.as_ptr(),
        to_wide_null("설치 완료 후 FreeView 즉시 실행").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        45,
        190,
        390,
        22,
        hwnd,
        IDC_CHK_LAUNCH as HMENU,
        h_instance,
        std::ptr::null(),
    );
    SendMessageW(h_chk_launch, WM_SETFONT, f_body as WPARAM, 1);
    SendMessageW(h_chk_launch, BM_SETCHECK, BST_CHECKED as WPARAM, 0);
    apply_dark_theme(h_chk_launch);

    // 3. 상태 안내 레이블
    let h_lbl_status = CreateWindowExW(
        0,
        static_cls.as_ptr(),
        to_wide_null("설치 준비 완료. [설치 시작] 버튼을 누르세요.").as_ptr(),
        WS_CHILD | WS_VISIBLE,
        25,
        248,
        435,
        18,
        hwnd,
        IDC_LBL_STATUS as HMENU,
        h_instance,
        std::ptr::null(),
    );
    SendMessageW(h_lbl_status, WM_SETFONT, f_body as WPARAM, 1);

    // 4. 커스텀 프로그레스 바
    let h_progress = CreateWindowExW(
        0,
        progress_cls.as_ptr(),
        std::ptr::null(),
        WS_CHILD | WS_VISIBLE,
        25,
        270,
        435,
        12,
        hwnd,
        IDC_PROGRESS as HMENU,
        h_instance,
        std::ptr::null(),
    );

    // 5. 하단 버튼들 (BS_OWNERDRAW)
    let h_btn_install = CreateWindowExW(
        0,
        btn_cls.as_ptr(),
        to_wide_null("설치 시작 (&I)").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
        250,
        304,
        115,
        36,
        hwnd,
        IDC_BTN_INSTALL as HMENU,
        h_instance,
        std::ptr::null(),
    );

    let h_btn_cancel = CreateWindowExW(
        0,
        btn_cls.as_ptr(),
        to_wide_null("취소 (&C)").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
        375,
        304,
        85,
        36,
        hwnd,
        IDC_BTN_CANCEL as HMENU,
        h_instance,
        std::ptr::null(),
    );

    CONTROLS = Some(WizardControls {
        h_lbl_status,
        h_lbl_path,
        h_chk_desktop,
        h_chk_startmenu,
        h_chk_launch,
        h_progress,
        h_btn_install,
        h_btn_cancel,
        brush_bg,
        brush_card,
        brush_chip,
    });
}

unsafe extern "system" fn wizard_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);

            let mut rc_client: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rc_client);

            // 1. Window Background (#09090b)
            let bg_brush = CreateSolidBrush(COLOR_BG_PRIMARY);
            FillRect(hdc, &rc_client, bg_brush);
            DeleteObject(bg_brush);

            SetBkMode(hdc, 1 /* TRANSPARENT */);

            // 2. Header Drawing
            if let Some(ref f) = FONTS {
                // Red Shield Badge at (25, 18, 55, 48)
                let shield_brush = CreateSolidBrush(COLOR_FUNCTIONAL_RED);
                let shield_pen = CreatePen(PS_SOLID as i32, 1, COLOR_FUNCTIONAL_RED);
                let old_b = SelectObject(hdc, shield_brush);
                let old_p = SelectObject(hdc, shield_pen);
                RoundRect(hdc, 25, 18, 55, 48, 8, 8);
                SelectObject(hdc, old_p);
                SelectObject(hdc, old_b);
                DeleteObject(shield_brush);
                DeleteObject(shield_pen);

                // Shield Text: 'FV'
                SelectObject(hdc, f.font_badge);
                SetTextColor(hdc, COLOR_TEXT_PRIMARY);
                let mut rc_shield = RECT { left: 25, top: 18, right: 55, bottom: 48 };
                DrawTextW(hdc, to_wide_null("FV").as_ptr(), 2, &mut rc_shield, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

                // Title: 'FreeView 3.1'
                SelectObject(hdc, f.font_title);
                SetTextColor(hdc, COLOR_TEXT_PRIMARY);
                let mut rc_title = RECT { left: 65, top: 12, right: 260, bottom: 36 };
                DrawTextW(hdc, to_wide_null("FreeView 3.1").as_ptr(), 12, &mut rc_title, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

                // Tag: '[SETUP]'
                SelectObject(hdc, f.font_badge);
                SetTextColor(hdc, COLOR_FUNCTIONAL_RED);
                let mut rc_badge = RECT { left: 235, top: 16, right: 360, bottom: 36 };
                DrawTextW(hdc, to_wide_null("[SETUP WIZARD]").as_ptr(), 14, &mut rc_badge, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

                // Subtitle
                SelectObject(hdc, f.font_subhead);
                SetTextColor(hdc, COLOR_TEXT_SECONDARY);
                let mut rc_sub = RECT { left: 65, top: 36, right: 460, bottom: 56 };
                let sub_text = to_wide_null("Windows DPI/SNI 차단 우회 및 고성능 네이티브 엔진");
                DrawTextW(hdc, sub_text.as_ptr(), sub_text.len() as i32 - 1, &mut rc_sub, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            }

            // 3. Separator Line at y=62
            let line_pen = CreatePen(PS_SOLID as i32, 1, COLOR_BORDER_DEFAULT);
            let old_p = SelectObject(hdc, line_pen);
            MoveToEx(hdc, 25, 62, std::ptr::null_mut());
            LineTo(hdc, 460, 62);
            SelectObject(hdc, old_p);
            DeleteObject(line_pen);

            // 4. Options Card Box at (25, 76, 460, 230)
            let card_brush = CreateSolidBrush(COLOR_BG_SECONDARY);
            let card_pen = CreatePen(PS_SOLID as i32, 1, COLOR_BORDER_DEFAULT);
            let old_b = SelectObject(hdc, card_brush);
            let old_p = SelectObject(hdc, card_pen);
            RoundRect(hdc, 25, 76, 460, 230, 10, 10);

            // Path Chip Box (38, 90, 447, 122)
            let chip_brush = CreateSolidBrush(COLOR_BG_PRIMARY);
            let chip_pen = CreatePen(PS_SOLID as i32, 1, COLOR_BORDER_DEFAULT);
            SelectObject(hdc, chip_brush);
            SelectObject(hdc, chip_pen);
            RoundRect(hdc, 38, 90, 447, 122, 6, 6);
            DeleteObject(chip_brush);
            DeleteObject(chip_pen);

            SelectObject(hdc, old_p);
            SelectObject(hdc, old_b);
            DeleteObject(card_brush);
            DeleteObject(card_pen);

            EndPaint(hwnd, &ps);
            0
        }

        WM_DRAWITEM => {
            let dis = &*(lparam as *const DRAWITEMSTRUCT);
            let is_pressed = (dis.itemState & ODS_SELECTED) != 0;
            let is_disabled = (dis.itemState & ODS_DISABLED) != 0;
            let hdc = dis.hDC;
            let rc = dis.rcItem;

            let (bg_color, text_color, border_color) = if dis.CtlID as usize == IDC_BTN_INSTALL {
                if is_disabled {
                    (COLOR_BORDER_DEFAULT, 0x0071717A, COLOR_BORDER_DEFAULT)
                } else if is_pressed {
                    (COLOR_RED_HOVER, COLOR_TEXT_PRIMARY, COLOR_RED_HOVER)
                } else {
                    (COLOR_FUNCTIONAL_RED, COLOR_TEXT_PRIMARY, COLOR_FUNCTIONAL_RED)
                }
            } else {
                // 취소 버튼
                if is_pressed {
                    (COLOR_BG_SECONDARY, COLOR_TEXT_SECONDARY, COLOR_BORDER_HOVER)
                } else {
                    (COLOR_BORDER_DEFAULT, COLOR_TEXT_PRIMARY, COLOR_BORDER_HOVER)
                }
            };

            let brush = CreateSolidBrush(bg_color);
            let pen = CreatePen(PS_SOLID as i32, 1, border_color);
            let old_brush = SelectObject(hdc, brush);
            let old_pen = SelectObject(hdc, pen);

            RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, 8, 8);

            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(pen);
            DeleteObject(brush);

            // 텍스트 출력
            let mut text_buf = [0u16; 64];
            let len = GetWindowTextW(dis.hwndItem, text_buf.as_mut_ptr(), 64);
            if len > 0 {
                SetBkMode(hdc, 1 /* TRANSPARENT */);
                SetTextColor(hdc, text_color);
                if let Some(ref f) = FONTS {
                    SelectObject(hdc, f.font_btn);
                }
                let mut rc_mut = rc;
                DrawTextW(hdc, text_buf.as_ptr(), len, &mut rc_mut, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            }

            1
        }

        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let hdc = wparam as HDC;
            let hwnd_ctrl = lparam as HWND;

            if let Some(ref c) = CONTROLS {
                if hwnd_ctrl == c.h_lbl_path {
                    SetBkColor(hdc, COLOR_BG_PRIMARY);
                    SetTextColor(hdc, COLOR_TEXT_PRIMARY);
                    return c.brush_chip as LRESULT;
                } else if hwnd_ctrl == c.h_chk_desktop
                    || hwnd_ctrl == c.h_chk_startmenu
                    || hwnd_ctrl == c.h_chk_launch
                {
                    SetBkColor(hdc, COLOR_BG_SECONDARY);
                    SetTextColor(hdc, COLOR_TEXT_PRIMARY);
                    return c.brush_card as LRESULT;
                } else {
                    SetBkColor(hdc, COLOR_BG_PRIMARY);
                    SetTextColor(hdc, COLOR_TEXT_SECONDARY);
                    return c.brush_bg as LRESULT;
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as usize;
            match id {
                IDC_BTN_INSTALL => {
                    if IS_FINISHED.load(Ordering::SeqCst) {
                        // [마침] 버튼 동작
                        let should_launch = if let Some(ref c) = CONTROLS {
                            SendMessageW(c.h_chk_launch, BM_GETCHECK, 0, 0) == BST_CHECKED as isize
                        } else {
                            true
                        };

                        if should_launch {
                            let target_exe = get_install_dir().join("freeview.exe");
                            let _ = Command::new(target_exe).spawn();
                        }
                        DestroyWindow(hwnd);
                    } else {
                        // [설치 시작] 동작
                        perform_installation(hwnd);
                    }
                }
                IDC_BTN_CANCEL => {
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
            0
        }

        WM_CLOSE => {
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

fn perform_installation(_hwnd: HWND) {
    unsafe {
        let (create_desktop, create_startmenu) = if let Some(ref c) = CONTROLS {
            EnableWindow(c.h_btn_install, 0);
            InvalidateRect(c.h_btn_install, std::ptr::null(), 1);
            EnableWindow(c.h_btn_cancel, 0);
            InvalidateRect(c.h_btn_cancel, std::ptr::null(), 1);
            SetWindowTextW(c.h_lbl_status, to_wide_null("설치 진행 중: 기존 프로세스 정리...").as_ptr());
            SendMessageW(c.h_progress, PBM_SETPOS, 20, 0);

            (
                SendMessageW(c.h_chk_desktop, BM_GETCHECK, 0, 0) == BST_CHECKED as isize,
                SendMessageW(c.h_chk_startmenu, BM_GETCHECK, 0, 0) == BST_CHECKED as isize,
            )
        } else {
            (true, true)
        };

        // 1. 기존 프로세스 종료
        let _ = Command::new("taskkill")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["/F", "/IM", "freeview.exe"])
            .output();

        // 2. 디렉터리 생성 및 파일 쓰기
        let install_dir = get_install_dir();
        let _ = fs::create_dir_all(&install_dir);

        if let Some(ref c) = CONTROLS {
            SetWindowTextW(c.h_lbl_status, to_wide_null("설치 진행 중: 바이너리 및 라이브러리 복사...").as_ptr());
            SendMessageW(c.h_progress, PBM_SETPOS, 50, 0);
        }

        let target_exe = install_dir.join("freeview.exe");
        let target_loader = install_dir.join("WebView2Loader.dll");
        let target_uninst = install_dir.join("uninstall.exe");
        let target_ico = install_dir.join("logo.ico");

        if let Err(e) = fs::write(&target_exe, FREEVIEW_EXE_BYTES) {
            let msg = format!("freeview.exe 쓰기 실패: {}", e);
            if let Some(ref c) = CONTROLS {
                SetWindowTextW(c.h_lbl_status, to_wide_null(&msg).as_ptr());
                EnableWindow(c.h_btn_cancel, 1);
            }
            return;
        }

        // WebView2Loader.dll 복사
        let _ = fs::write(&target_loader, WEBVIEW2_LOADER_BYTES);
        let _ = fs::write(&target_ico, LOGO_ICO_BYTES);

        // 현재 설치 프로그램을 uninstall.exe로 복사
        if let Ok(setup_exe) = std::env::current_exe() {
            let _ = fs::copy(setup_exe, &target_uninst);
        }

        if let Some(ref c) = CONTROLS {
            SetWindowTextW(c.h_lbl_status, to_wide_null("설치 진행 중: 바로가기 및 시스템 등록...").as_ptr());
            SendMessageW(c.h_progress, PBM_SETPOS, 80, 0);
        }

        // 3. 바로가기 생성 (cmd 창 절대 안 뜸)
        if create_desktop {
            if let Some(desktop) = get_special_folder("Desktop") {
                create_shortcut(&target_exe, &target_ico, &desktop.join("FreeView.lnk"));
            }
        }

        if create_startmenu {
            if let Some(programs) = get_special_folder("Programs") {
                create_shortcut(&target_exe, &target_ico, &programs.join("FreeView.lnk"));
            }
        }

        // 4. 제어판 언인스톨러 등록
        register_uninstaller(&target_exe, &target_uninst, &target_ico);

        // 5. 완료 상태 전환
        IS_FINISHED.store(true, Ordering::SeqCst);

        if let Some(ref c) = CONTROLS {
            SendMessageW(c.h_progress, PBM_SETPOS, 100, 0);
            SetWindowTextW(c.h_lbl_status, to_wide_null("✓ 설치 완료! [마침]을 누르면 FreeView가 실행됩니다.").as_ptr());
            SetWindowTextW(c.h_btn_install, to_wide_null("마침 (&F)").as_ptr());
            EnableWindow(c.h_btn_install, 1);
            InvalidateRect(c.h_btn_install, std::ptr::null(), 1);
            ShowWindow(c.h_btn_cancel, SW_HIDE);
        }
    }
}

fn do_uninstall_gui() {
    let install_dir = get_install_dir();
    let target_exe = install_dir.join("freeview.exe");

    // 네트워크 프록시 복원
    let _ = Command::new(&target_exe)
        .creation_flags(CREATE_NO_WINDOW)
        .arg("--restore")
        .output();

    // 프로세스 종료
    let _ = Command::new("taskkill")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["/F", "/IM", "freeview.exe"])
        .output();

    // 바로가기 삭제
    if let Some(desktop) = get_special_folder("Desktop") {
        let _ = fs::remove_file(desktop.join("FreeView.lnk"));
    }
    if let Some(programs) = get_special_folder("Programs") {
        let _ = fs::remove_file(programs.join("FreeView.lnk"));
    }

    // 제어판 키 삭제
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Uninstall", KEY_WRITE) {
        let _ = key.delete_subkey_all("FreeView");
    }

    if let Ok(run_key) = hkcu.open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_WRITE) {
        let _ = run_key.delete_value("FreeView");
    }

    // 폴더 삭제 스케줄 (cmd 창 숨김)
    let delete_cmd = format!(
        "Start-Sleep -Seconds 1; Remove-Item -Recurse -Force '{}'",
        install_dir.to_string_lossy()
    );
    let _ = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-Command", &delete_cmd])
        .spawn();

    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            to_wide_null("FreeView가 시스템에서 완전히 제거되었습니다.").as_ptr(),
            to_wide_null("제거 완료").as_ptr(),
            windows_sys::Win32::UI::WindowsAndMessaging::MB_OK | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION,
        );
    }
}

fn get_install_dir() -> PathBuf {
    let base = match std::env::var_os("LOCALAPPDATA") {
        Some(val) => PathBuf::from(val),
        None => std::env::temp_dir(),
    };
    base.join("Programs").join("FreeView")
}

fn get_special_folder(folder_name: &str) -> Option<PathBuf> {
    let script = format!("[Environment]::GetFolderPath('{}')", folder_name);
    let output = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !path_str.is_empty() {
        Some(PathBuf::from(path_str))
    } else {
        None
    }
}

fn create_shortcut(target_exe: &Path, icon_path: &Path, shortcut_path: &Path) {
    let script = format!(
        "$ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut('{}'); $s.TargetPath = '{}'; $s.IconLocation = '{}'; $s.WorkingDirectory = '{}'; $s.Save()",
        shortcut_path.to_string_lossy().replace('\'', "''"),
        target_exe.to_string_lossy().replace('\'', "''"),
        icon_path.to_string_lossy().replace('\'', "''"),
        target_exe.parent().unwrap_or(target_exe).to_string_lossy().replace('\'', "''")
    );

    let _ = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output();
}

fn register_uninstaller(target_exe: &Path, target_uninst: &Path, target_ico: &Path) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((key, _)) = hkcu.create_subkey(UNINSTALL_KEY_PATH) {
        let _ = key.set_value("DisplayName", &"FreeView 3.1");
        let _ = key.set_value("DisplayVersion", &"3.1.0");
        let _ = key.set_value("Publisher", &"QuarterPi & knoblab");
        let _ = key.set_value("DisplayIcon", &target_ico.to_string_lossy().as_ref());
        let uninst_cmd = format!("\"{}\" --uninstall", target_uninst.to_string_lossy());
        let _ = key.set_value("UninstallString", &uninst_cmd);
        let _ = key.set_value("InstallLocation", &target_exe.parent().unwrap_or(target_exe).to_string_lossy().as_ref());
        let _ = key.set_value("NoModify", &1u32);
        let _ = key.set_value("NoRepair", &1u32);
    }
}

unsafe fn apply_dark_mode(hwnd: HWND) {
    let dwmapi = LoadLibraryW(to_wide_null("dwmapi.dll").as_ptr());
    if dwmapi != std::ptr::null_mut() {
        let func = GetProcAddress(dwmapi, b"DwmSetWindowAttribute\0".as_ptr());
        if let Some(f) = func {
            type DwmSetWindowAttributeFn = unsafe extern "system" fn(HWND, u32, *const std::ffi::c_void, u32) -> i32;
            let dwm_set_attr: DwmSetWindowAttributeFn = std::mem::transmute(f);
            let dark_mode: i32 = 1;
            dwm_set_attr(hwnd, 20, &dark_mode as *const _ as _, 4);
            dwm_set_attr(hwnd, 19, &dark_mode as *const _ as _, 4);
        }
    }
}

unsafe fn apply_dark_theme(hwnd: HWND) {
    let uxtheme = LoadLibraryW(to_wide_null("uxtheme.dll").as_ptr());
    if uxtheme != std::ptr::null_mut() {
        let func = GetProcAddress(uxtheme, b"SetWindowTheme\0".as_ptr());
        if let Some(f) = func {
            type SetWindowThemeFn = unsafe extern "system" fn(HWND, *const u16, *const u16) -> i32;
            let set_theme: SetWindowThemeFn = std::mem::transmute(f);
            let theme_name = to_wide_null("DarkMode_Explorer");
            set_theme(hwnd, theme_name.as_ptr(), std::ptr::null());
        }
    }
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
