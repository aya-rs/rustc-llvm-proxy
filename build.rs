extern crate cargo_metadata;
extern crate quote;
extern crate syn;

fn main() {
    let out_dir = std::env::var_os("OUT_DIR").unwrap();

    llvm::Generator::default()
        .parse_llvm_sys_crate()
        .expect("Unable to parse 'llvm-sys' crate")
        .write_declarations(&std::path::PathBuf::from(out_dir).join("llvm_gen.rs"))
        .expect("Unable to write generated LLVM declarations");

    // Workaround for `cargo package`
    // `cargo metadata` creates a new Cargo.lock file, which needs removing
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let cargo_lock = std::path::PathBuf::from(manifest_dir).join("Cargo.lock");
    if let Err(err) = std::fs::remove_file(&cargo_lock) {
        if err.kind() != std::io::ErrorKind::NotFound {
            panic!("failed to delete {}: {err}", cargo_lock.display());
        }
    }
}

mod llvm {
    use anyhow::{format_err, Context as _, Error};
    use cargo_metadata::{MetadataCommand, Package, Target};
    use quote::{format_ident, quote};
    use std::{
        collections::{
            hash_map::{Entry, HashMap},
            HashSet,
        },
        fs, io, iter,
        path::Path,
    };

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
        functions: HashMap<syn::Ident, (Vec<syn::Ident>, syn::ItemFn)>,
    }

    fn llvm_sys() -> syn::Ident {
        format_ident!("llvm_sys")
    }

    impl Generator {
        pub fn parse_llvm_sys_crate(&mut self) -> Result<&mut Self, Error> {
            let metadata = MetadataCommand::new()
                .exec()
                .context("Unable to get crate metadata")?;

            let llvm_sys_src_path = metadata
                .packages
                .into_iter()
                .find_map(|Package { name, targets, .. }| {
                    (name == "llvm-sys")
                        .then(|| {
                            targets
                                .into_iter()
                                .find_map(|Target { name, src_path, .. }| {
                                    (name == "llvm-sys").then_some(src_path)
                                })
                        })
                        .flatten()
                })
                .ok_or_else(|| format_err!("Unable to find 'llvm-sys' in the crate metadata"))?;

            self.generate_file(llvm_sys_src_path.as_std_path(), &[llvm_sys()])?;

            Ok(self)
        }

        pub fn generate_mod(
            &mut self,
            fs_path: &Path,
            mod_path: &[syn::Ident],
            m: syn::ItemMod,
        ) -> Result<(), Error> {
            let syn::ItemMod { ident, content, .. } = m;
            let directory = fs_path.join(ident.to_string());
            let mod_path: Vec<_> = mod_path.iter().chain(iter::once(&ident)).cloned().collect();
            match content {
                None => {
                    // The module is in another file (or directory).
                    let fs_path = if directory
                        .try_exists()
                        .with_context(|| directory.display().to_string())?
                    {
                        directory.join("mod.rs")
                    } else {
                        directory.with_extension("rs")
                    };
                    self.generate_file(&fs_path, mod_path.as_slice())
                        .with_context(|| fs_path.display().to_string())
                        .map_err(Into::into)
                }
                Some((_, items)) => {
                    // The module is inline.
                    for item in items {
                        match item {
                            syn::Item::Mod(m) => {
                                self.generate_mod(&directory, mod_path.as_slice(), m)
                                    .with_context(|| quote! { #(#mod_path)::* }.to_string())?;
                            }
                            syn::Item::Type(..) => {}
                            item => {
                                panic!("unexpected item {}", quote! { #item });
                            }
                        }
                    }
                    Ok(())
                }
            }
        }

        pub fn generate_file(
            &mut self,
            fs_path: &Path,
            mod_path: &[syn::Ident],
        ) -> Result<(), Error> {
            let content =
                fs::read_to_string(fs_path).with_context(|| fs_path.display().to_string())?;
            let syn::File {
                shebang: _,
                attrs: _,
                items,
            } = syn::parse_file(&content).context(content)?;
            for item in items {
                match item {
                    syn::Item::Mod(m) => {
                        let fs_path = fs_path.parent().unwrap();
                        self.generate_mod(fs_path, mod_path, m)?;
                    }
                    syn::Item::ForeignMod(syn::ItemForeignMod {
                        attrs: _,
                        unsafety: mod_unsafety,
                        abi: mod_abi,
                        items,
                        brace_token: _,
                    }) => {
                        for item in items {
                            match item {
                                syn::ForeignItem::Fn(syn::ForeignItemFn {
                                    attrs: _,
                                    mut sig,
                                    vis,
                                    semi_token: _,
                                }) => {
                                    let syn::Signature {
                                        constness: _,
                                        asyncness: _,
                                        unsafety,
                                        abi,
                                        fn_token,
                                        ident,
                                        generics: _,
                                        paren_token,
                                        inputs,
                                        variadic,
                                        output,
                                    } = &mut sig;
                                    if unsafety.is_none() {
                                        *unsafety = mod_unsafety;
                                    }
                                    if abi.is_none() {
                                        *abi = Some(mod_abi.clone());
                                    }
                                    if INIT_MACROS.iter().any(|macro_name| ident == macro_name) {
                                        // Skip target initialization wrappers
                                        // (see llvm-sys/wrappers/target.c)
                                        continue;
                                    }
                                    let mut bare_inputs = syn::punctuated::Punctuated::new();
                                    let mut input_names = Vec::new();
                                    for input in inputs.iter_mut() {
                                        match input {
                                            syn::FnArg::Receiver(receiver) => {
                                                panic!(
                                                    "unexpected receiver {}",
                                                    quote! { #receiver }
                                                );
                                            }
                                            syn::FnArg::Typed(syn::PatType {
                                                attrs,
                                                ref mut pat,
                                                colon_token: _,
                                                ty,
                                            }) => {
                                                bare_inputs.push(syn::BareFnArg {
                                                    attrs: attrs.clone(),
                                                    name: None,
                                                    ty: (**ty).clone(),
                                                });

                                                match &mut **pat {
                                                    syn::Pat::Ident(syn::PatIdent {
                                                        ref mut ident,
                                                        ..
                                                    }) => {
                                                        // error[E0530]: function parameters cannot shadow tuple variants
                                                        //      |
                                                        // 9923 |     pub fn LLVMGetErrorTypeId(Err: LLVMErrorRef) -> LLVMErrorTypeId {
                                                        //      |                               ^^^ cannot be named the same as a tuple variant
                                                        if ident == "Err" {
                                                            *ident = format_ident!("Error");
                                                        }
                                                        input_names.push(ident.clone());
                                                    }
                                                    pat => {
                                                        panic!(
                                                            "unexpected pat {}",
                                                            quote! { #pat }
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    let variadic = variadic.as_ref().map(
                                        |syn::Variadic {
                                             attrs,
                                             pat: _,
                                             dots,
                                             comma,
                                         }| {
                                            syn::BareVariadic {
                                                attrs: attrs.clone(),
                                                name: None,
                                                dots: *dots,
                                                comma: *comma,
                                            }
                                        },
                                    );

                                    let type_bare_fn = syn::TypeBareFn {
                                        lifetimes: Default::default(),
                                        unsafety: *unsafety,
                                        abi: abi.clone(),
                                        fn_token: *fn_token,
                                        paren_token: *paren_token,
                                        inputs: bare_inputs,
                                        variadic,
                                        output: output.clone(),
                                    };

                                    let block = quote! {
                                        {
                                            let entry = unsafe {
                                                crate::proxy::SHARED_LIB.get::<#type_bare_fn>(
                                                    stringify!(#ident).as_bytes(),
                                                )
                                            }.expect(stringify!(#ident));
                                            entry(#(#input_names),*)
                                        }
                                    };
                                    let block = syn::parse2(block).unwrap();

                                    let ident = ident.clone();
                                    let item_fn = syn::ItemFn {
                                        attrs: Vec::new(),
                                        vis,
                                        sig,
                                        block,
                                    };
                                    let item_fn = syn::parse2(quote! {
                                        #[no_mangle]
                                        #item_fn
                                    })
                                    .unwrap();

                                    let Self { functions } = self;
                                    match functions.entry(ident) {
                                        Entry::Occupied(entry) => {
                                            if entry.key() == "LLVMAddInstructionCombiningPass" {
                                                // TODO(https://reviews.llvm.org/D155402): Remove this when the declaration isn't duplicated.
                                                continue;
                                            }
                                            let ident = entry.key();
                                            let (other_mod_path, _) = entry.get();
                                            let mod_path = quote! { #(#mod_path::)*#ident };
                                            let other_mod_path =
                                                quote! { #(#other_mod_path::)*#ident };
                                            panic!(
                                                "duplicate function `{}` `{}`",
                                                mod_path, other_mod_path
                                            );
                                        }
                                        Entry::Vacant(entry) => {
                                            entry.insert((mod_path.into(), item_fn));
                                        }
                                    }
                                }
                                item => {
                                    panic!("unexpected item {}", quote! { #item });
                                }
                            }
                        }
                    }
                    syn::Item::Const(..)
                    | syn::Item::Enum(..)
                    | syn::Item::ExternCrate(..)
                    | syn::Item::Macro(..)
                    | syn::Item::Struct(..)
                    | syn::Item::Type(..)
                    | syn::Item::Use(..) => {}
                    item => {
                        panic!("unexpected item {}", quote! { #item });
                    }
                }
            }
            Ok(())
        }

        pub fn write_declarations(&self, path: &Path) -> io::Result<()> {
            let Self { functions } = self;
            let mut items = Vec::new();
            let mut paths = HashSet::new();
            let root = [llvm_sys()];
            let prelude = [llvm_sys(), format_ident!("prelude")];
            paths.insert(root.as_slice());
            paths.insert(prelude.as_slice());
            for (path, item_fn) in functions.values() {
                let item_fn = syn::parse2(quote! {
                    #item_fn
                })
                .unwrap();
                items.push(item_fn);
                paths.insert(path);
            }
            let items = paths
                .into_iter()
                .map(|path| {
                    syn::parse2(quote! {
                        use #(#path::)**;
                    })
                    .unwrap()
                })
                .chain(items)
                .collect();
            let file = syn::File {
                shebang: None,
                attrs: Vec::new(),
                items,
            };
            let formatted = prettyplease::unparse(&file);
            fs::write(path, formatted)
        }
    }
}
