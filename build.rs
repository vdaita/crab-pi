use std::process::Command;
use std::fs;
use std::path::Path;

fn main() {
    let kernels_dir = "src/gpu_kernels";
    println!("cargo:rerun-if-changed={}", kernels_dir);

    let entries = fs::read_dir(kernels_dir)
        .expect("Failed to read gpu_kernels directory");

    for entry in entries {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("qasm") {
            let kernel_src = path.to_string_lossy().to_string();
            let kernel_out = path.with_extension("bin").to_string_lossy().to_string();

            let output = Command::new("vc4asm")
                .args(&["-o", &kernel_out, &kernel_src])
                .output();

            match output {
                Ok(out) if out.status.success() => {
                    println!("cargo:warning=Compiled {}", kernel_src);
                }
                Ok(out) => {
                    let err = String::from_utf8_lossy(&out.stderr);
                    panic!("vc4asm error compiling {}:\n{}", kernel_src, err);
                }
                Err(_) => {
                    panic!("vc4asm not found!");
                }
            }
        }
    }
}
