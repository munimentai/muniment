//! Keeps ADR 0008's Pi pin aligned with the release descriptors.

use std::path::Path;

use muniment_core::sidecar::pi_install::{PI_ARTIFACT, PI_RELEASE_BASE};
use muniment_core::sidecar::{PI_NPM_PACKAGE, PI_VERSION};

const RELEASE_ARTIFACTS: &[(&str, u64, &str)] = &[
    (
        "pi-darwin-arm64.tar.gz",
        30_928_407,
        "c68e3ac4d05b4e282aaab2e6c76f161d3e9e68f19a22e38913cbfaadb6c800f0",
    ),
    (
        "pi-darwin-x64.tar.gz",
        33_440_191,
        "7a042d6413065421387001a4986190a1a03186c95a695f4dee0bdc76e60de8f7",
    ),
    (
        "pi-linux-arm64.tar.gz",
        42_529_658,
        "135580f6b942151646e67b8b866d987d28ce3cff5a497030775ddd29659f943d",
    ),
    (
        "pi-linux-x64.tar.gz",
        42_464_648,
        "c2f3c3e6a1850bd87654cc3ca8811013272397c3d042a4e2a64c43ee1b423972",
    ),
    (
        "pi-windows-x64.zip",
        44_907_374,
        "03b2318774f18721e959d9f8f3340a9f942e7aa516fb7030d3007a12a40a4a97",
    ),
];

#[test]
fn adr_0008_records_the_factory_pi_release() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/decisions/0008-pi-runtime-distribution.md");
    let markdown = std::fs::read_to_string(&path).unwrap();

    assert_eq!(PI_NPM_PACKAGE, "@earendil-works/pi-coding-agent");
    assert_eq!(PI_VERSION, "0.84.4");
    assert!(PI_RELEASE_BASE.ends_with("/v0.84.4"));
    assert!(markdown.contains("Amended: 2026-09-04"));
    assert!(markdown.contains("**`@earendil-works/pi-coding-agent` 0.84.4**"));
    let compact_markdown = markdown.replace(',', "");
    for (archive, byte_size, sha256) in RELEASE_ARTIFACTS {
        assert!(markdown.contains(&format!("`{archive}`")));
        assert!(compact_markdown.contains(&byte_size.to_string()));
        assert!(markdown.contains(&format!("`{sha256}`")));
    }
    assert!(RELEASE_ARTIFACTS.contains(&(
        PI_ARTIFACT.archive,
        PI_ARTIFACT.byte_size,
        PI_ARTIFACT.sha256,
    )));
}
