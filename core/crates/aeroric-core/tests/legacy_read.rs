//! Core query contracts remain distinct from native read/preview policies.

use aeroric_core::files::{
    legacy_read::{self, ImagePreviewData},
    preview::{preview_decision, PreviewDecision},
    read_write,
};
use serde_json::json;
use std::fs;

#[path = "common/scratch.rs"]
mod scratch;

#[test]
fn image_wire_and_native_preview_catalog_remain_distinct() {
    assert_eq!(
        serde_json::to_value(ImagePreviewData {
            data_url: "data:image/png;base64,".into(),
            mime_type: "image/png".into(),
            byte_length: 0,
        })
        .unwrap(),
        json!({"dataUrl": "data:image/png;base64,", "mimeType": "image/png", "byteLength": 0})
    );
    let scratch = scratch::Scratch::new("legacy-read-catalog");
    let svg = scratch.path().join("image.svg");
    fs::write(&svg, b"<svg/>").unwrap();
    let before = fs::metadata(&svg).unwrap().modified().unwrap();
    assert_eq!(
        legacy_read::read_image_preview(&svg).unwrap().mime_type,
        "image/svg+xml"
    );
    assert_eq!(
        preview_decision("image.svg", 6, true),
        PreviewDecision::Text
    );
    for (name, mime) in [("image.ico", "image/x-icon"), ("image.tiff", "image/tiff")] {
        assert_eq!(
            legacy_read::read_image_preview(&scratch.path().join(name))
                .err()
                .as_deref(),
            Some("Unsupported image format")
        );
        assert_eq!(
            preview_decision(name, 0, true),
            PreviewDecision::Image {
                media_type: mime.into()
            }
        );
    }
    assert_eq!(fs::read(&svg).unwrap(), b"<svg/>");
    assert_eq!(fs::metadata(&svg).unwrap().modified().unwrap(), before);
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 1);
}

#[test]
fn text_io_is_exact_and_not_native_error_wrapping() {
    let scratch = scratch::Scratch::new("legacy-read-text");
    let text = scratch.path().join("text");
    let invalid = scratch.path().join("invalid");
    let expected = "\u{feff}first\r\n末尾\0";
    fs::write(&text, expected).unwrap();
    fs::write(&invalid, [0xff, 0xfe]).unwrap();
    let before = fs::metadata(&text).unwrap().modified().unwrap();
    assert_eq!(legacy_read::read_file_content(&text).unwrap(), expected);
    let encoding_error = fs::read_to_string(&invalid).unwrap_err().to_string();
    assert_eq!(
        legacy_read::read_file_content(&invalid),
        Err(encoding_error.clone())
    );
    assert_eq!(
        read_write::read_file_content(scratch.path(), "invalid"),
        Err(format!("cannot read file: {encoding_error}"))
    );
    let absent = scratch.path().join("absent");
    let open_error = fs::File::open(&absent).unwrap_err().to_string();
    assert_eq!(legacy_read::read_file_content(&absent), Err(open_error));
    assert_eq!(fs::read_to_string(&text).unwrap(), expected);
    assert_eq!(fs::read(&invalid).unwrap(), [0xff, 0xfe]);
    assert_eq!(fs::metadata(&text).unwrap().modified().unwrap(), before);
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 2);
}

#[test]
fn metadata_limits_keep_original_errors_without_mutating_files() {
    let scratch = scratch::Scratch::new("legacy-read-limits");
    let text = scratch.path().join("large.txt");
    let image = scratch.path().join("large.png");
    fs::File::create(&text)
        .unwrap()
        .set_len(2 * 1024 * 1024 + 1)
        .unwrap();
    fs::File::create(&image)
        .unwrap()
        .set_len(10 * 1024 * 1024 + 1)
        .unwrap();
    let before_text = fs::metadata(&text).unwrap();
    let before_image = fs::metadata(&image).unwrap();
    assert_eq!(
        legacy_read::read_file_content(&text).err().as_deref(),
        Some("File too large (2.0 MB)")
    );
    assert_eq!(
        legacy_read::read_image_preview(&image).err().as_deref(),
        Some("Image too large (10.0 MB)")
    );
    assert_eq!(legacy_read::MAX_IMAGE_PREVIEW_BYTES, 10 * 1024 * 1024);
    for (path, before) in [(&text, before_text), (&image, before_image)] {
        let after = fs::metadata(path).unwrap();
        assert_eq!(after.len(), before.len());
        assert_eq!(after.modified().unwrap(), before.modified().unwrap());
        assert_eq!(after.permissions(), before.permissions());
    }
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 2);
}
