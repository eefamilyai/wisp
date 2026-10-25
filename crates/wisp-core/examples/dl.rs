fn main() {
    let id = std::env::args().nth(1).unwrap();
    let mut last = 0;
    // A downloader that prints progress in quarters, for trying a voice pack by hand.
    let r = wisp_core::voices::download(&id, |d, t| {
        let pct = (d * 100).checked_div(t).unwrap_or(0);
        if pct >= last + 25 {
            last = pct;
            println!("{pct}%");
        }
    });
    println!("{r:?} installed={}", wisp_core::voices::is_installed(&id));
}
