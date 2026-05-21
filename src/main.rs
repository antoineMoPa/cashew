mod backend;
mod frontend;

fn main() {
    dioxus::launch(frontend::App);
}
