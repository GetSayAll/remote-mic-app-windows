//! Uses the production wheel sender after a short foreground positioning delay.
use sayall_windows::send_input::ScrollDirection;
use sayall_windows::send_input_windows::SendInputRuntime;

fn main() {
    let direction = match std::env::args().nth(1).as_deref() {
        Some("up") => ScrollDirection::Up,
        Some("down") => ScrollDirection::Down,
        _ => panic!("usage: wheel_probe <up|down> [delay-ms]"),
    };
    let delay = std::env::args()
        .nth(2)
        .map(|arg| arg.parse::<u64>().expect("delay-ms"))
        .unwrap_or(3000);
    std::thread::sleep(std::time::Duration::from_millis(delay));
    let snapshot = SendInputRuntime::new()
        .scroll(direction, 1)
        .expect("wheel submission");
    println!(
        "direction={direction:?} submitted_events={}",
        snapshot.submitted_events
    );
}
