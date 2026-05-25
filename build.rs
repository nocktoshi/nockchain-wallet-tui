fn main() {
    // Load .env so that compile-time env!() macros (e.g. KERNEL_JAM_PATH) see the values
    dotenvy::dotenv().ok();

    // Propagate KERNEL_JAM_PATH (and any other needed vars) to rustc invocations
    // for *all* crates in this build, including ones fetched from git.
    if let Ok(val) = std::env::var("KERNEL_JAM_PATH") {
        println!("cargo:env=KERNEL_JAM_PATH={}", val);
    }

    // Re-run if .env changes
    println!("cargo:rerun-if-changed=.env");
    println!("cargo:rerun-if-changed=.env.example");
}
