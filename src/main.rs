// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gitcraft::app;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("gitcraft")
            .with_inner_size([1400.0, 860.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "gitcraft",
        options,
        Box::new(|cc| {
            let open_arg = std::env::args_os().nth(1).map(std::path::PathBuf::from);
            Ok(Box::new(app::App::new(cc, open_arg)))
        }),
    )
}
