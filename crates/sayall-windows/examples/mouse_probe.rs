//! Exercises the production mouse sender against a dedicated test window.
use sayall_windows::send_input::{MouseClickKind, MoveDirection, ScrollDirection};
use sayall_windows::send_input_windows::SendInputRuntime;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let runtime = SendInputRuntime::new();
    let result = match args.get(1).map(String::as_str) {
        Some("click") => runtime.mouse_click(match args[2].as_str() {
            "left" => MouseClickKind::Left, "right" => MouseClickKind::Right,
            "middle" => MouseClickKind::Middle, "double_left" => MouseClickKind::DoubleLeft, _ => panic!("click kind"),
        }),
        Some("move") => runtime.mouse_move(match args[2].as_str() {
            "up" => MoveDirection::Up, "down" => MoveDirection::Down, "left" => MoveDirection::Left, "right" => MoveDirection::Right, _ => panic!("move direction"),
        }, args[3].parse().expect("distance")),
        Some("scroll") => runtime.scroll(match args[2].as_str() {
            "up" => ScrollDirection::Up, "down" => ScrollDirection::Down, _ => panic!("scroll direction"),
        }, args[3].parse().expect("steps")),
        _ => panic!("usage: mouse_probe click <kind> | move <direction> <pixels> | scroll <direction> <steps>"),
    };
    println!(
        "submitted_events={}",
        result.expect("mouse action").submitted_events
    );
}
