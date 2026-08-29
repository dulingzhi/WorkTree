#[cfg(target_os = "windows")]
fn main() {
    println!("cargo:rerun-if-changed=windows/worktree.rc");
    println!("cargo:rerun-if-changed=../../assets/worktree.ico");
    let _ = embed_resource::compile("windows/worktree.rc", embed_resource::NONE);
}

#[cfg(not(target_os = "windows"))]
fn main() {}
