extern crate cargo_metadata;
extern crate quote;
extern crate syn;

#[macro_use]
extern crate failure;

use std::{env, fs, io::ErrorKind, path::PathBuf};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let mut manifest_dir: PathBuf = env::var("CARGO_MANIFEST_DIR").unwrap().into();

    // Dummy declarations for RLS.
    if std::env::var("CARGO").unwrap_or_default().ends_with("rls") {
        llvm::Generator::default()
            .write_declarations(&format!("{}/llvm_gen.rs", out_dir))
            .expect("Unable to write generated LLVM declarations");

        return;
    }

    println!("cargo:rerun-if-changed=build.rs");

    llvm::Generator::default()
        .parse_llvm_sys_crate()
        .expect("Unable to parse 'llvm-sys' crate")
        .write_declarations(&format!("{}/llvm_gen.rs", out_dir))
        .expect("Unable to write generated LLVM declarations");

    // Workaround for `cargo package`
    // `cargo metadata` creates a new Cargo.lock file, which needs removing
    manifest_dir.push("Cargo.lock");
    if let Err(e) = fs::remove_file(&manifest_dir) {
        if e.kind() != ErrorKind::NotFound {
            panic!("unexpected error clearing local Cargo.lock: {}", e);
        }
    }
}

#[derive(Debug)]
struct Declaration {
    name: String,
    args: String,
    ret_ty: String,
}

mod llvm {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};

    use cargo_metadata::MetadataCommand;
    use failure::Error;
    use quote::ToTokens;
    use syn::{parse_file, Abi, ForeignItem, Item, ItemForeignMod, ReturnType};

    use super::*;

    const LLVM_SOURCES: &[&str] = &[
        "analysis.rs",
        "bit_reader.rs",
        "bit_writer.rs",
        "core.rs",
        "debuginfo.rs",
        "disassembler.rs",
        "error_handling.rs",
        "execution_engine.rs",
        "initialization.rs",
        "ir_reader.rs",
        "linker.rs",
        "lto.rs",
        "object.rs",
        "orc2/ee.rs",
        "orc2/lljit.rs",
        "orc2/mod.rs",
        "support.rs",
        "target.rs",
        "target_machine.rs",
        "transforms/ipo.rs",
        "transforms/pass_builder.rs",
        "transforms/pass_manager_builder.rs",
        "transforms/scalar.rs",
        "transforms/vectorize.rs",
    ];

    const INIT_MACROS: &[&str] = &[
        "LLVM_InitializeAllTargetInfos",
        "LLVM_InitializeAllTargets",
        "LLVM_InitializeAllTargetMCs",
        "LLVM_InitializeAllAsmPrinters",
        "LLVM_InitializeAllAsmParsers",
        "LLVM_InitializeAllDisassemblers",
        "LLVM_InitializeNativeTarget",
        "LLVM_InitializeNativeAsmParser",
        "LLVM_InitializeNativeAsmPrinter",
        "LLVM_InitializeNativeDisassembler",
    ];

    #[derive(Default)]
    pub struct Generator {
        declarations: Vec<Declaration>,
    }

    impl Generator {
        pub fn parse_llvm_sys_crate(mut self) -> Result<Self, Error> {
            let llvm_src_path = self.get_llvm_sys_crate_path()?;

            for file in LLVM_SOURCES {
                let path = llvm_src_path.join(file);
                let mut declarations = self.extract_file_declarations(&path)?;

                self.declarations.append(&mut declarations);
            }

            Ok(self)
        }

        pub fn write_declarations(self, path: &str) -> Result<(), Error> {
            let mut file = File::create(path)?;

            for decl in self.declarations {
                if INIT_MACROS.contains(&decl.name.as_str()) {
                    // Skip target initialization wrappers
                    // (see llvm-sys/wrappers/target.c)
                    continue;
                }
                writeln!(
                    file,
                    "create_proxy!({}; {}; {});",
                    decl.name,
                    decl.ret_ty,
                    decl.args
                        // We cannot use `Err` as an argument name provided to
                        // the macro, it conflicts with the `Err` tuple variant
                        // from Rust std preludes. That `Err` comes from the
                        // `pub type` declaration, where using it allowed.
                        // https://play.rust-lang.org/?gist=ef464634c9ee2193c08f6d97bdba5dd2
                        .replace("Err :", "Error :")
                        .trim_end_matches(',')
                )?;
            }

            Ok(())
        }

        fn get_llvm_sys_crate_path(&self) -> Result<PathBuf, Error> {
            let metadata = MetadataCommand::new()
                .exec()
                .map_err(|_| format_err!("Unable to get crate metadata"))?;

            let llvm_dependency = metadata
                .packages
                .into_iter()
                .find(|item| item.name == "llvm-sys")
                .ok_or_else(|| format_err!("Unable to find 'llvm-sys' in the crate metadata"))?;

            let llvm_lib_rs_path = llvm_dependency
                .targets
                .into_iter()
                .find(|item| item.name == "llvm-sys")
                .ok_or_else(|| format_err!("Unable to find lib target for 'llvm-sys' crate"))?
                .src_path;

            Ok(llvm_lib_rs_path.parent().unwrap().into())
        }

        fn extract_file_declarations(&self, path: &Path) -> Result<Vec<Declaration>, Error> {
            let mut file = File::open(path)
                .map_err(|_| format_err!("Unable to open file: {}", path.to_str().unwrap()))?;

            let mut content = String::new();
            file.read_to_string(&mut content)?;

            let ast = parse_file(&content).map_err(|e| failure::err_msg(e.to_string()))?;

            Ok(ast.items.iter().fold(vec![], |mut list, item| match item {
                Item::ForeignMod(ref item) if item.abi.is_c() => {
                    list.append(&mut self.extract_foreign_mod_declarations(item));
                    list
                }

                _ => list,
            }))
        }

        fn extract_foreign_mod_declarations(&self, item: &ItemForeignMod) -> Vec<Declaration> {
            item.items.iter().fold(vec![], |mut list, item| match item {
                ForeignItem::Fn(ref item) => {
                    let ret_ty = match item.sig.output {
                        ReturnType::Default => "()".to_string(),
                        ReturnType::Type(_, ref ty) => ty.to_token_stream().to_string(),
                    };

                    list.push(Declaration {
                        name: item.sig.ident.to_string(),
                        args: item.sig.inputs.to_token_stream().to_string(),
                        ret_ty,
                    });

                    list
                }

                _ => list,
            })
        }
    }

    trait AbiExt {
        fn is_c(&self) -> bool;
    }

    impl AbiExt for Abi {
        fn is_c(&self) -> bool {
            let abi_name = self
                .name
                .as_ref()
                .map(|item| item.value())
                .unwrap_or_default();

            abi_name == "C"
        }
    }
}
