#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let origin = std::env::args().nth(1).unwrap_or_default();
    let result = match mind_place_archive::data_root() {
        Ok(root) => mind_place_capture_host::serve_one(
            std::io::stdin().lock(),
            std::io::stdout().lock(),
            &root,
            &origin,
        ),
        Err(error) => mind_place_capture_host::write_response(
            std::io::stdout().lock(),
            &serde_json::json!({"ok":false,"error":error.to_string()}),
        ),
    };
    if let Err(error) = result {
        eprintln!("Mind Place native host: {error}");
        std::process::exit(1);
    }
}
