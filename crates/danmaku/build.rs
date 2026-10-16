fn main() {
    prost_build::Config::new()
        .compile_protos(&["proto/dm.proto"], &["proto/"])
        .expect("compile dm.proto");
}
