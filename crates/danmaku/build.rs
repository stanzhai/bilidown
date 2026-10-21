fn main() {
    // CI runners and fresh Windows dev boxes have no system protoc; use the
    // vendored binary so the build never depends on the host environment.
    std::env::set_var(
        "PROTOC",
        protoc_bin_vendored::protoc_bin_path().expect("vendored protoc"),
    );
    prost_build::Config::new()
        .compile_protos(&["proto/dm.proto"], &["proto/"])
        .expect("compile dm.proto");
}
