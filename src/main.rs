mod backend;
mod frontend;

use dioxus::desktop::{Config, WindowBuilder};

fn main() {
    let title = dioxus_cli_config::app_title()
        .unwrap_or_else(|| "Cashew AI Workflow Spreadsheet".to_string());

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_background_color((7, 17, 31, 255))
                .with_window(WindowBuilder::new().with_title(title)),
        )
        .launch(frontend::App);
}
