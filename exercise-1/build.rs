use std::env;
use std::process::Command;

fn main() {

    println!("cargo:warning=BUILD SCRIPT RUNNING");
    println!("cargo:warning=cwd={:?}", env::current_dir());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/main.rs");

    let cwd = env::current_dir().unwrap().to_string_lossy().to_string();
    let xpdf_dir = format!("{}/xpdf", cwd);

    println!("cargo:warning=cwd={:?}", xpdf_dir);

    // make clean; remove any leftover gunk from prior builds
    Command::new("make")
        .arg("clean")
        .current_dir(xpdf_dir.clone())
        .status()
        .expect("Couldn't clean xpdf directory");

    println!("cargo:warning=cwd=HERE");

    // clean doesn't know about the install directory we use to build, remove it as well
    Command::new("rm")
        .arg("-r")
        .arg("-v")
        .arg("-f")
        .arg(&format!("{}/install", xpdf_dir))
        .current_dir(xpdf_dir.clone())
        .status()
        .expect("Couldn't clean xpdf's install directory");

    println!("cargo:warning=cwd=HERE2");

    // export LLVM_CONFIG=llvm-config-19
    unsafe { env::set_var("LLVM_CONFIG", "llvm-config-19") };

    println!("cargo:warning=cwd=HERE3");

    // configure with afl-clang-fast and set install directory to ./xpdf/install
    Command::new("./configure")
        .arg(&format!("--prefix={}/install", xpdf_dir))
        .env("CC", "/usr/local/bin/afl-clang-fast")
        .env("CXX", "/usr/local/bin/afl-clang-fast++")
        .current_dir(xpdf_dir.clone())
        .status()
        .expect("Couldn't configure xpdf to build using afl-clang-fast");

    println!("cargo:warning=cwd=HERE4");

    println!("cargo:warning=I am here");

    // make && make install
    let status = Command::new("make")
    .current_dir(&xpdf_dir)
    .status()
    .expect("failed to run make");

    assert!(status.success(), "make failed");

    println!("cargo:warning=cwd=HERE4");

    Command::new("make")
        .arg("install")
        .current_dir(xpdf_dir)
        .status()
        .expect("Couldn't install xpdf");

    println!("cargo:warning=cwd=HERE5");
}


