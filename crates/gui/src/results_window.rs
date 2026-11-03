use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone};
use eframe::egui;
use logtools_app::read_context_around;

use crate::theme;

pub struct SearchResultsWindowState {
    pub open: bool,
    pub search_base: String,
    pub search_text: String,
    pub search_results: Arc<Vec<(u64, String)>>,
    pub results_path: Option<String>,
    pub nav_target_row: Option<usize>,
    pub context_lines: usize,
    pub context_marks: HashSet<(u64, String)>,

    pub find_open: bool,
    pub find_text: String,
    pub find_matches: Vec<usize>,
    pub find_match_idx: usize,
    pub find_last_text: String,
    pub find_was_open: bool,
}

impl SearchResultsWindowState {
    pub fn new(
        search_base: String,
        search_text: String,
        search_results: Arc<Vec<(u64, String)>>,
        results_path: Option<String>,
    ) -> Self {
        Self {
            open: true,
            search_base,
            search_text,
            search_results,
            results_path,
            nav_target_row: None,
            context_lines: 5,
            context_marks: HashSet::new(),
            find_open: false,
            find_text: String::new(),
            find_matches: Vec::new(),
            find_match_idx: 0,
            find_last_text: String::new(),
            find_was_open: false,
        }
    }
}

pub fn render(ctx: &eframe::egui::Context, app: &mut crate::app::LogtoolsApp) {
    let Some(state) = &app.search_results_window else {
        return;
    };

    let state_arc = state.clone();

    let open = {
        let s = state_arc.lock().unwrap();
        s.open
    };

    if !open {
        return;
    }

    ctx.show_viewport_deferred(
        egui::ViewportId::from_hash_of("search_results_window"),
        egui::ViewportBuilder::default()
            .with_title("Search Results")
            .with_inner_size([900.0, 500.0])
            .with_min_inner_size([600.0, 300.0]),
        move |ctx, _class| {
            if ctx.input(|i| i.viewport().close_requested()) {
                state_arc.lock().unwrap().open = false;
                return;
            }

            let (
                base,
                text,
                results,
                results_path,
                context_marks,
                find_open,
                find_text,
                find_matches,
                find_match_idx,
                find_was_open,
            ) = {
                let mut s = state_arc.lock().unwrap();

                if ctx.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.ctrl) {
                    s.find_open = !s.find_open;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    s.find_open = false;
                }

                let base = s.search_base.clone();
                let text = s.search_text.clone();
                let results = s.search_results.clone();
                let context_marks = s.context_marks.clone();
                let results_path = s.results_path.clone();
                let find_open = s.find_open;
                let find_text = s.find_text.clone();
                let find_matches = s.find_matches.clone();
                let find_match_idx = s.find_match_idx;
                let find_was_open = s.find_was_open;

                (
                    base,
                    text,
                    results,
                    results_path,
                    context_marks,
                    find_open,
                    find_text,
                    find_matches,
                    find_match_idx,
                    find_was_open,
                )
            };

            egui::TopBottomPanel::bottom("results_bottom_panel").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                let mut open_results_file_requested = false;

                ui.horizontal(|ui| {
                    if let Some(path) = &results_path {
                        ui.label(theme::muted("Results file:").monospace());
                        ui.label(egui::RichText::new(path.as_str()).monospace());
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(theme::ghost_button("Close")).clicked() {
                            state_arc.lock().unwrap().open = false;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        if ui.add(theme::ghost_button("Find")).clicked() {
                            state_arc.lock().unwrap().find_open = true;
                        }

                        if ui.add(theme::primary_button("Open results file")).clicked() {
                            open_results_file_requested = true;
                        }
                    });
                });

                ui.add_space(4.0);

                {
                    let mut s = state_arc.lock().unwrap();
                    ui.label(theme::muted("Контекст (строк):").small());
                    ui.add(
                        egui::DragValue::new(&mut s.context_lines)
                            .range(1..=1000)
                            .speed(1),
                    );
                }

                ui.add_space(4.0);

                if open_results_file_requested {
                    if let Some(path) = &results_path {
                        let path_buf = PathBuf::from(path);
                        crate::theme::open_in_system_app(&path_buf);
                    }
                }
            });

            let tree = build_date_tree(&results);

            egui::SidePanel::right("date_tree_panel")
                .width_range(180.0..=220.0)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("DATES")
                            .monospace()
                            .small()
                            .color(theme::TEXT_FAINT),
                    );
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    let mut new_target: Option<usize> = None;

                    egui::ScrollArea::vertical()
                        .id_salt("date_tree_scroll")
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            for (year, months) in &tree {
                                egui::CollapsingHeader::new(
                                    egui::RichText::new(year.to_string())
                                        .monospace()
                                        .strong()
                                        .color(theme::TEXT),
                                )
                                .default_open(false)
                                .show(ui, |ui| {
                                    for (month, days) in months {
                                        egui::CollapsingHeader::new(
                                            egui::RichText::new(format!(
                                                "{}  ({})",
                                                month_name(*month),
                                                days.len()
                                            ))
                                            .monospace()
                                            .color(theme::TEXT_MUTED),
                                        )
                                        .default_open(false)
                                        .show(ui, |ui| {
                                            for (day, idx) in days {
                                                let resp = ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new(format!("{:2}", day))
                                                            .monospace()
                                                            .color(theme::TEXT_MUTED),
                                                    )
                                                    .fill(egui::Color32::TRANSPARENT)
                                                    .stroke(egui::Stroke::NONE)
                                                    .min_size(egui::vec2(
                                                        ui.available_width(),
                                                        20.0,
                                                    )),
                                                );

                                                if resp.clicked() {
                                                    new_target = Some(*idx);
                                                }
                                            }
                                        });
                                    }
                                });
                            }

                            if let Some(idx) = new_target {
                                let mut s = state_arc.lock().unwrap();
                                s.nav_target_row = Some(idx);
                            }
                        });
                });

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Search Results");
                ui.add_space(4.0);

                if !base.is_empty() || !text.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("Base: {}   |   Query: {}", base, text))
                            .small()
                            .color(theme::TEXT_MUTED),
                    );
                    ui.add_space(4.0);
                }

                let total = results.len();
                ui.label(
                    egui::RichText::new(format!("{} result(s)", total))
                        .monospace()
                        .strong(),
                );

                let mut find_buf = find_text.clone();
                let mut find_nav: Option<isize> = None;
                let mut find_close = false;
                let just_opened = find_open && !find_was_open;

                if find_open {
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Find in results:")
                                .small()
                                .color(theme::TEXT_MUTED),
                        );
                        ui.add_space(4.0);

                        let find_resp = ui.add(
                            egui::TextEdit::singleline(&mut find_buf)
                                .desired_width(260.0)
                                .hint_text("text to find within results"),
                        );

                        if just_opened {
                            find_resp.request_focus();
                        }

                        let prev = ui.add(theme::small_button("▲"));
                        let next = ui.add(theme::small_button("▼"));

                        if prev.clicked() {
                            find_nav = Some(-1);
                        }
                        if next.clicked() {
                            find_nav = Some(1);
                        }

                        let matches_len = find_matches.len();
                        let idx = find_match_idx.min(matches_len.saturating_sub(1));
                        if matches_len == 0 {
                            ui.label(egui::RichText::new("0 matches").small().color(theme::RED));
                        } else {
                            ui.label(
                                egui::RichText::new(format!("{} of {}", idx + 1, matches_len))
                                    .small()
                                    .color(theme::TEXT_MUTED),
                            );
                        }

                        if ui.add(theme::small_button("×")).clicked() {
                            find_close = true;
                        }

                        if find_resp.has_focus() {
                            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                                find_nav = Some(-1);
                            }
                            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                                find_nav = Some(1);
                            }
                        }
                    });

                    ui.add_space(4.0);
                }

                let target_row = {
                    let mut s = state_arc.lock().unwrap();

                    if find_buf != s.find_text {
                        s.find_text = find_buf;
                        s.find_last_text = s.find_text.clone();
                        s.find_matches.clear();
                        s.find_match_idx = 0;
                        if !s.find_text.is_empty() {
                            let needle = s.find_text.to_lowercase();
                            for (i, (_, line)) in results.iter().enumerate() {
                                if line.to_lowercase().contains(&needle) {
                                    s.find_matches.push(i);
                                }
                            }
                            if !s.find_matches.is_empty() {
                                s.find_match_idx = 0;
                                s.nav_target_row = Some(s.find_matches[0]);
                            }
                        }
                    }

                    if let Some(dir) = find_nav {
                        let len = s.find_matches.len() as isize;
                        if len > 0 {
                            let new_idx = (s.find_match_idx as isize + dir).rem_euclid(len);
                            s.find_match_idx = new_idx as usize;
                            s.nav_target_row = Some(s.find_matches[s.find_match_idx]);
                        }
                    }

                    if find_close {
                        s.find_open = false;
                    }

                    s.find_was_open = s.find_open;
                    s.nav_target_row.take()
                };

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;

                if let Some(target) = target_row {
                    let target_y = target as f32 * row_height;
                    let scroll_state_id = ui.id().with(egui::Id::new("search_results_window"));
                    let mut sa_state: egui::scroll_area::State = ctx
                        .data(|d| d.get_temp(scroll_state_id))
                        .unwrap_or_default();
                    sa_state.offset.y = target_y;
                    ctx.data_mut(|d| d.insert_temp(scroll_state_id, sa_state));
                }

                ui.spacing_mut().item_spacing.y = 0.0;

                egui::ScrollArea::vertical()
                    .id_salt("search_results_window")
                    .auto_shrink(false)
                    .show_rows(ui, row_height, total, |ui, range| {
                        let last_day_id = ui.id().with("last_shown_day");
                        let mut last_day: Option<String> =
                            ctx.data(|d| d.get_temp::<String>(last_day_id));

                        for i in range {
                            let (unix, line) = &results[i];
                            let date = format_unix_time_gui(*unix);

                            let cur_day = date[..10].to_string();
                            let is_new_day = last_day.as_deref() != Some(&cur_day);

                            if is_new_day && i > 0 {
                                let (spacer_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 10.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(spacer_rect, 0.0, theme::PANEL);
                            }

                            let (row_rect, row_response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), row_height),
                                egui::Sense::click(),
                            );

                            row_response.context_menu(|ui| {
                                let mut s = state_arc.lock().unwrap();

                                ui.label(
                                    egui::RichText::new(format_unix_time_gui(*unix))
                                        .small()
                                        .color(theme::TEXT_MUTED),
                                );
                                ui.separator();

                                ui.label(
                                    egui::RichText::new("Строк контекста:")
                                        .small()
                                        .color(theme::TEXT_MUTED),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut s.context_lines)
                                        .range(1..=1000)
                                        .speed(1),
                                );
                                ui.separator();

                                if ui.button("Показать контекст").clicked() {
                                    let n = s.context_lines;
                                    if let Ok(ctx_lines) =
                                        read_context_around(&s.search_base, *unix as i64, n, n)
                                    {
                                        let mut new_results: Vec<(u64, String)> =
                                            (*s.search_results).clone();
                                        let mut existing: std::collections::HashSet<(u64, String)> =
                                            new_results.iter().cloned().collect();
                                        for (ts, line) in ctx_lines {
                                            let u = ts as u64;
                                            let line_cloned = line.clone();
                                            if existing.insert((u, line_cloned.clone())) {
                                                new_results.push((u, line));
                                                s.context_marks.insert((u, line_cloned));
                                            }
                                        }
                                        new_results.sort_unstable_by_key(|x| x.0);
                                        s.search_results = Arc::new(new_results);
                                    }
                                }
                            });

                            let row_is_current = find_match_idx < find_matches.len()
                                && find_matches[find_match_idx] == i;
                            let row_is_match =
                                !find_text.is_empty() && find_matches.binary_search(&i).is_ok();

                            let is_context = context_marks.contains(&(*unix, line.clone()));
                            let bg = if is_context {
                                theme::SURFACE_ACTIVE
                            } else if row_is_current {
                                theme::FIND_ROW_CURRENT
                            } else if i % 2 == 0 {
                                theme::SURFACE
                            } else {
                                egui::Color32::from_rgb(0x11, 0x11, 0x14)
                            };
                            ui.painter().rect_filled(row_rect, 0.0, bg);

                            if is_new_day {
                                let sep_y = row_rect.top();
                                ui.painter().line_segment(
                                    [
                                        egui::pos2(row_rect.left(), sep_y),
                                        egui::pos2(row_rect.right(), sep_y),
                                    ],
                                    egui::Stroke::new(1.0_f32, theme::TEXT_FAINT),
                                );

                                let label = cur_day.clone();
                                let label_galley = ui.painter().layout_no_wrap(
                                    label,
                                    egui::FontId::monospace(10.0),
                                    theme::TEXT_MUTED,
                                );
                                let label_w = label_galley.size().x + 6.0;
                                let label_h = label_galley.size().y;
                                let label_rect = egui::Rect::from_min_size(
                                    egui::pos2(row_rect.right() - label_w - 4.0, sep_y + 2.0),
                                    egui::vec2(label_w, label_h),
                                );
                                ui.painter().rect_filled(label_rect, 2.0, theme::PANEL);
                                ui.painter().galley(
                                    label_rect.left_top() + egui::vec2(3.0, 0.0),
                                    label_galley,
                                    theme::TEXT_MUTED,
                                );
                            }

                            let date_galley = ui.painter().layout_no_wrap(
                                date,
                                egui::FontId::monospace(12.5),
                                theme::GREEN,
                            );
                            let text_color = theme::log_line_color(line);
                            let font = egui::FontId::monospace(12.5);
                            let line_y = row_rect.center().y
                                - ui.text_style_height(&egui::TextStyle::Monospace) * 0.5;

                            let date_x = row_rect.left() + 8.0;
                            let line_x = date_x + 200.0;
                            let date_y = row_rect.center().y - date_galley.size().y * 0.5;

                            ui.painter().galley(
                                egui::pos2(date_x, date_y),
                                date_galley,
                                theme::GREEN,
                            );

                            if row_is_match {
                                if let Some((start, end)) = find_ci_span(line, &find_text) {
                                    let before = &line[..start];
                                    let matched = &line[start..end];
                                    let after = &line[end..];

                                    let before_galley = ui.painter().layout_no_wrap(
                                        before.to_string(),
                                        font.clone(),
                                        text_color,
                                    );
                                    let before_w = before_galley.size().x;
                                    let matched_galley = ui.painter().layout_no_wrap(
                                        matched.to_string(),
                                        font.clone(),
                                        theme::FIND_MATCH_TEXT,
                                    );
                                    let matched_w = matched_galley.size().x;
                                    let after_galley = ui.painter().layout_no_wrap(
                                        after.to_string(),
                                        font,
                                        text_color,
                                    );

                                    let mut x = line_x;
                                    ui.painter().galley(
                                        egui::pos2(x, line_y),
                                        before_galley,
                                        text_color,
                                    );
                                    x += before_w;

                                    let m_rect = egui::Rect::from_min_size(
                                        egui::pos2(x, row_rect.top()),
                                        egui::vec2(matched_w, row_rect.height()),
                                    );
                                    let m_bg = if row_is_current {
                                        theme::FIND_MATCH_CURRENT_BG
                                    } else {
                                        theme::FIND_MATCH_BG
                                    };
                                    ui.painter().rect_filled(m_rect, 2.0, m_bg);
                                    ui.painter().galley(
                                        egui::pos2(x, line_y),
                                        matched_galley,
                                        theme::FIND_MATCH_TEXT,
                                    );
                                    x += matched_w;

                                    ui.painter().galley(
                                        egui::pos2(x, line_y),
                                        after_galley,
                                        text_color,
                                    );
                                } else {
                                    let line_galley =
                                        ui.painter().layout_no_wrap(line.clone(), font, text_color);
                                    ui.painter().galley(
                                        egui::pos2(line_x, line_y),
                                        line_galley,
                                        text_color,
                                    );
                                }
                            } else {
                                let line_galley =
                                    ui.painter().layout_no_wrap(line.clone(), font, text_color);
                                ui.painter().galley(
                                    egui::pos2(line_x, line_y),
                                    line_galley,
                                    text_color,
                                );
                            }
                            last_day = Some(cur_day.clone());
                        }

                        ctx.data_mut(|d| d.insert_temp(last_day_id, last_day));
                    });
            });
        },
    );
}

pub fn find_ci_span(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    let needle_lc: String = needle.to_lowercase();
    if needle_lc.is_empty() {
        return None;
    }
    let chars: Vec<(usize, char)> = haystack.char_indices().collect();
    let mut si = 0;
    while si < chars.len() {
        let mut acc = String::new();
        let mut ei = si;
        while ei < chars.len() && acc.len() < needle_lc.len() {
            acc.extend(chars[ei].1.to_lowercase());
            ei += 1;
        }
        if acc == needle_lc {
            let start = chars[si].0;
            let end = chars[ei - 1].0 + chars[ei - 1].1.len_utf8();
            return Some((start, end));
        }
        si += 1;
    }
    None
}

pub fn parse_search_result_line(line: &str) -> Option<(u64, String)> {
    if line.len() <= 13 {
        return None;
    }
    let unix = line[0..12].parse::<u64>().ok()?;
    let text = line[13..].to_string();
    Some((unix, text))
}

pub fn parse_date_input(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| dt.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp());
    }

    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return dt
            .and_local_timezone(Local)
            .single()
            .map(|dt| dt.timestamp());
    }

    s.parse::<i64>().ok()
}

pub fn format_unix_time_gui(unix: u64) -> String {
    let dt: DateTime<Local> = Local
        .timestamp_opt(unix as i64, 0)
        .single()
        .unwrap_or_default();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn build_date_tree(results: &[(u64, String)]) -> Vec<(i32, Vec<(u32, Vec<(u32, usize)>)>)> {
    let mut years: BTreeMap<i32, BTreeMap<u32, BTreeMap<u32, usize>>> = BTreeMap::new();

    for (i, (unix, _)) in results.iter().enumerate() {
        let Some(dt) = Local.timestamp_opt(*unix as i64, 0).single() else {
            continue;
        };
        let (y, m, d) = (dt.year(), dt.month(), dt.day());

        years
            .entry(y)
            .or_default()
            .entry(m)
            .or_default()
            .entry(d)
            .or_insert(i);
    }

    years
        .into_iter()
        .map(|(y, months)| {
            (
                y,
                months
                    .into_iter()
                    .map(|(m, days)| {
                        (
                            m,
                            days.into_iter()
                                .map(|(d, idx)| (d, idx))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

pub fn month_name(m: u32) -> &'static str {
    match m {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    }
}
