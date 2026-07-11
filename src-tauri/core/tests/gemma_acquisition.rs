use std::collections::VecDeque;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use muniment_core::llama::acquisition::{
    acquire_gemma_stage, GemmaAcquisitionError, GemmaAcquisitionLimits, GemmaDownloadRequest,
    GemmaDownloadResponse, GemmaDownloadTransport, GemmaTransportError,
};
use muniment_core::llama::lifecycle::{GemmaNoticeDescriptor, GemmaRevisionDescriptor};
use muniment_core::llama::ResidentModelDescriptor;

static MODEL: ResidentModelDescriptor = ResidentModelDescriptor {
    filename: "model.gguf",
    byte_size: 3,
    sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    alias: "fixture",
    context_tokens: 1,
};
static REVISION: GemmaRevisionDescriptor = GemmaRevisionDescriptor {
    identity: "fixture-v1",
    revision: "0123456789abcdef",
    model: &MODEL,
    notice: GemmaNoticeDescriptor {
        filename: "NOTICE.txt",
        contents: b"fixture notice",
    },
};

enum Reply {
    Response(u16, Option<(u64, u64, u64)>, &'static [u8]),
    Error(GemmaTransportError),
}

struct Transport {
    replies: VecDeque<Reply>,
    offsets: Vec<u64>,
}

impl Transport {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            replies: replies.into_iter().collect(),
            offsets: Vec::new(),
        }
    }
}

impl GemmaDownloadTransport for Transport {
    type Body = Cursor<Vec<u8>>;

    fn download(
        &mut self,
        request: &GemmaDownloadRequest,
    ) -> Result<GemmaDownloadResponse<Self::Body>, GemmaTransportError> {
        assert_eq!(request.url(), "https://huggingface.co/google/gemma-3-4b-it-qat-q4_0-gguf/resolve/0123456789abcdef/model.gguf");
        assert!(!request.limits.deadline.is_zero());
        self.offsets.push(request.offset);
        match self.replies.pop_front().unwrap() {
            Reply::Response(status, range, bytes) => Ok(GemmaDownloadResponse {
                status,
                content_range: range,
                body: Cursor::new(bytes.to_vec()),
            }),
            Reply::Error(error) => Err(error),
        }
    }
}

fn root() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "muniment-gemma-acquisition-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    root
}

#[test]
fn resumes_a_part_and_returns_only_a_verified_publication_stage() {
    let root = root();
    fs::create_dir(root.join("install")).unwrap();
    fs::write(root.join("install/model.gguf.part"), b"a").unwrap();
    let mut transport = Transport::new([Reply::Response(206, Some((1, 2, 3)), b"bc")]);

    let stage = acquire_gemma_stage(
        &root,
        "install",
        &REVISION,
        GemmaAcquisitionLimits::default(),
        &mut transport,
        &|| false,
    )
    .unwrap();

    assert_eq!(transport.offsets, [1]);
    assert_eq!(fs::read(stage.join("model.gguf")).unwrap(), b"abc");
    assert_eq!(
        fs::read(stage.join("NOTICE.txt")).unwrap(),
        b"fixture notice"
    );
    assert!(!stage.join("model.gguf.part").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_full_response_to_a_range_request_restarts_instead_of_appending() {
    let root = root();
    fs::create_dir(root.join("install")).unwrap();
    fs::write(root.join("install/model.gguf.part"), b"a").unwrap();
    let mut transport = Transport::new([Reply::Response(200, None, b"abc")]);
    acquire_gemma_stage(
        &root,
        "install",
        &REVISION,
        GemmaAcquisitionLimits::default(),
        &mut transport,
        &|| false,
    )
    .unwrap();
    assert_eq!(fs::read(root.join("install/model.gguf")).unwrap(), b"abc");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn transient_failure_retries_from_the_preserved_part() {
    let root = root();
    let mut transport = Transport::new([
        Reply::Error(GemmaTransportError::Transient),
        Reply::Response(200, None, b"abc"),
    ]);
    acquire_gemma_stage(
        &root,
        "install",
        &REVISION,
        GemmaAcquisitionLimits::default(),
        &mut transport,
        &|| false,
    )
    .unwrap();
    assert_eq!(transport.offsets, [0, 0]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_inconsistent_range_discards_the_part_before_retrying() {
    let root = root();
    fs::create_dir(root.join("install")).unwrap();
    fs::write(root.join("install/model.gguf.part"), b"a").unwrap();
    let mut transport = Transport::new([
        Reply::Response(206, Some((0, 2, 3)), b"abc"),
        Reply::Response(200, None, b"abc"),
    ]);
    acquire_gemma_stage(
        &root,
        "install",
        &REVISION,
        GemmaAcquisitionLimits::default(),
        &mut transport,
        &|| false,
    )
    .unwrap();
    assert_eq!(transport.offsets, [1, 0]);
    assert_eq!(fs::read(root.join("install/model.gguf")).unwrap(), b"abc");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bad_or_oversized_bytes_are_never_exposed_as_a_complete_stage() {
    for bytes in [&b"abd"[..], &b"abcd"[..]] {
        let root = root();
        let mut transport = Transport::new([Reply::Response(200, None, bytes)]);
        assert!(matches!(
            acquire_gemma_stage(
                &root,
                "install",
                &REVISION,
                GemmaAcquisitionLimits::default(),
                &mut transport,
                &|| false
            ),
            Err(GemmaAcquisitionError::Verification(_)) | Err(GemmaAcquisitionError::TooLarge)
        ));
        assert!(!root.join("install/model.gguf").exists());
        assert!(!root.join("install/model.gguf.part").exists());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn cancellation_is_retryable_without_destroying_resume_bytes() {
    let root = root();
    fs::create_dir(root.join("install")).unwrap();
    fs::write(root.join("install/model.gguf.part"), b"a").unwrap();
    let mut transport = Transport::new([]);
    assert_eq!(
        acquire_gemma_stage(
            &root,
            "install",
            &REVISION,
            GemmaAcquisitionLimits::default(),
            &mut transport,
            &|| true
        ),
        Err(GemmaAcquisitionError::Cancelled)
    );
    assert_eq!(
        fs::read(root.join("install/model.gguf.part")).unwrap(),
        b"a"
    );
    fs::remove_dir_all(root).unwrap();
}
