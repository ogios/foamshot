mod config;
mod foamshot;
mod helper;
mod mode;
mod protocols;
mod wayland_ctx;
fn main() {
    env_logger::init();
    foamshot::run_main_loop();
    // foam_shot::run_main_loop();
}
