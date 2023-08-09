#![deny(warnings)]
#![allow(
    non_snake_case,
    unused_imports,
    unused_macros,
    deprecated,
    clippy::missing_safety_doc
)]

//! This is a **fork** of the [rustc-llvm-proxy](https://github.com/denzp/rustc-llvm-proxy) crate.
//!
//! Dynamically proxy LLVM calls into Rust own shared library! 🎉
//!
//! ## Use cases
//! Normally there is no much need for the crate, except a couple of exotic cases:
//!
//! * Your crate is some kind build process helper that leverages LLVM (e.g. [bpf-linker](https://github.com/aya-rs/bpf-linker)),
//! * Your crate needs to stay up to date with Rust LLVM version (again [bpf-linker](https://github.com/aya-rs/bpf-linker)),
//! * You would prefer not to have dependencies on host LLVM libs (as always [bpf-linker](https://github.com/aya-rs/bpf-linker)).
//!
//! ## Usage
//! First, you need to make sure no other crate links your binary against system LLVM library.
//! In case you are using `llvm-sys`, this can be achieved with a special feature:
//!
//! ``` toml
//! [dependencies.llvm-sys]
//! version = "70"
//! features = ["no-llvm-linking"]
//! ```
//!
//! Then all you need to do is to include the crate into your project:
//!
//! ``` toml
//! [dependencies]
//! rustc-llvm-proxy = "0.4"
//! ```
//!
//! ``` rust
//! extern crate aya_rustc_llvm_proxy;
//! ```

use libloading::Library;

mod path;
use path::find_lib_path;

pub mod init;

lazy_static::lazy_static! {
    static ref SHARED_LIB: Library = {
        let lib_path = match find_lib_path() {
            Ok(path) => path,

            Err(error) => {
                eprintln!("{}", error);
                panic!();
            }
        };

        unsafe {
            match Library::new(lib_path) {
                Ok(path) => path,

                Err(error) => {
                    eprintln!("Unable to open LLVM shared lib: {}", error);
                    panic!();
                }
            }
        }
    };
}

/// LLVM C-API symbols with dynamic resolving.
pub mod proxy {
    use super::SHARED_LIB;

    include!(concat!(env!("OUT_DIR"), "/llvm_gen.rs"));
}
