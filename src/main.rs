#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod protection;

#[cfg(windows)]
mod blocked;
#[cfg(windows)]
mod browser;
#[cfg(windows)]
mod center;
#[cfg(windows)]
mod chrome;
#[cfg(windows)]
mod chrome_draw;
#[cfg(windows)]
mod chrome_menu;
#[cfg(windows)]
mod favicon;
#[cfg(windows)]
mod frame;
#[cfg(windows)]
mod http;
#[cfg(windows)]
mod inspector;
#[cfg(windows)]
mod instance;
#[cfg(windows)]
mod omnibox;
#[cfg(windows)]
mod profile;
#[cfg(windows)]
mod translation;
#[cfg(windows)]
mod updates;

#[cfg(windows)]
fn main() {
    if let Err(error) = browser::run() {
        eprintln!("{error}");
        browser::show_error(&error.to_string());
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("O protótipo Brisa usa WebView2 e roda no Windows.");
}
