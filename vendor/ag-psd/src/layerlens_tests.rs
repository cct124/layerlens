//! Regression tests for LayerLens's narrowly scoped compatibility patch.

use crate::psd::{
    ChannelId, ColorMode, Compression, DecodeContext, DiagnosticKind, Layer, LayerRawData,
    LayerRawDataChannel, Psd, ReadOptions,
};
use crate::reader::{PsdReader, ReadError, read_section, read_uint32, read_unicode_string};

fn strict_options() -> ReadOptions {
    ReadOptions {
        strict: Some(true),
        throw_for_missing_features: Some(true),
        use_raw_data: Some(true),
        allow_unsupported_features: true,
        ..Default::default()
    }
}

fn document(resources: &[u8], layers: &[u8]) -> Vec<u8> {
    let mut data = b"8BPS\0\x01\0\0\0\0\0\0\0\x03".to_vec();
    data.extend_from_slice(&1_u32.to_be_bytes());
    data.extend_from_slice(&1_u32.to_be_bytes());
    data.extend_from_slice(b"\0\x08\0\x03\0\0\0\0");
    for section in [resources, layers] {
        data.extend_from_slice(&(section.len() as u32).to_be_bytes());
        data.extend_from_slice(section);
    }
    data.extend_from_slice(&[0, 0, 10, 20, 30]);
    data
}

fn resource(id: u16, payload: &[u8]) -> Vec<u8> {
    let mut data = b"8BIM".to_vec();
    data.extend_from_slice(&id.to_be_bytes());
    data.extend_from_slice(&[0, 0]);
    data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    data.extend_from_slice(payload);
    if payload.len() % 2 != 0 {
        data.push(0);
    }
    data
}

#[test]
fn unknown_resource_reports_its_source_offset_without_disabling_strictness() {
    let data = document(&resource(1028, &[5, 6]), &[]);
    let psd = crate::read_psd(&data, &strict_options()).unwrap();
    assert_eq!(psd.diagnostics.len(), 1);
    assert_eq!(psd.diagnostics[0].tag, "1028");
    assert_eq!(psd.diagnostics[0].offset, 34);
    assert_eq!(psd.diagnostics[0].kind, DiagnosticKind::Unknown);
    assert_eq!(
        crate::get_composite_image_data(&psd).unwrap().unwrap().data,
        [10, 20, 30, 255]
    );
}

#[test]
fn malformed_known_resource_is_never_a_recoverable_feature() {
    let data = document(&resource(1005, &[1]), &[]);
    let mut options = strict_options();
    options.throw_for_missing_features = Some(false);
    assert!(crate::read_psd(&data, &options).is_err());
}

#[test]
fn unsupported_gradient_is_typed_and_cannot_swallow_a_truncated_descriptor() {
    let mut payload = 16_u32.to_be_bytes().to_vec();
    payload.extend_from_slice(&0_u32.to_be_bytes()); // descriptor name
    payload.extend_from_slice(&0_u32.to_be_bytes()); // four-byte class id
    payload.extend_from_slice(b"null");
    payload.extend_from_slice(&1_u32.to_be_bytes());
    payload.extend_from_slice(&0_u32.to_be_bytes());
    payload.extend_from_slice(b"Gradbool\x01");
    let build = |payload: &[u8]| {
        let mut layers = vec![0; 8]; // empty layer records and global mask
        layers.extend_from_slice(b"8BIMGdFl");
        layers.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        layers.extend_from_slice(payload);
        if payload.len() % 2 != 0 {
            layers.push(0);
        }
        document(&[], &layers)
    };
    let data = build(&payload);
    let psd = crate::read_psd(&data, &strict_options()).unwrap();
    assert_eq!(psd.diagnostics[0].tag, "GdFl");
    assert_eq!(psd.diagnostics[0].kind, DiagnosticKind::Unsupported);
    let mut options = strict_options();
    options.allow_unsupported_features = false;
    assert!(matches!(
        crate::read_psd(&data, &options),
        Err(ReadError::UnsupportedFeature { .. })
    ));
    payload.pop();
    assert!(crate::read_psd(&build(&payload), &strict_options()).is_err());
}

#[test]
fn section_primitive_cannot_borrow_bytes_from_next_block() {
    let data = [0, 0, 0, 2, 1, 2, 3, 4];
    let mut reader = PsdReader::new(&data, None, None);
    assert!(read_section(&mut reader, 1, |r, _| read_uint32(r), false, false).is_err());
    assert_eq!(reader.buffer.len(), data.len());
}

#[test]
fn oversized_unicode_and_negative_descriptor_count_fail_before_allocation() {
    let data = u32::MAX.to_be_bytes();
    let mut reader = PsdReader::new(&data, None, None);
    assert!(read_unicode_string(&mut reader).is_err());
    let mut reader = PsdReader::new(&data, None, None);
    assert!(crate::descriptor::read_os_type(&mut reader, "VlLs").is_err());
}

#[test]
fn descriptor_and_engine_data_nesting_are_bounded() {
    let mut data = Vec::new();
    for _ in 0..65 {
        data.extend_from_slice(&1_u32.to_be_bytes());
        data.extend_from_slice(b"VlLs");
    }
    data.extend_from_slice(&0_u32.to_be_bytes());
    let mut reader = PsdReader::new(&data, None, None);
    assert!(crate::descriptor::read_os_type(&mut reader, "VlLs").is_err());
    assert_eq!(reader.descriptor_depth, 0);
    let nested = "[ ".repeat(130) + &"] ".repeat(130);
    assert!(crate::engine_data::parse_engine_data(nested.as_bytes()).is_err());
}

#[test]
fn deferred_composite_retains_alpha_strictness_and_memory_budget() {
    let mut psd = Psd {
        width: 1.0,
        height: 1.0,
        channels: Some(4.0),
        bits_per_channel: Some(8.0),
        color_mode: Some(ColorMode::Rgb),
        raw_composite_data: Some(vec![0, 0, 100, 110, 120, 128]),
        raw_composite_context: Some(DecodeContext {
            global_alpha: true,
            strict: true,
            total_memory_limit: Some(64),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        crate::get_composite_image_data(&psd).unwrap().unwrap().data[3],
        128
    );
    psd.raw_composite_context
        .as_mut()
        .unwrap()
        .total_memory_limit = Some(1);
    assert!(matches!(
        crate::get_composite_image_data(&psd),
        Err(ReadError::ExceededMemoryLimit { .. })
    ));
    psd.raw_composite_context
        .as_mut()
        .unwrap()
        .total_memory_limit = Some(64);
    psd.raw_composite_data.as_mut().unwrap().pop();
    assert!(crate::get_composite_image_data(&psd).is_err());
}

#[test]
fn deferred_layer_retains_strictness_and_memory_budget() {
    let mut layer = Layer {
        top: Some(0.0),
        left: Some(0.0),
        bottom: Some(1.0),
        right: Some(2.0),
        raw_data: Some(LayerRawData {
            color_mode: ColorMode::Rgb,
            bits_per_channel: 8.0,
            large: false,
            channels: vec![LayerRawDataChannel {
                id: ChannelId::Color0,
                compression: Compression::RawData,
                data: Some(vec![1]),
            }],
            decode_context: DecodeContext {
                strict: true,
                total_memory_limit: Some(64),
                ..Default::default()
            },
        }),
        ..Default::default()
    };
    assert!(crate::get_layer_image_data(&layer).is_err());
    layer
        .raw_data
        .as_mut()
        .unwrap()
        .decode_context
        .total_memory_limit = Some(1);
    assert!(matches!(
        crate::get_layer_image_data(&layer),
        Err(ReadError::ExceededMemoryLimit { .. })
    ));
}

#[test]
fn deferred_layer_counts_rgba_table_and_row_against_one_budget() {
    let mut layer = Layer {
        top: Some(0.0),
        left: Some(0.0),
        bottom: Some(1.0),
        right: Some(2.0),
        raw_data: Some(LayerRawData {
            color_mode: ColorMode::Rgb,
            bits_per_channel: 8.0,
            large: false,
            channels: vec![LayerRawDataChannel {
                id: ChannelId::Color0,
                compression: Compression::RleCompressed,
                data: Some(vec![0, 3, 1, 10, 20]),
            }],
            decode_context: DecodeContext {
                strict: true,
                total_memory_limit: Some(12),
                ..Default::default()
            },
        }),
        ..Default::default()
    };
    // 8 bytes of RGBA + 4 bytes of row table + 2 decoded bytes are live together.
    assert!(matches!(
        crate::get_layer_image_data(&layer),
        Err(ReadError::ExceededMemoryLimit { .. })
    ));
    layer
        .raw_data
        .as_mut()
        .unwrap()
        .decode_context
        .total_memory_limit = Some(14);
    assert_eq!(
        crate::get_layer_image_data(&layer).unwrap().unwrap().data,
        [10, 0, 0, 255, 20, 0, 0, 255]
    );
    for malformed in [vec![0, 2, 254, 7], vec![0, 1, 255], vec![0, 2, 0, 1]] {
        layer.raw_data.as_mut().unwrap().channels[0].data = Some(malformed);
        assert!(crate::get_layer_image_data(&layer).is_err());
    }
}

#[test]
fn unsupported_timeline_requires_a_structurally_valid_descriptor() {
    let mut descriptor = 16_u32.to_be_bytes().to_vec();
    descriptor.extend_from_slice(&[0; 8]);
    descriptor.extend_from_slice(b"null");
    descriptor.extend_from_slice(&0_u32.to_be_bytes());
    let psd = crate::read_psd(
        &document(&resource(1075, &descriptor), &[]),
        &strict_options(),
    )
    .unwrap();
    assert_eq!(psd.diagnostics[0].kind, DiagnosticKind::Unsupported);
    descriptor.pop();
    assert!(
        crate::read_psd(
            &document(&resource(1075, &descriptor), &[]),
            &strict_options()
        )
        .is_err()
    );
}
