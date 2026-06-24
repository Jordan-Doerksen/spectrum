//! Spectrum control panel — thin entrypoint.
//! Release build hides the console (`windows_subsystem`); debug keeps it for engine logs.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    spectrum_pro_lib::run();
}
