// Im Release kein zusätzliches Konsolenfenster unter Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    fotodatenbank_lib::run()
}
