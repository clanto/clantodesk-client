use librustdesk::*;

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    // Necessario anche qui: senza, APP_NAME e ORG restano ai default upstream e
    // il servizio cerca il socket IPC e la config nei percorsi di RustDesk.
    // Prima di load_custom_client, come in core_main, cosi' il custom client vince.
    let _ = crate::common::global_init();
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--write-plists" {
        if let Err(e) = librustdesk::platform::write_plists() {
            eprintln!("Failed to write plists: {}", e);
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    crate::common::load_custom_client();
    hbb_common::init_log(false, "service");
    crate::start_os_service();
}
