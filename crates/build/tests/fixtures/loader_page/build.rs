//! Generates the page registration and the hydrate entry from `pages/`, as an app does.

fn main() -> std::process::ExitCode {
    fundamental_build::PagesBuild::run_build_script(fundamental_build::Hydration::Plain)
}
