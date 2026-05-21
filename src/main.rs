mod backend;
mod frontend;

use dioxus::desktop::{Config, WindowBuilder};

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_window(WindowBuilder::new().with_title("Cashew AI Workflow Spreadsheet")),
        )
        .launch(frontend::App);
}
