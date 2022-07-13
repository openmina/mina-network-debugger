# Mina Network Debugger

(readme for developers)

## Build

See dockerfile.

Most likely developer machine already have all deps.

Need rust `nightly-2022-07-01`. And bpf linker.

```
rustup update nightly-2022-07-01
rustup component add rust-src --toolchain nightly-2022-07-01-x86_64-unknown-linux-gnu
cargo install bpf-linker --git https://github.com/vlad9486/bpf-linker --branch keep-btf
```

## Run

```
cargo run --bin bpf-recorder --release
```

Will ask your superuser password.

In separate terminal run the application with env variable `BPF_ALIAS=` set.
The value of the variable doesn't matter. That is for bpf-recorder
to recognize the target application.

Maybe, we will pass some useful info to the debugger using this env variables.
