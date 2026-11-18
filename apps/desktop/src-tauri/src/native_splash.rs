#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)]
mod platform {
    use std::{
        ffi::c_void,
        ptr,
        sync::{
            atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering},
            Mutex, OnceLock,
        },
        thread,
        time::Instant,
    };

    const LOGICAL_CARD_W: f32 = 340.0;
    const LOGICAL_CARD_H: f32 = 216.0;
    const LOGICAL_SHADOW_PAD: f32 = 20.0;
    const LOGICAL_TOTAL_W: f32 = LOGICAL_CARD_W + LOGICAL_SHADOW_PAD * 2.0;
    const LOGICAL_TOTAL_H: f32 = LOGICAL_CARD_H + LOGICAL_SHADOW_PAD * 2.0;

    const WM_DESTROY: u32 = 0x0002;
    const WM_TIMER: u32 = 0x0113;
    const WM_APP_CLOSE: u32 = 0x8001;
    const TIMER_ID: usize = 1;
    const FRAME_MS: u32 = 16;

    const WS_POPUP: u32 = 0x8000_0000;
    const WS_VISIBLE: u32 = 0x1000_0000;
    const WS_EX_TOPMOST: u32 = 0x0000_0008;
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
    const WS_EX_LAYERED: u32 = 0x0008_0000;
    const WS_EX_NOACTIVATE: u32 = 0x0800_0000;

    const ULW_ALPHA: u32 = 0x0000_0002;
    const AC_SRC_OVER: u8 = 0x00;
    const AC_SRC_ALPHA: u8 = 0x01;
    const BI_RGB: u32 = 0;
    const DIB_RGB_COLORS: u32 = 0;

    const MONITOR_DEFAULTTONEAREST: u32 = 0x0000_0002;
    const SM_CXSCREEN: i32 = 0;
    const SM_CYSCREEN: i32 = 1;

    const PIXEL_FORMAT_32BPP_PARGB: i32 = 11 | (32 << 8) | 0x0004_0000 | 0x0008_0000 | 0x0002_0000;
    const UNIT_PIXEL: i32 = 2;
    const LINE_CAP_ROUND: i32 = 2;
    const SMOOTHING_MODE_ANTI_ALIAS: i32 = 4;
    const TEXT_RENDERING_ANTI_ALIAS: i32 = 4;
    const FONT_STYLE_REGULAR: i32 = 0;
    const FONT_STYLE_BOLD: i32 = 1;
    const STRING_ALIGN_CENTER: i32 = 1;
    const FILL_MODE_ALTERNATE: i32 = 0;
    const WRAP_MODE_TILE_FLIP_X: i32 = 1;

    type HWND = isize;
    type HDC = isize;
    type HBITMAP = isize;
    type HGDIOBJ = isize;
    type HINSTANCE = isize;
    type HMONITOR = isize;
    type WPARAM = usize;
    type LPARAM = isize;
    type LRESULT = isize;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct POINT {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SIZE {
        cx: i32,
        cy: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RECT {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct MONITORINFO {
        cb_size: u32,
        rc_monitor: RECT,
        rc_work: RECT,
        dw_flags: u32,
    }

    #[repr(C)]
    struct BLENDFUNCTION {
        blend_op: u8,
        blend_flags: u8,
        source_constant_alpha: u8,
        alpha_format: u8,
    }

    #[repr(C)]
    struct BITMAPINFOHEADER {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_xpels_per_meter: i32,
        bi_ypels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    #[repr(C)]
    struct BITMAPINFO {
        bmi_header: BITMAPINFOHEADER,
        bmi_colors: [u32; 1],
    }

    #[repr(C)]
    struct WNDCLASSEXW {
        cb_size: u32,
        style: u32,
        lpfn_wnd_proc: Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>,
        cb_cls_extra: i32,
        cb_wnd_extra: i32,
        h_instance: HINSTANCE,
        h_icon: isize,
        h_cursor: isize,
        hbr_background: isize,
        lpsz_menu_name: *const u16,
        lpsz_class_name: *const u16,
        h_icon_sm: isize,
    }

    #[repr(C)]
    struct MSG {
        hwnd: HWND,
        message: u32,
        w_param: WPARAM,
        l_param: LPARAM,
        time: u32,
        pt: POINT,
    }

    #[repr(C)]
    struct GdiplusStartupInput {
        gdiplus_version: u32,
        debug_event_callback: *mut c_void,
        suppress_background_thread: i32,
        suppress_external_codecs: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RectF {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct PointF {
        x: f32,
        y: f32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn RegisterClassExW(lpwcx: *const WNDCLASSEXW) -> u16;
        fn CreateWindowExW(
            dw_ex_style: u32,
            lp_class_name: *const u16,
            lp_window_name: *const u16,
            dw_style: u32,
            x: i32,
            y: i32,
            n_width: i32,
            n_height: i32,
            h_wnd_parent: HWND,
            h_menu: isize,
            h_instance: HINSTANCE,
            lp_param: *mut c_void,
        ) -> HWND;
        fn DestroyWindow(h_wnd: HWND) -> i32;
        fn DefWindowProcW(h_wnd: HWND, msg: u32, w_param: WPARAM, l_param: LPARAM) -> LRESULT;
        fn PostMessageW(h_wnd: HWND, msg: u32, w_param: WPARAM, l_param: LPARAM) -> i32;
        fn GetMessageW(
            lp_msg: *mut MSG,
            h_wnd: HWND,
            w_msg_filter_min: u32,
            w_msg_filter_max: u32,
        ) -> i32;
        fn TranslateMessage(lp_msg: *const MSG) -> i32;
        fn DispatchMessageW(lp_msg: *const MSG) -> LRESULT;
        fn SetTimer(h_wnd: HWND, n_id_event: usize, u_elapse: u32, lp_timer_func: isize) -> usize;
        fn KillTimer(h_wnd: HWND, u_id_event: usize) -> i32;
        fn GetSystemMetrics(n_index: i32) -> i32;
        fn GetCursorPos(lp_point: *mut POINT) -> i32;
        fn MonitorFromPoint(pt: POINT, dw_flags: u32) -> HMONITOR;
        fn GetMonitorInfoW(h_monitor: HMONITOR, lpmi: *mut MONITORINFO) -> i32;
        fn GetDpiForWindow(hwnd: HWND) -> u32;
        fn GetDC(h_wnd: HWND) -> HDC;
        fn ReleaseDC(h_wnd: HWND, h_dc: HDC) -> i32;
        fn UpdateLayeredWindow(
            h_wnd: HWND,
            hdc_dst: HDC,
            ppt_dst: *const POINT,
            psize: *const SIZE,
            hdc_src: HDC,
            ppt_src: *const POINT,
            cr_key: u32,
            pblend: *const BLENDFUNCTION,
            dw_flags: u32,
        ) -> i32;
        fn PostQuitMessage(n_exit_code: i32);
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateCompatibleDC(hdc: HDC) -> HDC;
        fn DeleteDC(hdc: HDC) -> i32;
        fn CreateDIBSection(
            hdc: HDC,
            pbmi: *const BITMAPINFO,
            usage: u32,
            ppv_bits: *mut *mut c_void,
            h_section: isize,
            offset: u32,
        ) -> HBITMAP;
        fn SelectObject(hdc: HDC, h: HGDIOBJ) -> HGDIOBJ;
        fn DeleteObject(ho: HGDIOBJ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(lp_module_name: *const u16) -> HINSTANCE;
    }

    #[link(name = "gdiplus")]
    extern "system" {
        fn GdiplusStartup(
            token: *mut usize,
            input: *const GdiplusStartupInput,
            output: *mut c_void,
        ) -> i32;
        fn GdiplusShutdown(token: usize);
        fn GdipCreateBitmapFromScan0(
            width: i32,
            height: i32,
            stride: i32,
            format: i32,
            scan0: *mut u8,
            bitmap: *mut *mut c_void,
        ) -> i32;
        fn GdipDisposeImage(image: *mut c_void) -> i32;
        fn GdipGetImageGraphicsContext(image: *mut c_void, graphics: *mut *mut c_void) -> i32;
        fn GdipDeleteGraphics(graphics: *mut c_void) -> i32;
        fn GdipSetSmoothingMode(graphics: *mut c_void, mode: i32) -> i32;
        fn GdipSetTextRenderingHint(graphics: *mut c_void, mode: i32) -> i32;
        fn GdipGraphicsClear(graphics: *mut c_void, color: u32) -> i32;
        fn GdipCreateSolidFill(color: u32, brush: *mut *mut c_void) -> i32;
        fn GdipCreateLineBrush(
            point1: *const PointF,
            point2: *const PointF,
            color1: u32,
            color2: u32,
            wrap_mode: i32,
            line_gradient: *mut *mut c_void,
        ) -> i32;
        fn GdipDeleteBrush(brush: *mut c_void) -> i32;
        fn GdipCreatePen1(color: u32, width: f32, unit: i32, pen: *mut *mut c_void) -> i32;
        fn GdipDeletePen(pen: *mut c_void) -> i32;
        fn GdipSetPenStartCap(pen: *mut c_void, cap: i32) -> i32;
        fn GdipSetPenEndCap(pen: *mut c_void, cap: i32) -> i32;
        fn GdipCreatePath(fill_mode: i32, path: *mut *mut c_void) -> i32;
        fn GdipDeletePath(path: *mut c_void) -> i32;
        fn GdipClosePathFigure(path: *mut c_void) -> i32;
        fn GdipAddPathArc(
            path: *mut c_void,
            x: f32,
            y: f32,
            width: f32,
            height: f32,
            start_angle: f32,
            sweep_angle: f32,
        ) -> i32;
        fn GdipFillPath(graphics: *mut c_void, brush: *mut c_void, path: *mut c_void) -> i32;
        fn GdipDrawPath(graphics: *mut c_void, pen: *mut c_void, path: *mut c_void) -> i32;
        fn GdipDrawArc(
            graphics: *mut c_void,
            pen: *mut c_void,
            x: f32,
            y: f32,
            width: f32,
            height: f32,
            start_angle: f32,
            sweep_angle: f32,
        ) -> i32;
        fn GdipFillEllipse(
            graphics: *mut c_void,
            brush: *mut c_void,
            x: f32,
            y: f32,
            width: f32,
            height: f32,
        ) -> i32;
        fn GdipCreateFontFamilyFromName(
            name: *const u16,
            font_collection: *mut c_void,
            font_family: *mut *mut c_void,
        ) -> i32;
        fn GdipDeleteFontFamily(font_family: *mut c_void) -> i32;
        fn GdipCreateFont(
            font_family: *const c_void,
            em_size: f32,
            style: i32,
            unit: i32,
            font: *mut *mut c_void,
        ) -> i32;
        fn GdipDeleteFont(font: *mut c_void) -> i32;
        fn GdipCreateStringFormat(
            format_attributes: i32,
            language: u16,
            format: *mut *mut c_void,
        ) -> i32;
        fn GdipDeleteStringFormat(format: *mut c_void) -> i32;
        fn GdipSetStringFormatAlign(format: *mut c_void, align: i32) -> i32;
        fn GdipSetStringFormatLineAlign(format: *mut c_void, align: i32) -> i32;
        fn GdipDrawString(
            graphics: *mut c_void,
            string: *const u16,
            length: i32,
            font: *const c_void,
            layout_rect: *const RectF,
            string_format: *const c_void,
            brush: *const c_void,
        ) -> i32;
    }

    struct SplashState {
        generation: AtomicU64,
        active: AtomicBool,
        hwnd: AtomicIsize,
        lock: Mutex<()>,
    }

    static SPLASH: OnceLock<SplashState> = OnceLock::new();
    static CLASS_REGISTERED: OnceLock<u16> = OnceLock::new();

    fn state() -> &'static SplashState {
        SPLASH.get_or_init(|| SplashState {
            generation: AtomicU64::new(0),
            active: AtomicBool::new(false),
            hwnd: AtomicIsize::new(0),
            lock: Mutex::new(()),
        })
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
        ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }

    struct SplashPalette {
        shadow_rgb: (u8, u8, u8),
        shadow_base_alpha: u8,
        shadow_step_alpha: u8,
        card_bg: u32,
        card_border: u32,
        halo_outer_rgb: (u8, u8, u8),
        halo_outer_alpha: f32,
        halo_inner_rgb: (u8, u8, u8),
        halo_inner_alpha: f32,
        ring_track: u32,
        ring_primary: u32,
        ring_secondary: u32,
        badge_bg: u32,
        badge_border: u32,
        bar_top: u32,
        bar_bottom: u32,
        title: u32,
        subtitle: u32,
        track_bg: u32,
        beam_start: u32,
        beam_end: u32,
    }

    impl SplashPalette {
        fn for_theme(is_dark: bool) -> Self {
            if is_dark {
                Self {
                    shadow_rgb: (3, 5, 10),
                    shadow_base_alpha: 7,
                    shadow_step_alpha: 4,
                    card_bg: argb(252, 18, 22, 31),
                    card_border: argb(30, 255, 255, 255),
                    halo_outer_rgb: (94, 106, 210),
                    halo_outer_alpha: 18.0,
                    halo_inner_rgb: (142, 153, 243),
                    halo_inner_alpha: 28.0,
                    ring_track: argb(24, 255, 255, 255),
                    ring_primary: argb(245, 142, 153, 243),
                    ring_secondary: argb(145, 94, 106, 210),
                    badge_bg: argb(255, 21, 25, 34),
                    badge_border: argb(38, 255, 255, 255),
                    bar_top: argb(255, 148, 160, 255),
                    bar_bottom: argb(255, 94, 106, 210),
                    title: argb(248, 243, 245, 248),
                    subtitle: argb(220, 146, 158, 174),
                    track_bg: argb(24, 255, 255, 255),
                    beam_start: argb(180, 94, 106, 210),
                    beam_end: argb(255, 165, 174, 255),
                }
            } else {
                Self {
                    shadow_rgb: (15, 23, 42),
                    shadow_base_alpha: 5,
                    shadow_step_alpha: 3,
                    card_bg: argb(253, 255, 255, 255),
                    card_border: argb(28, 15, 23, 42),
                    halo_outer_rgb: (79, 91, 213),
                    halo_outer_alpha: 13.0,
                    halo_inner_rgb: (104, 115, 227),
                    halo_inner_alpha: 18.0,
                    ring_track: argb(22, 15, 23, 42),
                    ring_primary: argb(245, 79, 91, 213),
                    ring_secondary: argb(135, 116, 128, 234),
                    badge_bg: argb(255, 246, 247, 249),
                    badge_border: argb(26, 15, 23, 42),
                    bar_top: argb(255, 104, 115, 227),
                    bar_bottom: argb(255, 79, 91, 213),
                    title: argb(248, 15, 23, 42),
                    subtitle: argb(225, 82, 96, 113),
                    track_bg: argb(22, 15, 23, 42),
                    beam_start: argb(165, 116, 128, 234),
                    beam_end: argb(255, 79, 91, 213),
                }
            }
        }
    }

    struct Canvas {
        screen_dc: HDC,
        mem_dc: HDC,
        dib: HBITMAP,
        prev_obj: HGDIOBJ,
        bits: *mut u8,
        bitmap: *mut c_void,
        graphics: *mut c_void,
        width: i32,
        height: i32,
        pos: POINT,
        scale: f32,
        is_dark: bool,
        started: Instant,
        font_family: *mut c_void,
        title_font: *mut c_void,
        subtitle_font: *mut c_void,
        str_format: *mut c_void,
    }

    impl Canvas {
        unsafe fn new(
            hwnd: HWND,
            pos: POINT,
            width: i32,
            height: i32,
            scale: f32,
            is_dark: bool,
        ) -> Option<Self> {
            let screen_dc = GetDC(0);
            let mem_dc = CreateCompatibleDC(screen_dc);
            if mem_dc == 0 {
                ReleaseDC(0, screen_dc);
                return None;
            }
            let bmi = BITMAPINFO {
                bmi_header: BITMAPINFOHEADER {
                    bi_size: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    bi_width: width,
                    bi_height: -height, // top-down DIB matching GDI+ origin
                    bi_planes: 1,
                    bi_bit_count: 32,
                    bi_compression: BI_RGB,
                    bi_size_image: 0,
                    bi_xpels_per_meter: 0,
                    bi_ypels_per_meter: 0,
                    bi_clr_used: 0,
                    bi_clr_important: 0,
                },
                bmi_colors: [0],
            };
            let mut raw_bits: *mut c_void = ptr::null_mut();
            let dib = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut raw_bits, 0, 0);
            if dib == 0 || raw_bits.is_null() {
                DeleteDC(mem_dc);
                ReleaseDC(0, screen_dc);
                return None;
            }
            let prev_obj = SelectObject(mem_dc, dib);

            let mut bitmap: *mut c_void = ptr::null_mut();
            let stride = width * 4;
            if GdipCreateBitmapFromScan0(
                width,
                height,
                stride,
                PIXEL_FORMAT_32BPP_PARGB,
                raw_bits.cast::<u8>(),
                &mut bitmap,
            ) != 0
                || bitmap.is_null()
            {
                SelectObject(mem_dc, prev_obj);
                DeleteObject(dib);
                DeleteDC(mem_dc);
                ReleaseDC(0, screen_dc);
                return None;
            }

            let mut graphics: *mut c_void = ptr::null_mut();
            if GdipGetImageGraphicsContext(bitmap, &mut graphics) != 0 || graphics.is_null() {
                GdipDisposeImage(bitmap);
                SelectObject(mem_dc, prev_obj);
                DeleteObject(dib);
                DeleteDC(mem_dc);
                ReleaseDC(0, screen_dc);
                return None;
            }
            GdipSetSmoothingMode(graphics, SMOOTHING_MODE_ANTI_ALIAS);
            GdipSetTextRenderingHint(graphics, TEXT_RENDERING_ANTI_ALIAS);

            let mut font_family: *mut c_void = ptr::null_mut();
            let primary = wide("Microsoft YaHei UI");
            if GdipCreateFontFamilyFromName(primary.as_ptr(), ptr::null_mut(), &mut font_family)
                != 0
                || font_family.is_null()
            {
                let fallback = wide("Segoe UI");
                GdipCreateFontFamilyFromName(fallback.as_ptr(), ptr::null_mut(), &mut font_family);
            }

            let mut title_font: *mut c_void = ptr::null_mut();
            let mut subtitle_font: *mut c_void = ptr::null_mut();
            if !font_family.is_null() {
                GdipCreateFont(
                    font_family,
                    16.0 * scale,
                    FONT_STYLE_BOLD,
                    UNIT_PIXEL,
                    &mut title_font,
                );
                GdipCreateFont(
                    font_family,
                    12.0 * scale,
                    FONT_STYLE_REGULAR,
                    UNIT_PIXEL,
                    &mut subtitle_font,
                );
            }

            let mut str_format: *mut c_void = ptr::null_mut();
            GdipCreateStringFormat(0, 0, &mut str_format);
            if !str_format.is_null() {
                GdipSetStringFormatAlign(str_format, STRING_ALIGN_CENTER);
                GdipSetStringFormatLineAlign(str_format, STRING_ALIGN_CENTER);
            }

            let _ = hwnd;
            Some(Self {
                screen_dc,
                mem_dc,
                dib,
                prev_obj,
                bits: raw_bits.cast::<u8>(),
                bitmap,
                graphics,
                width,
                height,
                pos,
                scale,
                is_dark,
                started: Instant::now(),
                font_family,
                title_font,
                subtitle_font,
                str_format,
            })
        }

        unsafe fn render_frame(&self, hwnd: HWND) {
            let elapsed = self.started.elapsed().as_secs_f32();
            let fade = (elapsed / 0.14).clamp(0.0, 1.0);
            let s = self.scale;
            let palette = SplashPalette::for_theme(self.is_dark);
            let _ = self.bits;

            GdipGraphicsClear(self.graphics, 0);

            let pad = LOGICAL_SHADOW_PAD * s;
            let card_w = LOGICAL_CARD_W * s;
            let card_h = LOGICAL_CARD_H * s;
            let radius = 16.0 * s;

            // Layered soft elevation shadow
            for layer in 0..4 {
                let expand = (4 - layer) as f32 * 3.2 * s;
                let alpha = palette.shadow_base_alpha + layer as u8 * palette.shadow_step_alpha;
                let (sr, sg, sb) = palette.shadow_rgb;
                fill_round_rect(
                    self.graphics,
                    pad - expand * 0.5,
                    pad - expand * 0.25 + (3.0 * s),
                    card_w + expand,
                    card_h + expand,
                    radius + expand * 0.35,
                    argb(alpha, sr, sg, sb),
                );
            }

            // Card surface & subtle border matching active theme
            fill_round_rect(
                self.graphics,
                pad,
                pad,
                card_w,
                card_h,
                radius,
                palette.card_bg,
            );
            stroke_round_rect(
                self.graphics,
                RectF {
                    x: pad + 0.5 * s,
                    y: pad + 0.5 * s,
                    width: card_w - s,
                    height: card_h - s,
                },
                radius,
                1.0 * s,
                palette.card_border,
            );

            let cx = pad + card_w * 0.5;
            let cy = pad + 72.0 * s;

            // Breathing ambient indigo halo
            let glow_pulse = 0.72 + 0.28 * (elapsed * 2.1).sin();
            let halo_r = 68.0 * s;
            let (hor, hog, hob) = palette.halo_outer_rgb;
            fill_ellipse(
                self.graphics,
                cx - halo_r,
                cy - halo_r,
                halo_r * 2.0,
                halo_r * 2.0,
                argb((palette.halo_outer_alpha * glow_pulse) as u8, hor, hog, hob),
            );
            let halo_inner = 42.0 * s;
            let (hir, hig, hib) = palette.halo_inner_rgb;
            fill_ellipse(
                self.graphics,
                cx - halo_inner,
                cy - halo_inner,
                halo_inner * 2.0,
                halo_inner * 2.0,
                argb((palette.halo_inner_alpha * glow_pulse) as u8, hir, hig, hib),
            );

            // Outer orbital track
            let ring_r = 34.0 * s;
            let ring_rect = RectF {
                x: cx - ring_r,
                y: cy - ring_r,
                width: ring_r * 2.0,
                height: ring_r * 2.0,
            };
            draw_arc(
                self.graphics,
                ring_rect,
                0.0,
                360.0,
                1.75 * s,
                palette.ring_track,
            );

            // Spinning precision arc (main + trailing accent)
            let angle = (elapsed * 260.0) % 360.0;
            draw_arc(
                self.graphics,
                ring_rect,
                angle,
                96.0,
                2.5 * s,
                palette.ring_primary,
            );
            draw_arc(
                self.graphics,
                ring_rect,
                angle + 180.0,
                42.0,
                2.0 * s,
                palette.ring_secondary,
            );

            // Center glass badge
            let badge_size = 44.0 * s;
            let bx = cx - badge_size * 0.5;
            let by = cy - badge_size * 0.5;
            fill_round_rect(
                self.graphics,
                bx,
                by,
                badge_size,
                badge_size,
                12.0 * s,
                palette.badge_bg,
            );
            stroke_round_rect(
                self.graphics,
                RectF {
                    x: bx + 0.5 * s,
                    y: by + 0.5 * s,
                    width: badge_size - s,
                    height: badge_size - s,
                },
                12.0 * s,
                1.0 * s,
                palette.badge_border,
            );

            // 3 animated telemetry equalizer bars inside the badge
            let bar_w = 3.6 * s;
            let gap = 4.0 * s;
            let phases = [0.0_f32, 1.05, 2.1];
            let base_heights = [11.0_f32, 19.0, 13.5];
            for (idx, (&phase, &base_h)) in phases.iter().zip(base_heights.iter()).enumerate() {
                let wave = 0.65 + 0.35 * ((elapsed * 5.2 - phase).sin() * 0.5 + 0.5);
                let bh = base_h * wave * s;
                let bar_x = cx + (idx as f32 - 1.0) * (bar_w + gap) - bar_w * 0.5;
                let bar_y = cy - bh * 0.5;
                fill_round_rect_gradient(
                    self.graphics,
                    RectF {
                        x: bar_x,
                        y: bar_y,
                        width: bar_w,
                        height: bh,
                    },
                    bar_w * 0.5,
                    palette.bar_top,
                    palette.bar_bottom,
                );
            }

            // Brand title & status text
            if !self.title_font.is_null() && !self.str_format.is_null() {
                let title = wide("TokenScope");
                let rect = RectF {
                    x: pad + 24.0 * s,
                    y: pad + 118.0 * s,
                    width: card_w - 48.0 * s,
                    height: 26.0 * s,
                };
                draw_text(
                    self.graphics,
                    &title,
                    self.title_font,
                    &rect,
                    self.str_format,
                    palette.title,
                );
            }
            if !self.subtitle_font.is_null() && !self.str_format.is_null() {
                let sub = wide("正在唤醒工作区…");
                let rect = RectF {
                    x: pad + 24.0 * s,
                    y: pad + 145.0 * s,
                    width: card_w - 48.0 * s,
                    height: 22.0 * s,
                };
                draw_text(
                    self.graphics,
                    &sub,
                    self.subtitle_font,
                    &rect,
                    self.str_format,
                    palette.subtitle,
                );
            }

            // Indeterminate progress track + sweeping beam
            let track_w = 136.0 * s;
            let track_h = 3.2 * s;
            let track_x = cx - track_w * 0.5;
            let track_y = pad + 178.0 * s;
            fill_round_rect(
                self.graphics,
                track_x,
                track_y,
                track_w,
                track_h,
                track_h * 0.5,
                palette.track_bg,
            );
            let cycle = (elapsed / 1.2).fract();
            let eased = if cycle < 0.5 {
                2.0 * cycle * cycle
            } else {
                1.0 - (-2.0 * cycle + 2.0).powi(2) * 0.5
            };
            let beam_w = track_w * 0.42;
            let beam_raw_x = track_x - beam_w * 0.6 + eased * (track_w + beam_w * 0.2);
            let beam_left = beam_raw_x.max(track_x);
            let beam_right = (beam_raw_x + beam_w).min(track_x + track_w);
            if beam_right > beam_left {
                fill_round_rect_gradient(
                    self.graphics,
                    RectF {
                        x: beam_left,
                        y: track_y,
                        width: beam_right - beam_left,
                        height: track_h,
                    },
                    track_h * 0.5,
                    palette.beam_start,
                    palette.beam_end,
                );
            }

            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let src_pt = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                blend_op: AC_SRC_OVER,
                blend_flags: 0,
                source_constant_alpha: (255.0 * fade) as u8,
                alpha_format: AC_SRC_ALPHA,
            };
            UpdateLayeredWindow(
                hwnd,
                self.screen_dc,
                &self.pos,
                &size,
                self.mem_dc,
                &src_pt,
                0,
                &blend,
                ULW_ALPHA,
            );
        }
    }

    impl Drop for Canvas {
        fn drop(&mut self) {
            unsafe {
                if !self.str_format.is_null() {
                    GdipDeleteStringFormat(self.str_format);
                }
                if !self.title_font.is_null() {
                    GdipDeleteFont(self.title_font);
                }
                if !self.subtitle_font.is_null() {
                    GdipDeleteFont(self.subtitle_font);
                }
                if !self.font_family.is_null() {
                    GdipDeleteFontFamily(self.font_family);
                }
                if !self.graphics.is_null() {
                    GdipDeleteGraphics(self.graphics);
                }
                if !self.bitmap.is_null() {
                    GdipDisposeImage(self.bitmap);
                }
                SelectObject(self.mem_dc, self.prev_obj);
                DeleteObject(self.dib);
                DeleteDC(self.mem_dc);
                ReleaseDC(0, self.screen_dc);
            }
        }
    }

    unsafe fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> *mut c_void {
        let r = r.min(w * 0.5).min(h * 0.5).max(0.5);
        let d = r * 2.0;
        let mut path: *mut c_void = ptr::null_mut();
        if GdipCreatePath(FILL_MODE_ALTERNATE, &mut path) != 0 || path.is_null() {
            return ptr::null_mut();
        }
        GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
        GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
        GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
        GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
        GdipClosePathFigure(path);
        path
    }

    unsafe fn fill_round_rect(
        graphics: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        color: u32,
    ) {
        let path = rounded_rect_path(x, y, w, h, r);
        if path.is_null() {
            return;
        }
        let mut brush: *mut c_void = ptr::null_mut();
        if GdipCreateSolidFill(color, &mut brush) == 0 && !brush.is_null() {
            GdipFillPath(graphics, brush, path);
            GdipDeleteBrush(brush);
        }
        GdipDeletePath(path);
    }

    unsafe fn fill_round_rect_gradient(
        graphics: *mut c_void,
        rect: RectF,
        r: f32,
        c1: u32,
        c2: u32,
    ) {
        let path = rounded_rect_path(rect.x, rect.y, rect.width, rect.height, r);
        if path.is_null() {
            return;
        }
        let p1 = PointF {
            x: rect.x,
            y: rect.y,
        };
        let p2 = PointF {
            x: rect.x + rect.width,
            y: rect.y + rect.height,
        };
        let mut brush: *mut c_void = ptr::null_mut();
        if GdipCreateLineBrush(&p1, &p2, c1, c2, WRAP_MODE_TILE_FLIP_X, &mut brush) == 0
            && !brush.is_null()
        {
            GdipFillPath(graphics, brush, path);
            GdipDeleteBrush(brush);
        }
        GdipDeletePath(path);
    }

    unsafe fn stroke_round_rect(
        graphics: *mut c_void,
        rect: RectF,
        r: f32,
        width: f32,
        color: u32,
    ) {
        let path = rounded_rect_path(rect.x, rect.y, rect.width, rect.height, r);
        if path.is_null() {
            return;
        }
        let mut pen: *mut c_void = ptr::null_mut();
        if GdipCreatePen1(color, width, UNIT_PIXEL, &mut pen) == 0 && !pen.is_null() {
            GdipDrawPath(graphics, pen, path);
            GdipDeletePen(pen);
        }
        GdipDeletePath(path);
    }

    unsafe fn fill_ellipse(graphics: *mut c_void, x: f32, y: f32, w: f32, h: f32, color: u32) {
        let mut brush: *mut c_void = ptr::null_mut();
        if GdipCreateSolidFill(color, &mut brush) == 0 && !brush.is_null() {
            GdipFillEllipse(graphics, brush, x, y, w, h);
            GdipDeleteBrush(brush);
        }
    }

    unsafe fn draw_arc(
        graphics: *mut c_void,
        rect: RectF,
        start_angle: f32,
        sweep_angle: f32,
        width: f32,
        color: u32,
    ) {
        let mut pen: *mut c_void = ptr::null_mut();
        if GdipCreatePen1(color, width, UNIT_PIXEL, &mut pen) == 0 && !pen.is_null() {
            GdipSetPenStartCap(pen, LINE_CAP_ROUND);
            GdipSetPenEndCap(pen, LINE_CAP_ROUND);
            GdipDrawArc(
                graphics,
                pen,
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                start_angle,
                sweep_angle,
            );
            GdipDeletePen(pen);
        }
    }

    unsafe fn draw_text(
        graphics: *mut c_void,
        text: &[u16],
        font: *mut c_void,
        rect: &RectF,
        format: *mut c_void,
        color: u32,
    ) {
        let mut brush: *mut c_void = ptr::null_mut();
        if GdipCreateSolidFill(color, &mut brush) == 0 && !brush.is_null() {
            GdipDrawString(
                graphics,
                text.as_ptr(),
                (text.len().saturating_sub(1)) as i32,
                font,
                rect,
                format,
                brush,
            );
            GdipDeleteBrush(brush);
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_APP_CLOSE => {
                KillTimer(hwnd, TIMER_ID);
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                KillTimer(hwnd, TIMER_ID);
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, w_param, l_param),
        }
    }

    unsafe fn active_work_area() -> RECT {
        let mut cursor = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut cursor) != 0 {
            let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
            if monitor != 0 {
                let mut info = MONITORINFO {
                    cb_size: std::mem::size_of::<MONITORINFO>() as u32,
                    rc_monitor: RECT {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 0,
                    },
                    rc_work: RECT {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 0,
                    },
                    dw_flags: 0,
                };
                if GetMonitorInfoW(monitor, &mut info) != 0 {
                    return info.rc_work;
                }
            }
        }
        RECT {
            left: 0,
            top: 0,
            right: GetSystemMetrics(SM_CXSCREEN).max(800),
            bottom: GetSystemMetrics(SM_CYSCREEN).max(600),
        }
    }

    fn run_splash_thread(generation: u64, is_dark: bool) {
        unsafe {
            let mut token: usize = 0;
            let startup = GdiplusStartupInput {
                gdiplus_version: 1,
                debug_event_callback: ptr::null_mut(),
                suppress_background_thread: 0,
                suppress_external_codecs: 0,
            };
            if GdiplusStartup(&mut token, &startup, ptr::null_mut()) != 0 {
                return;
            }

            let h_instance = GetModuleHandleW(ptr::null());
            let class_name = wide("TokenScopeNativeSplash");
            let registered = *CLASS_REGISTERED.get_or_init(|| {
                let wc = WNDCLASSEXW {
                    cb_size: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: 0,
                    lpfn_wnd_proc: Some(wnd_proc),
                    cb_cls_extra: 0,
                    cb_wnd_extra: 0,
                    h_instance,
                    h_icon: 0,
                    h_cursor: 0,
                    hbr_background: 0,
                    lpsz_menu_name: ptr::null(),
                    lpsz_class_name: class_name.as_ptr(),
                    h_icon_sm: 0,
                };
                RegisterClassExW(&wc)
            });

            if registered == 0
                || !state().active.load(Ordering::SeqCst)
                || state().generation.load(Ordering::SeqCst) != generation
            {
                GdiplusShutdown(token);
                return;
            }

            let title = wide("TokenScope");
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                LOGICAL_TOTAL_W as i32,
                LOGICAL_TOTAL_H as i32,
                0,
                0,
                h_instance,
                ptr::null_mut(),
            );
            if hwnd == 0 {
                GdiplusShutdown(token);
                return;
            }

            let dpi = GetDpiForWindow(hwnd);
            let scale = if dpi >= 96 { dpi as f32 / 96.0 } else { 1.0 };
            let width = (LOGICAL_TOTAL_W * scale).round() as i32;
            let height = (LOGICAL_TOTAL_H * scale).round() as i32;
            let work = active_work_area();
            let pos = POINT {
                x: work.left + ((work.right - work.left) - width) / 2,
                y: work.top + ((work.bottom - work.top) - height) / 2,
            };

            let Some(canvas) = Canvas::new(hwnd, pos, width, height, scale, is_dark) else {
                DestroyWindow(hwnd);
                GdiplusShutdown(token);
                return;
            };

            if !state().active.load(Ordering::SeqCst)
                || state().generation.load(Ordering::SeqCst) != generation
            {
                drop(canvas);
                DestroyWindow(hwnd);
                GdiplusShutdown(token);
                return;
            }

            state().hwnd.store(hwnd, Ordering::SeqCst);
            canvas.render_frame(hwnd);
            SetTimer(hwnd, TIMER_ID, FRAME_MS, 0);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                if !state().active.load(Ordering::SeqCst)
                    || state().generation.load(Ordering::SeqCst) != generation
                {
                    PostMessageW(hwnd, WM_APP_CLOSE, 0, 0);
                }
                if msg.message == WM_TIMER && msg.hwnd == hwnd {
                    canvas.render_frame(hwnd);
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            state()
                .hwnd
                .compare_exchange(hwnd, 0, Ordering::SeqCst, Ordering::SeqCst)
                .ok();
            drop(canvas);
            GdiplusShutdown(token);
        }
    }

    pub(crate) fn show(is_dark: bool) {
        let s = state();
        let _guard = s.lock.lock().ok();
        let previous_hwnd = s.hwnd.swap(0, Ordering::SeqCst);
        if previous_hwnd != 0 {
            unsafe {
                PostMessageW(previous_hwnd, WM_APP_CLOSE, 0, 0);
            }
        }
        let gen = s.generation.fetch_add(1, Ordering::SeqCst) + 1;
        s.active.store(true, Ordering::SeqCst);
        let _ = thread::Builder::new()
            .name("tokenscope-splash".into())
            .spawn(move || run_splash_thread(gen, is_dark));
    }

    pub(crate) fn close() {
        let s = state();
        let _guard = s.lock.lock().ok();
        s.active.store(false, Ordering::SeqCst);
        s.generation.fetch_add(1, Ordering::SeqCst);
        let hwnd = s.hwnd.swap(0, Ordering::SeqCst);
        if hwnd != 0 {
            unsafe {
                PostMessageW(hwnd, WM_APP_CLOSE, 0, 0);
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub(crate) fn show(_is_dark: bool) {}
    pub(crate) fn close() {}
}

pub(crate) fn show(is_dark: bool) {
    platform::show(is_dark);
}

pub(crate) fn close() {
    platform::close();
}
