fn main() {
    // Debug builds carry element info so tests can find and click UI elements; release
    // builds for the Pi leave it out.
    let debug = std::env::var("PROFILE").is_ok_and(|p| p == "debug");
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-dark".into())
        .with_debug_info(debug);
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint build failed");
}
