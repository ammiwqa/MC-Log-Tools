#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! GUI на egui/eframe.
//!
//! Логика разнесена по модулям:
//! - `app.rs` — состояние приложения, команды, обработка событий
//! - `presenter.rs` — реализация Presenter для egui
//! - `theme.rs` — константы оформления, стиль, шрифты
//! - `screens/` — отдельный модуль на каждый экран
//! - `results_window.rs` — окно результатов поиска

mod app;
mod commands;
mod presenter;
mod results_window;
mod screens;
mod theme;

use app::LogtoolsApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([840.0, 560.0])
            .with_title("LogTools"),
        ..Default::default()
    };

    eframe::run_native(
        "LogTools",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::configure_style(&cc.egui_ctx);
            Ok(Box::new(LogtoolsApp::new()))
        }),
    )
}
