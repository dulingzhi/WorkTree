#[cfg(target_os = "windows")]
fn main() {
    println!("cargo:rerun-if-changed=windows/repositorytree.rc");
    println!("cargo:rerun-if-changed=../../assets/repositorytree.ico");
    let _ = embed_resource::compile("windows/repositorytree.rc", embed_resource::NONE);
}

#[cfg(not(target_os = "windows"))]
fn main() {}
