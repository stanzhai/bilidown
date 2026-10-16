//! Shared fixtures: protobuf segment bodies as hex (generated from the
//! dm.proto schema; see tests/danmaku.rs for the field values).

pub fn hex_seg1() -> Vec<u8> {
    hex_decode("0a2d08e90710b4601801201928ffffff0732036162633a0ce699aee9809ae5bcb9e5b9954080e2cfaa0662033939390a2f08ea071090bf0518042012288080fc0732036465663a0ce5ba95e983a8e5bcb9e5b99540e4e2cfaa066204313030300a0c08eb071801201940016201780a2308ec071088273a15e9bb98e8aea4e580bce5ad97e6aeb5e79c81e795a5620431303032")
}

pub fn hex_seg2() -> Vec<u8> {
    hex_decode("0a2108d10f1080b51818052880fe033a0ce9a1b6e983a8e5bcb9e5b9956204323030310a1e08d20f10d0bb1b180128fe95a1033a087363726f6c6c2062620432303032")
}


fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex digit"))
        .collect()
}
