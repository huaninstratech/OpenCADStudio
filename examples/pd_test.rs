//! Manual check for the shell progress dialog: `cargo run --example pd_test`
//! shows "Test dialog" with a bar stepping 1..=5 over ten seconds.
#![allow(non_snake_case)]

fn main() {
    let progress = OpenCADStudio::sys::ConvertProgress::start("Test dialog", 5)
        .expect("progress dialog must create");
    for i in 1..=5u32 {
        progress.file(&format!("pumping file {i} of 5"), i, 5);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < end {
            OpenCADStudio::sys::pump_waiting_messages();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    progress.finish();
    println!("done");
}
