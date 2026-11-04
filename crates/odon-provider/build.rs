// `option_env!` is baked in at compile time. Without these directives cargo
// reuses an artifact compiled before the credentials existed, silently shipping
// a binary whose Antigravity login can never work. Declaring the dependency makes
// a change to either variable force a rebuild.
fn main() {
    for name in [
        "TERMIGO_ANTIGRAVITY_CLIENT_ID",
        "TERMIGO_ANTIGRAVITY_CLIENT_SECRET",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
}
