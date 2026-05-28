pub fn copy(text: &str) {
    if let Ok(mut clip) = arboard::Clipboard::new() {
        let _ = clip.set_text(text.to_string());
    }
}
