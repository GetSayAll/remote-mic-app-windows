fn main() {
    let apps =
        sayall_windows::registered_apps::scan_registered_apps().expect("registered app scan");
    assert!(!apps.is_empty(), "registered app list was empty");
    println!("registered_count={}", apps.len());
    if let Some(name) = std::env::args().nth(1) {
        let app = apps
            .iter()
            .find(|app| app.name.eq_ignore_ascii_case(&name))
            .expect("requested test application was not registered");
        sayall_windows::app_launcher::activate_or_launch(&app.path).expect("registered launch");
        println!("launch_submitted=true");
    }
}
