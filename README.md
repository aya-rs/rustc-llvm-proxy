# Aya Rustc LLVM Proxy

[![Build](https://github.com/aya-rs/rustc-llvm-proxy/actions/workflows/ci.yml/badge.svg)](https://github.com/aya-rs/rustc-llvm-proxy/actions/workflows/ci.yml)
[![Current Version](https://img.shields.io/crates/v/aya-rustc-llvm-proxy.svg)](https://crates.io/crates/aya-rustc-llvm-proxy)
[![Docs](https://docs.rs/aya-rustc-llvm-proxy/badge.svg)](https://docs.rs/aya-rustc-llvm-proxy)


This is a **fork** of the [rustc-llvm-proxy](https://github.com/denzp/rustc-llvm-proxy) crate.

Dynamically proxy LLVM calls into Rust own shared library! 🎉

## Use cases

Normally there is no much need for the crate, except a couple of exotic cases:

* Your crate is some kind build process helper that leverages LLVM (e.g. [bpf-linker](https://github.com/aya-rs/bpf-linker)),
* Your crate needs to stay up to date with Rust LLVM version (again [bpf-linker](https://github.com/aya-rs/bpf-linker)),
* You would prefer not to have dependencies on host LLVM libs (as always [bpf-linker](https://github.com/aya-rs/bpf-linker)).

## Usage

First, you need to make sure no other crate links your binary against system LLVM library.
In case you are using `llvm-sys`, this can be achieved with a special feature:

``` toml
[dependencies.llvm-sys]
version = "60"
features = ["no-llvm-linking", "disable-alltargets-init"]
```

Then all you need to do is to include the crate into your project:

``` toml
[dependencies]
aya-rustc-llvm-proxy = "0.7"
```

``` rust
extern crate aya_rustc_llvm_proxy;
```
