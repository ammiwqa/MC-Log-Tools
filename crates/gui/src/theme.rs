use eframe::egui;

// =========================
// THEME
// =========================

pub const BG: egui::Color32 = egui::Color32::from_rgb(0x09, 0x09, 0x0A);
pub const PANEL: egui::Color32 = egui::Color32::from_rgb(0x0E, 0x0E, 0x10);
pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x16, 0x16, 0x19);
pub const SURFACE_HOVER: egui::Color32 = egui::Color32::from_rgb(0x1E, 0x1E, 0x22);
pub const SURFACE_ACTIVE: egui::Color32 = egui::Color32::from_rgb(0x27, 0x27, 0x2C);

pub const BORDER: egui::Color32 = egui::Color32::from_rgb(0x26, 0x26, 0x2B);
pub const BORDER_HOVER: egui::Color32 = egui::Color32::from_rgb(0x3B, 0x3B, 0x42);

pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xEB, 0xEB, 0xE7);
pub const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(0x8B, 0x8B, 0x86);
pub const TEXT_FAINT: egui::Color32 = egui::Color32::from_rgb(0x5E, 0x5E, 0x5A);

pub const GREEN: egui::Color32 = egui::Color32::from_rgb(0x0F, 0xDB, 0x92);
pub const GREEN_DIM: egui::Color32 = egui::Color32::from_rgb(0x0B, 0x93, 0x66);
pub const ON_GREEN: egui::Color32 = egui::Color32::from_rgb(0x05, 0x0D, 0x0A);

pub const LOG_KEYWORD: egui::Color32 = egui::Color32::from_rgb(0x22, 0xAF, 0x77);

pub const RED: egui::Color32 = egui::Color32::from_rgb(0xEC, 0x6A, 0x88);
pub const AMBER: egui::Color32 = egui::Color32::from_rgb(0xE0, 0xAF, 0x5F);

// Find in page
pub const FIND_ROW_CURRENT: egui::Color32 = egui::Color32::from_rgb(0x3A, 0x2A, 0x15);
pub const FIND_MATCH_BG: egui::Color32 = egui::Color32::from_rgb(0xE0, 0xAF, 0x5F);
pub const FIND_MATCH_CURRENT_BG: egui::Color32 = egui::Color32::from_rgb(0xEC, 0x6A, 0x88);
pub const FIND_MATCH_TEXT: egui::Color32 = egui::Color32::from_rgb(0x05, 0x0D, 0x0A);

pub fn stroke_1(color: egui::Color32) -> egui::Stroke {
    egui::Stroke::new(1.0_f32, color)
}

pub fn muted(text: &str) -> egui::RichText {
    egui::RichText::new(text).color(TEXT_MUTED)
}

pub fn faint(text: &str) -> egui::RichText {
    egui::RichText::new(text).color(TEXT_FAINT)
}

pub fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).color(ON_GREEN).strong())
        .fill(GREEN)
        .min_size(egui::vec2(0.0, 30.0))
}

pub fn ghost_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text))
        .fill(SURFACE)
        .stroke(stroke_1(BORDER))
        .min_size(egui::vec2(0.0, 30.0))
}

pub fn danger_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).color(RED))
        .fill(egui::Color32::TRANSPARENT)
        .stroke(stroke_1(BORDER))
        .min_size(egui::vec2(0.0, 24.0))
}

pub fn small_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).size(12.0))
        .fill(SURFACE)
        .stroke(stroke_1(BORDER))
        .min_size(egui::vec2(0.0, 24.0))
}

pub fn screen_title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("//")
                .color(GREEN)
                .heading()
                .strong(),
        );
        ui.heading(egui::RichText::new(title));
    });
    ui.label(faint(subtitle).small());
    ui.add_space(8.0);
}

pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .monospace()
            .small()
            .color(TEXT_FAINT),
    );
}

pub fn status_dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
}

pub fn zed_progress_bar(ui: &mut egui::Ui, fraction: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());

    ui.painter().rect_filled(rect, 3.0, SURFACE_ACTIVE);

    let width = (rect.width() * fraction).min(rect.width());

    if width > 1.0 {
        let bar = egui::Rect::from_min_size(rect.left_top(), egui::vec2(width, rect.height()));
        let radius = 3.0_f32.min(width * 0.5);
        ui.painter().rect_filled(bar, radius, GREEN);
    }
}

pub fn log_line_color(line: &str) -> egui::Color32 {
    //   [ERROR] message...           → red
    //   [WARN]  message...           → amber
    //   ERROR: message...            → red
    //   WARN:  message...            → amber
    //   ERROR message...             → red
    //   WARN  message...             → amber
    //   Severe  message...           → red   (Forge/Fabric severity)
    //   Warning message...           → amber
    //   Fine/Finer/Finest/Info/Debug → muted (по умолчанию)

    let trimmed = line.trim_start();
    let first = match trimmed.find(char::is_whitespace) {
        Some(idx) => &trimmed[..idx],
        None => trimmed,
    };
    let bare = first
        .trim_end_matches(':')
        .trim_end_matches(']')
        .trim_start_matches('[');

    match bare {
        "ERROR" | "Error" | "SEVERE" | "Severe" | "FATAL" | "Fatal" | "CRITICAL" | "Critical" => {
            RED
        }
        "WARN" | "Warn" | "WARNING" | "Warning" => AMBER,
        _ => TEXT_MUTED,
    }
}

pub fn log_line_job(ui: &egui::Ui, line: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();

    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let color = log_line_color(line);
    let is_alert = color == RED || color == AMBER;

    let (first, rest) = match line.find(char::is_whitespace) {
        Some(idx) => (&line[..idx], &line[idx..]),
        None => (line, ""),
    };

    if is_alert || first.is_empty() {
        job.append(line, 0.0, egui::TextFormat::simple(font, color));
    } else {
        job.append(
            first,
            0.0,
            egui::TextFormat::simple(font.clone(), LOG_KEYWORD),
        );
        job.append(rest, 0.0, egui::TextFormat::simple(font, color));
    }

    job
}

pub fn install_fonts(ctx: &egui::Context) {
    let candidates: &[&str] = &[
        // Windows
        r"C:\Windows\Fonts\CascadiaMono.ttf",
        r"C:\Windows\Fonts\cascadiamono.ttf",
        r"C:\Windows\Fonts\consola.ttf",
        // macOS
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/SFMono-Regular.otf",
        "/Library/Fonts/JetBrainsMono-Regular.ttf",
        // Linux
        "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf",
        "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf",
        "/usr/share/fonts/truetype/cascadia/CascadiaCode-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ];

    let mut fonts = egui::FontDefinitions::default();

    for path in candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(
                "zed_mono".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(data)),
            );

            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                family.insert(0, "zed_mono".to_owned());
            }

            break;
        }
    }

    ctx.set_fonts(fonts);
}

pub fn configure_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BG;
    visuals.text_edit_bg_color = Some(BG);
    visuals.faint_bg_color = SURFACE;
    visuals.window_stroke = stroke_1(BORDER);
    visuals.hyperlink_color = GREEN;
    visuals.weak_text_color = Some(TEXT_MUTED);
    visuals.warn_fg_color = AMBER;
    visuals.error_fg_color = RED;

    visuals.selection.bg_fill = GREEN_DIM;
    visuals.selection.stroke = stroke_1(GREEN);

    let w = &mut visuals.widgets;

    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.bg_stroke = stroke_1(BORDER);
    w.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);

    w.inactive.bg_fill = SURFACE;
    w.inactive.bg_stroke = stroke_1(BORDER);
    w.inactive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);

    w.hovered.bg_fill = SURFACE_HOVER;
    w.hovered.bg_stroke = stroke_1(BORDER_HOVER);
    w.hovered.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);

    w.active.bg_fill = SURFACE_ACTIVE;
    w.active.bg_stroke = stroke_1(GREEN_DIM);
    w.active.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);

    w.open.bg_fill = SURFACE;
    w.open.bg_stroke = stroke_1(BORDER);
    w.open.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);

    let mut style = egui::Style::default();
    style.visuals = visuals;
    style.text_styles = [
        (egui::TextStyle::Heading, egui::FontId::monospace(20.0)),
        (egui::TextStyle::Body, egui::FontId::proportional(13.5)),
        (egui::TextStyle::Button, egui::FontId::monospace(13.0)),
        (egui::TextStyle::Small, egui::FontId::proportional(10.5)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(12.5)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);

    ctx.set_style(style);
}

pub fn open_in_system_app(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

pub fn strip_path_quotes(raw: &str) -> &str {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(trimmed);
    let trimmed = trimmed
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(trimmed);
    trimmed.trim()
}
