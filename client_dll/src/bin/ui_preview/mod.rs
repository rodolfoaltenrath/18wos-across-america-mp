//! GDI preview in OUR OWN window/DIB. Never paints/hooks/injects a game window.
use anyhow::{Result, ensure};
use mod_ui::{
    flow::{Action, App, Effect, Screen},
    locale,
    view::{Kind, MAIN_PANEL, Rect as UiRect, Transform, View},
};
use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{GR_GDIOBJECTS, GetCurrentProcess, GetGuiResources},
    },
    UI::{
        Input::KeyboardAndMouse::{GetKeyState, SetFocus},
        WindowsAndMessaging::*,
    },
};
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn rgb(r: u32, g: u32, b: u32) -> u32 {
    r | (g << 8) | (b << 16)
}
struct Object(HGDIOBJ);
impl Drop for Object {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                DeleteObject(self.0);
            }
        }
    }
}
fn brush(color: u32) -> Result<Object> {
    let b = unsafe { CreateSolidBrush(color) };
    ensure!(!b.is_null(), "CreateSolidBrush failed");
    Ok(Object(b))
}
fn font(size: i32, bold: bool) -> Result<Object> {
    let face = wide("Segoe UI");
    let f = unsafe {
        CreateFontW(
            -size,
            0,
            0,
            0,
            if bold { 700 } else { 400 },
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            (DEFAULT_PITCH | FF_SWISS) as u32,
            face.as_ptr(),
        )
    };
    ensure!(!f.is_null(), "CreateFontW failed");
    Ok(Object(f))
}
fn rect(t: Transform, r: UiRect) -> RECT {
    RECT {
        left: (t.x + r.x as f64 * t.scale).round() as i32,
        top: (t.y + r.y as f64 * t.scale).round() as i32,
        right: (t.x + (r.x + r.w) as f64 * t.scale).round() as i32,
        bottom: (t.y + (r.y + r.h) as f64 * t.scale).round() as i32,
    }
}
fn fill(dc: HDC, t: Transform, r: UiRect, b: &Object) {
    let r = rect(t, r);
    unsafe {
        FillRect(dc, &r, b.0);
    }
}
fn text(dc: HDC, t: Transform, r: UiRect, value: &str, f: &Object, color: u32) {
    let mut r = rect(t, r);
    let text = wide(value);
    unsafe {
        let old = SelectObject(dc, f.0);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        DrawTextW(
            dc,
            text.as_ptr(),
            (text.len() - 1) as i32,
            &mut r,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(dc, old);
    }
}
fn paint(dc: HDC, width: i32, height: i32, app: &App, focus: Option<u16>) -> Result<()> {
    let t = Transform::new(width, height).ok_or_else(|| anyhow::anyhow!("invalid viewport"))?;
    let main = app.screen == Screen::Main;
    let bg = brush(rgb(13, 20, 29))?;
    let panel = brush(rgb(23, 34, 46))?;
    let accent = brush(rgb(239, 166, 65))?;
    let field = brush(rgb(34, 47, 62))?;
    let small = font(
        ((if main { 13.0 } else { 17.0 }) * t.scale).max(10.0) as i32,
        false,
    )?;
    let body = font(
        ((if main { 18.0 } else { 20.0 }) * t.scale).max(11.0) as i32,
        false,
    )?;
    let title = font(
        ((if main { 24.0 } else { 36.0 }) * t.scale).max(14.0) as i32,
        true,
    )?;
    let all = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    unsafe {
        FillRect(dc, &all, bg.0);
    }
    fill(
        dc,
        t,
        if main {
            MAIN_PANEL
        } else {
            UiRect {
                x: 30,
                y: 30,
                w: 964,
                h: 708,
            }
        },
        &panel,
    );
    fill(
        dc,
        t,
        if main {
            UiRect {
                x: 47,
                y: 326,
                w: 68,
                h: 3,
            }
        } else {
            UiRect {
                x: 72,
                y: 102,
                w: 68,
                h: 4,
            }
        },
        &accent,
    );
    if !main {
        text(
            dc,
            t,
            UiRect {
                x: 72,
                y: 52,
                w: 880,
                h: 35,
            },
            locale::text("brand", app.language),
            &body,
            rgb(239, 166, 65),
        );
    }
    let view = View::build(app);
    text(
        dc,
        t,
        if main {
            UiRect {
                x: 47,
                y: 292,
                w: 280,
                h: 28,
            }
        } else {
            UiRect {
                x: 72,
                y: 122,
                w: 880,
                h: 54,
            }
        },
        &view.title,
        &title,
        rgb(239, 242, 246),
    );
    if main {
        text(
            dc,
            t,
            UiRect {
                x: 440,
                y: 320,
                w: 540,
                h: 50,
            },
            locale::text("menu.background", app.language),
            &title,
            rgb(165, 181, 198),
        );
        text(
            dc,
            t,
            UiRect {
                x: 440,
                y: 375,
                w: 540,
                h: 35,
            },
            locale::text("menu.background.note", app.language),
            &small,
            rgb(165, 181, 198),
        );
    } else {
        text(
            dc,
            t,
            UiRect {
                x: 72,
                y: 177,
                w: 880,
                h: 30,
            },
            &view.subtitle,
            &small,
            rgb(165, 181, 198),
        );
    }
    for w in view.widgets {
        let label = matches!(w.kind, Kind::Label);
        if !label {
            fill(
                dc,
                t,
                w.rect,
                if w.primary && w.enabled {
                    &accent
                } else {
                    &field
                },
            );
            if focus == Some(w.id) {
                fill(
                    dc,
                    t,
                    UiRect {
                        x: w.rect.x,
                        y: w.rect.y,
                        w: 4,
                        h: w.rect.h,
                    },
                    &accent,
                );
            }
        }
        let value = match w.kind {
            Kind::Toggle { checked, .. } => {
                format!("{}  {}", if checked { "[x]" } else { "[ ]" }, w.text)
            }
            _ => w.text,
        };
        text(
            dc,
            t,
            UiRect {
                x: w.rect.x + 14,
                y: w.rect.y,
                w: w.rect.w - 28,
                h: w.rect.h,
            },
            &value,
            &body,
            if !w.enabled {
                rgb(128, 146, 164)
            } else if w.primary {
                rgb(18, 26, 34)
            } else {
                rgb(229, 236, 242)
            },
        );
    }
    if app.preview {
        text(
            dc,
            t,
            if main {
                UiRect {
                    x: 47,
                    y: 646,
                    w: 280,
                    h: 24,
                }
            } else {
                UiRect {
                    x: 490,
                    y: 690,
                    w: 462,
                    h: 24,
                }
            },
            locale::text("preview", app.language),
            &small,
            rgb(165, 181, 198),
        );
    }
    Ok(())
}
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut c_void,
}
impl Surface {
    fn new() -> Result<Self> {
        unsafe {
            let dc = CreateCompatibleDC(ptr::null_mut());
            ensure!(!dc.is_null(), "memory DC failed");
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = 1024;
            info.bmiHeader.biHeight = -768;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB;
            let mut bits = ptr::null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, ptr::null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);
                anyhow::bail!("DIB allocation failed");
            }
            let old = SelectObject(dc, bitmap);
            Ok(Self {
                dc,
                bitmap,
                old,
                bits,
            })
        }
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
pub fn bitmap(app: &App) -> Result<Vec<u8>> {
    let s = Surface::new()?;
    paint(s.dc, 1024, 768, app, None)?;
    unsafe {
        GdiFlush();
    }
    // SAFETY: fixed-size, top-down 32bpp DIB owned/selected by this thread; flushed,
    // no concurrent writers, copied before Surface releases the backing storage.
    let pixels = unsafe { std::slice::from_raw_parts(s.bits as *const u8, 1024 * 768 * 4) };
    let mut bytes = Vec::with_capacity(54 + pixels.len());
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&54u32.to_le_bytes());
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&1024i32.to_le_bytes());
    bytes.extend_from_slice(&(-768i32).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&[0; 24]);
    bytes.extend_from_slice(pixels);
    Ok(bytes)
}
pub fn self_test() -> Result<serde_json::Value> {
    let warm = bitmap(&App::default())?;
    ensure!(warm.len() == 54 + 1024 * 768 * 4, "BMP bounds");
    let before = unsafe { GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) };
    for _ in 0..3 {
        for &screen in mod_ui::flow::SCREENS {
            let b = bitmap(&App::preview(screen, locale::Language::PtBr))?;
            let colors: std::collections::BTreeSet<_> = b[54..]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2]])
                .collect();
            ensure!(colors.len() > 8, "blank/monochrome screen");
        }
    }
    let after = unsafe { GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) };
    ensure!(after <= before, "GDI objects retained after repeated draws");
    let s = Surface::new()?;
    let f = font(20, false)?;
    let old = unsafe { SelectObject(s.dc, f.0) };
    let sample: Vec<u16> = "áàâãéêíóôõúüçÁÃÉÊÓÕÇ".encode_utf16().collect();
    let mut glyphs = vec![0u16; sample.len()];
    let n = unsafe {
        GetGlyphIndicesW(
            s.dc,
            sample.as_ptr(),
            sample.len() as i32,
            glyphs.as_mut_ptr(),
            GGI_MARK_NONEXISTING_GLYPHS,
        )
    };
    unsafe {
        SelectObject(s.dc, old);
    }
    ensure!(
        n != u32::MAX && !glyphs.contains(&0xffff),
        "preview font lacks PT-BR glyphs"
    );
    Ok(
        serde_json::json!({"preview_only":true,"screens":mod_ui::flow::SCREENS.len(),"repeated_draws":27,"gdi_before":before,"gdi_after":after,"pt_br_glyphs":true,"game_tested":false,"network_started":false,"native_fonts_tested":false}),
    )
}
struct State {
    app: App,
    focus: Option<u16>,
    select_all: bool,
}
impl State {
    fn action(&mut self, a: Action) -> bool {
        self.focus = None;
        self.select_all = false;
        match self.app.act(a) {
            Some(Effect::Exit) => true,
            Some(
                Effect::Host { request, .. }
                | Effect::Join { request, .. }
                | Effect::Discover { request },
            ) => {
                self.app.complete(request, false);
                false
            }
            _ => false,
        }
    }
    fn focus_next(&mut self) {
        let v = View::build(&self.app);
        let ids: Vec<_> = v
            .widgets
            .iter()
            .filter(|w| w.enabled && !matches!(w.kind, Kind::Label))
            .map(|w| w.id)
            .collect();
        self.focus = ids
            .get(
                self.focus
                    .and_then(|id| ids.iter().position(|v| *v == id))
                    .map_or(0, |i| (i + 1) % ids.len().max(1)),
            )
            .copied();
    }
    fn input(&mut self, ch: Option<char>) {
        let v = View::build(&self.app);
        if let Some(w) = v
            .widgets
            .iter()
            .find(|w| Some(w.id) == self.focus && w.enabled)
            && let Kind::Input { field } = w.kind
        {
            let mut text = if self.select_all {
                String::new()
            } else {
                self.app.field(field).to_owned()
            };
            self.select_all = false;
            if let Some(c) = ch {
                text.push(c);
            } else {
                text.pop();
            }
            let _ = self.app.set_field(field, text);
        }
    }
}
unsafe extern "system" fn proc(window: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    catch_unwind(AssertUnwindSafe(|| unsafe { dispatch(window, msg, w, l) })).unwrap_or_else(|_| {
        unsafe {
            PostQuitMessage(1);
        }
        0
    })
}
unsafe fn dispatch(window: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let c = unsafe { &*(l as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, c.lpCreateParams as _);
        }
        return 1;
    }
    // DestroyWindow/SetFocus may synchronously reenter the window procedure.
    // Those lifecycle/default messages must never borrow the UI state while
    // the outer input/paint callback still owns it.
    match msg {
        WM_CLOSE => {
            unsafe { DestroyWindow(window) };
            return 0;
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            return 0;
        }
        WM_NCDESTROY => {
            unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, 0) };
            return unsafe { DefWindowProcW(window, msg, w, l) };
        }
        WM_SIZE => {
            unsafe { InvalidateRect(window, ptr::null(), 0) };
            return 0;
        }
        WM_PAINT | WM_LBUTTONDOWN | WM_KEYDOWN | WM_CHAR => {}
        _ => return unsafe { DefWindowProcW(window, msg, w, l) },
    }
    let p = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *mut State;
    if p.is_null() {
        return unsafe { DefWindowProcW(window, msg, w, l) };
    }
    // SAFETY: run owns the Box until this window's message loop ends; all messages
    // are dispatched on this thread; lifecycle/default reentry above never
    // borrows state. No callback pumps input or paint while borrowing it.
    let s = unsafe { &mut *p };
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
            let dc = unsafe { BeginPaint(window, &mut ps) };
            let mut r = RECT::default();
            unsafe {
                GetClientRect(window, &mut r);
            }
            let result = if r.right > 0 && r.bottom > 0 {
                paint(dc, r.right, r.bottom, &s.app, s.focus)
            } else {
                Ok(())
            };
            unsafe {
                EndPaint(window, &ps);
            }
            if result.is_err() {
                unsafe {
                    PostQuitMessage(1);
                }
            }
            0
        }
        WM_LBUTTONDOWN => {
            let x = (l as u32 & 0xffff) as i16 as i32;
            let y = ((l as u32 >> 16) & 0xffff) as i16 as i32;
            let mut r = RECT::default();
            unsafe {
                GetClientRect(window, &mut r);
            }
            if let Some(t) = Transform::new(r.right, r.bottom) {
                let (x, y) = t.logical(x, y);
                let view = View::build(&s.app);
                if let Some(widget) = view.hit(x, y) {
                    s.focus = Some(widget.id);
                    s.select_all = matches!(widget.kind, Kind::Input { .. });
                    match &widget.kind {
                        Kind::Button { action } | Kind::Toggle { action, .. }
                            if s.action(action.clone()) =>
                        unsafe {
                            DestroyWindow(window);
                        },
                        _ => {}
                    }
                }
            }
            unsafe {
                SetFocus(window);
                InvalidateRect(window, ptr::null(), 0);
            }
            0
        }
        WM_KEYDOWN => {
            match w as u32 {
                65 if unsafe { GetKeyState(0x11) } < 0 => s.select_all = true,
                9 => s.focus_next(),
                8 => s.input(None),
                27 => {
                    s.action(Action::Back);
                }
                13 => {
                    let v = View::build(&s.app);
                    if let Some(widget) = v.widgets.iter().find(|i| Some(i.id) == s.focus)
                        && let Kind::Button { action } | Kind::Toggle { action, .. } = &widget.kind
                        && s.action(action.clone())
                    {
                        unsafe {
                            DestroyWindow(window);
                        }
                    }
                }
                _ => {}
            }
            unsafe {
                InvalidateRect(window, ptr::null(), 0);
            }
            0
        }
        WM_CHAR => {
            if unsafe { GetKeyState(0x11) } >= 0
                && let Some(c) = char::from_u32(w as u32)
                && !c.is_control()
            {
                s.input(Some(c));
                unsafe {
                    InvalidateRect(window, ptr::null(), 0);
                }
            }
            0
        }
        _ => unsafe { DefWindowProcW(window, msg, w, l) },
    }
}
pub fn run(app: App) -> Result<()> {
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let class = wide("AcrossAmericaMP.OwnUiPreview.v1");
        let title = wide("Across America MP — Prévia local da interface");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            ..std::mem::zeroed()
        };
        ensure!(RegisterClassW(&wc) != 0, "window class registration failed");
        let mut state = Box::new(State {
            app,
            focus: None,
            select_all: false,
        });
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1120,
            880,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            (&mut *state as *mut State).cast(),
        );
        if window.is_null() {
            UnregisterClassW(class.as_ptr(), instance);
            anyhow::bail!("preview window creation failed");
        }
        let mut msg: MSG = std::mem::zeroed();
        let result = loop {
            let n = GetMessageW(&mut msg, ptr::null_mut(), 0, 0);
            if n <= 0 {
                break n;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        };
        if IsWindow(window) != 0 {
            DestroyWindow(window);
        }
        UnregisterClassW(class.as_ptr(), instance);
        ensure!(result >= 0 && msg.wParam == 0, "preview event loop failed");
        Ok(())
    }
}
