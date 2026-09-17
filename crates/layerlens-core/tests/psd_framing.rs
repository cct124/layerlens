//! 独立编码微型 PSD，验证真实稿暴露的图层区段边界差异；不收录下载资源。

use std::io::Cursor;

use layerlens_core::psd::{ParseLimits, PsdDocument, PsdErrorCode};

fn section(bytes: &[u8]) -> Vec<u8> {
    let mut result = (bytes.len() as u32).to_be_bytes().to_vec();
    result.extend_from_slice(bytes);
    result
}

// 单层 RGB8 RAW；width 改变通道长度，从而覆盖图层信息长度的全部模四余数。
fn sample(width: u32, padding: &[u8], global: &[u8]) -> Vec<u8> {
    let mut info = 1_i16.to_be_bytes().to_vec();
    for value in [0_u32, 0, 1, width] {
        info.extend_from_slice(&value.to_be_bytes());
    }
    info.extend_from_slice(&3_u16.to_be_bytes());
    for id in 0_i16..3 {
        info.extend_from_slice(&id.to_be_bytes());
        info.extend_from_slice(&(width + 2).to_be_bytes());
    }
    info.extend_from_slice(b"8BIMnorm\xff\0\0\0");
    // 空 mask、空 blending ranges、按四字节补齐的 Pascal 名称 A。
    info.extend(section(b"\0\0\0\0\0\0\0\0\x01A\0\0"));
    for value in [255, 0, 0] {
        info.extend_from_slice(&0_u16.to_be_bytes());
        info.extend(std::iter::repeat_n(value, width as usize));
    }
    info.extend_from_slice(padding);
    let mut layer_mask = section(&info);
    layer_mask.extend_from_slice(global);
    let mut bytes = b"8BPS\0\x01\0\0\0\0\0\0\0\x03".to_vec();
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(b"\0\x08\0\x03");
    bytes.extend(section(&[])); // Color Mode Data
    bytes.extend(section(&[])); // Image Resources
    bytes.extend(section(&layer_mask));
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    for value in [255, 0, 0] {
        bytes.extend(std::iter::repeat_n(value, width as usize));
    }
    bytes
}

fn assert_red_preview(bytes: &[u8], width: u32, with_tag: bool) {
    let document = PsdDocument::from_bytes(bytes, ParseLimits::default()).unwrap();
    assert_eq!(document.info().layers.len(), 1);
    assert_eq!(document.info().layers[0].name, "A");
    assert_eq!(
        document.info().resources.global_additional_blocks.len(),
        usize::from(with_tag)
    );
    if with_tag {
        assert_eq!(
            document.info().resources.global_additional_blocks[0],
            "zz01"
        );
    }
    let mut reader = png::Decoder::new(Cursor::new(document.preview_png().unwrap()))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((frame.width, frame.height), (width, 1));
    assert_eq!(pixels, [255, 0, 0, 255].repeat(width as usize));
}

fn assert_invalid(bytes: &[u8]) {
    let error = PsdDocument::from_bytes(bytes, ParseLimits::default())
        .err()
        .expect("非法区段应拒绝");
    assert_eq!(error.code, PsdErrorCode::InvalidInput);
}

#[test]
fn layer_info_accepts_only_exact_two_or_four_byte_zero_padding() {
    let global = b"\0\0\0\08BIMzz01\0\0\0\0";
    for width in 1..=4 {
        let consumed = 72 + 3 * width as usize;
        let two = (2 - consumed % 2) % 2;
        let four = (4 - consumed % 4) % 4;
        for length in 0..=7 {
            let mut padding = vec![0; length];
            let bytes = sample(width, &padding, global);
            if length == two || length == four {
                assert_red_preview(&bytes, width, true);
                if let Some(last) = padding.last_mut() {
                    *last = 1;
                    assert_invalid(&sample(width, &padding, global));
                }
            } else {
                assert_invalid(&bytes);
            }
        }
    }
}

#[test]
fn global_mask_length_may_be_absent_only_at_the_parent_boundary() {
    assert_red_preview(&sample(4, &[], &[]), 4, false);
    assert_red_preview(&sample(4, &[], &[0; 4]), 4, false);
    for length in 1..=3 {
        assert_invalid(&sample(4, &[], &vec![0; length]));
    }
    assert_invalid(&sample(4, &[], &[0, 0, 0, 1]));
    assert_invalid(&sample(4, &[], &[0, 0, 0, 1, 0]));
}
