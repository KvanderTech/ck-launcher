fn main() {
    println!("cargo:rerun-if-env-changed=CK_CURSEFORGE_API_KEY");
}
