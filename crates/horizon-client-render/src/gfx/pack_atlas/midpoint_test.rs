//! Original GPU probes of the production terrain inputs, without a resource or shader pack.

use super::PackAtlas;
use crate::mesher::{TerrainVertex, UV_STEPS};
use wgpu::util::DeviceExt;

#[test]
fn mid_tex_coord_follows_each_quads_mapped_corners() {
    let _gpu = crate::gfx::test_gpu::lock();
    let instance = crate::gfx::test_gpu::instance();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    eprintln!("midpoint probe adapter: {:?}", adapter.get_info());
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 5,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let atlas = PackAtlas::new(&device, &mut encoder, &[texture], 5, 5, 16).unwrap();
    queue.submit([encoder.finish()]);
    // Each row uses a different atlas cell. Expected atlas coordinates are independent
    // numbers, including the existing clamp of a model UV beyond the sprite's edge.
    let cases = [
        (
            "whole",
            [[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
            [1. / 6., 0.25],
            [1. / 6., 0.25],
            [[0., 0.], [1. / 3., 0.], [1. / 3., 0.5], [0., 0.5]],
        ),
        (
            "half-height",
            [[0., 0.], [1., 0.], [1., 0.5], [0., 0.5]],
            [0.5, 0.125],
            [0.5, 0.25],
            [
                [1. / 3., 0.],
                [2. / 3., 0.],
                [2. / 3., 0.25],
                [1. / 3., 0.25],
            ],
        ),
        (
            "rotated partial",
            [[0.25, 0.75], [0.25, 0.25], [0.5, 0.25], [0.5, 0.75]],
            [19. / 24., 0.25],
            [5. / 6., 0.25],
            [
                [0.75, 0.375],
                [0.75, 0.125],
                [5. / 6., 0.125],
                [5. / 6., 0.375],
            ],
        ),
        (
            "clamped",
            [[0., 0.], [1.5, 0.], [1.5, 0.5], [0., 0.5]],
            [1. / 6., 0.625],
            [1. / 6., 0.75],
            [[0., 0.5], [1. / 3., 0.5], [1. / 3., 0.75], [0., 0.75]],
        ),
        (
            "fourth corner",
            [[0., 0.], [1., 0.], [1., 0.5], [0., 1.]],
            [0.5, 0.6875],
            [0.5, 0.75],
            [
                [1. / 3., 0.5],
                [2. / 3., 0.5],
                [2. / 3., 0.75],
                [1. / 3., 1.],
            ],
        ),
    ];
    let positions = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
    let vertices: Vec<_> = cases
        .iter()
        .enumerate()
        .flat_map(|(layer, (_, uv, ..))| {
            (0..4).map(move |k| {
                TerrainVertex::new(positions[k], uv[k], layer as u16 | 0x8000, u32::MAX, 0)
            })
        })
        .collect();
    let mut failures = Vec::new();
    // Positive origins encode fade bits beside the power-of-two scale. Negative origins
    // use the older fractional scale encoding. Both must match the mapped vertex UVs.
    for origin in [
        f32::from_bits((1. / UV_STEPS).to_bits() | 0x12345),
        -3. - 1. / UV_STEPS,
    ] {
        for flat in [true, false] {
            for shadow in [false, true] {
                let pixels = sample(&device, &queue, &atlas, &vertices, origin, [flat, shadow]);
                for (quad, (name, _, midpoint, fallback, mapped)) in cases.iter().enumerate() {
                    let centre = if flat { midpoint } else { fallback };
                    for (corner, uv) in mapped.iter().enumerate() {
                        let actual = pixels[4 * quad + corner];
                        let expected = [centre[0], centre[1], uv[0], uv[1]];
                        if actual
                            .iter()
                            .zip(expected)
                            .any(|(a, b)| !a.is_finite() || (a - b).abs() > 1e-6)
                        {
                            failures.push(format!("{name} corner {corner}, flat={flat} shadow={shadow} origin={origin}: {actual:?} != {expected:?}"));
                        }
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A stitched atlas (the one `stitched_sprites_keep_their_own_size_and_padding` works out: 16 by
/// 8, sprites of 2, 4 and 1 px at (9, 1), (1, 1) and (9, 5)): flat draws map a quad's UVs onto its
/// sprite's own pixels, so a corner is half the sprite's own side from `mc_midTexCoord`.
#[test]
fn flat_draws_map_uvs_onto_a_stitched_sprite() {
    let _gpu = crate::gfx::test_gpu::lock();
    let instance = crate::gfx::test_gpu::instance();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let array = super::tests::upscaled_array(&device, &queue, &[2, 4, 1]);
    let mut encoder = device.create_command_encoder(&Default::default());
    let atlas =
        PackAtlas::stitched(&device, &mut encoder, &[array], 3, 4, &[2, 4, 1], 0, 0).unwrap();
    queue.submit([encoder.finish()]);
    let whole = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
    let positions = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
    let vertices: Vec<_> = (0..3)
        .flat_map(|layer| {
            (0..4).map(move |k| {
                TerrainVertex::new(positions[k], whole[k], layer as u16 | 0x8000, u32::MAX, 0)
            })
        })
        .collect();
    // Corner and side of each layer's sprite, in atlas pixels.
    let sprites = [([9., 1.], 2.), ([1., 1.], 4.), ([9., 5.], 1.)];
    let origin = f32::from_bits((1. / UV_STEPS).to_bits());
    let pixels = sample(&device, &queue, &atlas, &vertices, origin, [true, false]);
    let mut failures = Vec::new();
    for (layer, ([x, y], side)) in sprites.into_iter().enumerate() {
        let mid = [(x + side / 2.) / 16., (y + side / 2.) / 8.];
        for (corner, [u, v]) in whole.iter().enumerate() {
            let expected = [mid[0], mid[1], (x + u * side) / 16., (y + v * side) / 8.];
            let actual = pixels[4 * layer + corner];
            if actual
                .iter()
                .zip(expected)
                .any(|(a, b)| (a - b).abs() > 1e-6)
            {
                failures.push(format!(
                    "layer {layer} corner {corner}: {actual:?} != {expected:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Read back with a probe pack of ours, Iris 1.11.7's terrain `gl_Vertex` (camera on the block
/// grid) puts a water top, 8/9 - 0.001 of a block up, at 29094/32768, and plants offset by 13/15
/// and 0.1 of a block at 28398/32768 and 3276/32768 (#8464): each 32768th below the position.
/// Whole blocks and sixteenths stay where they are, and so does a place float rounding put a hair
/// below a grid line: Iris reads plants offset by 9/15 exactly on a half block.
#[test]
fn terrain_vertices_go_down_to_the_32768th_below_them() {
    let _gpu = crate::gfx::test_gpu::lock();
    let instance = crate::gfx::test_gpu::instance();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let atlas = PackAtlas::new(&device, &mut encoder, &[texture], 1, 1, 16).unwrap();
    queue.submit([encoder.finish()]);
    let water = 8.0f32 / 9.0 - 0.001;
    let cases = [
        ([0.1, water, 13.0 / 15.0], [3276.0, 29094.0, 28398.0]),
        ([13.0 / 15.0, 0.1, water], [28398.0, 3276.0, 29094.0]),
        ([1.0, 0.5, 15.9375], [32768.0, 16384.0, 522240.0]),
        // One float step below 1, 0.5 and 9.5.
        (
            [
                1.0 - f32::EPSILON / 2.0,
                0.5 - f32::EPSILON / 4.0,
                9.5 - f32::EPSILON * 8.0,
            ],
            [32768.0, 16384.0, 311296.0],
        ),
    ];
    let vertices: Vec<_> = cases
        .iter()
        .map(|&(pos, _)| TerrainVertex::new(pos, [0.0, 0.0], 0x8000, u32::MAX, 0))
        .collect();
    let origin = f32::from_bits((1. / UV_STEPS).to_bits());
    let mut failures = Vec::new();
    for flat in [true, false] {
        for shadow in [false, true] {
            let read = sample_probe(
                &device,
                &queue,
                &atlas,
                &vertices,
                origin,
                [flat, shadow],
                "gl_Vertex",
            );
            for ((pos, expected), got) in cases.iter().zip(&read) {
                let got = [got[0], got[1], got[2]].map(|v| v * 32768.0);
                if got != *expected {
                    failures.push(format!(
                        "{pos:?}, flat={flat} shadow={shadow}: {got:?} != {expected:?}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A shader pack's flat terrain inputs read a compact record (`mesher::packed`) as the four
/// vertices it holds: a whole tinted face with its own ambient occlusion per corner
/// (`separateAo`), two more, and a slab's top kept as its vertices (`packed::FULL`).
#[test]
fn compact_records_feed_a_pack_the_vertices_they_hold() {
    let _gpu = crate::gfx::test_gpu::lock();
    use crate::mesher::packed;
    let instance = crate::gfx::test_gpu::instance();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut atlas = PackAtlas::new(&device, &mut encoder, &[texture], 1, 1, 16).unwrap();
    queue.submit([encoder.finish()]);
    atlas.separate_ao = true;
    let tint = 0x7F_B238u32;
    let ao = [255u32, 153, 102, 204];
    let face = |x: f32, y: f32| {
        let pos = [
            [x, y, 5.0],
            [x, y, 6.0],
            [x + 1.0, y, 6.0],
            [x + 1.0, y, 5.0],
        ];
        let uv = [[0, 0], [16 << 10, 0], [16 << 10, 16 << 10], [0, 16 << 10]];
        (0..4).map(move |i| {
            let [r, g, b] = [16, 8, 0].map(|s| 200 * (tint >> s & 255) / 255);
            TerrainVertex {
                pos: pos[i],
                uv: uv[i],
                layer: 0x8000,
                light: 0xF0 | 0x40 << 8,
                color: r | g << 8 | b << 16 | ao[i] << 24,
            }
        })
    };
    let solid: Vec<_> = [(3.0, 16.0), (5.0, 16.0), (7.0, 16.0), (3.0, 8.5)]
        .into_iter()
        .flat_map(|(x, y)| face(x, y))
        .collect();
    let records = packed::layout(&solid, &[tint; 4]);
    assert_eq!(
        packed::packed_quads(&records, 4),
        3,
        "whole faces pack, the slab doesn't"
    );
    let page = bytemuck::cast_slice(&records);
    let packed_range = packed::PACKED_BASE..packed::PACKED_BASE + 16;
    let origin = f32::from_bits((1. / UV_STEPS).to_bits());
    let mut failures = Vec::new();
    for expression in [
        "gl_Vertex",
        "gl_Color",
        "vec4(gl_MultiTexCoord0.xy, gl_MultiTexCoord1.xy)",
        "vec4(gl_Normal, 0.0)",
        "at_tangent",
        "at_midBlock",
        "vec4(mc_midTexCoord, mc_Entity.xy)",
    ] {
        for shadow in [false, true] {
            let flags = [true, shadow];
            let wide = sample_probe(&device, &queue, &atlas, &solid, origin, flags, expression);
            let compact = sample_page(
                &device,
                &queue,
                &atlas,
                page,
                packed_range.clone(),
                origin,
                flags,
                expression,
            );
            let bits = |v: &[[f32; 4]]| v.iter().flatten().map(|f| f.to_bits()).collect::<Vec<_>>();
            if bits(&wide) != bits(&compact) {
                failures.push(format!(
                    "{expression} shadow={shadow}: {wide:?} != {compact:?}"
                ));
            }
            if expression == "gl_Color" {
                for (corner, want) in ao.iter().enumerate() {
                    let got = compact[corner][3];
                    if (got - *want as f32 / 255.0).abs() > 1e-6 {
                        failures.push(format!("corner {corner}'s alpha {got} != {want}/255"));
                    }
                }
            }
        }
    }
    // A pack's frames keep the translucent quads in the section arena after the solid range: three
    // records and no `FULL` quad, then a water quad, whose corners must read as its own.
    let mut page: Vec<[u32; 6]> = packed::layout(&solid[..12], &[tint; 3]);
    let water: Vec<_> = face(9.0, 15.0).collect();
    let at = page.len() as u32;
    page.extend(water.iter().map(|v| bytemuck::cast::<_, [u32; 6]>(*v)));
    for expression in ["vec4(gl_Normal, 0.0)", "at_tangent", "at_midBlock"] {
        let flags = [true, false];
        let wide = sample_probe(&device, &queue, &atlas, &water, origin, flags, expression);
        let page = bytemuck::cast_slice(&page);
        let after = sample_page(
            &device,
            &queue,
            &atlas,
            page,
            at..at + 4,
            origin,
            flags,
            expression,
        );
        if wide != after {
            failures.push(format!("water {expression}: {wide:?} != {after:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Horizon PE: a pack is told each solid quad's block (`mc_Entity`, `at_midBlock`) from the
/// compact layout exactly as from whole vertices: a record's quad and a quad kept whole in it
/// alike. Starling 0.9.0 kept no source for a compact section, so under a shader pack no solid
/// block had an id and leaves, plants and crops stood still.
#[test]
fn compact_records_keep_each_quads_block() {
    let _gpu = crate::gfx::test_gpu::lock();
    use crate::mesher::{TerrainSource, packed};
    let instance = crate::gfx::test_gpu::instance();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        // Where a GPU is promised (ios-guard's software one), none is a failure, not a pass.
        assert!(
            std::env::var_os("HORIZON_REQUIRE_GPU").is_none(),
            "no adapter to test on"
        );
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let atlas = PackAtlas::new(&device, &mut encoder, &[texture], 1, 1, 16).unwrap();
    queue.submit([encoder.finish()]);
    let tint = 0x7F_B238u32;
    let face = |x: f32, y: f32| {
        let pos = [
            [x, y, 5.0],
            [x, y, 6.0],
            [x + 1.0, y, 6.0],
            [x + 1.0, y, 5.0],
        ];
        let uv = [[0, 0], [16 << 10, 0], [16 << 10, 16 << 10], [0, 16 << 10]];
        (0..4).map(move |i| {
            let [r, g, b] = [16, 8, 0].map(|s| 200 * (tint >> s & 255) / 255);
            TerrainVertex {
                pos: pos[i],
                uv: uv[i],
                layer: 0x8000,
                light: 0xF0 | 0x40 << 8,
                color: r | g << 8 | b << 16 | 255 << 24,
            }
        })
    };
    // Three whole faces, which pack, and a slab's top, which stays four vertices.
    let solid: Vec<_> = [(3.0, 16.0), (5.0, 16.0), (7.0, 16.0), (3.0, 8.5)]
        .into_iter()
        .flat_map(|(x, y)| face(x, y))
        .collect();
    let records = packed::layout(&solid, &[tint; 4]);
    assert_eq!(packed::packed_quads(&records, 4), 3);
    // Each quad's block: its state, its cell in the section, the light it gives off, and
    // whether it is a fluid.
    let blocks = [
        (1u32, [3, 15, 5], 0u8, false),
        (2, [5, 15, 5], 7, false),
        (3, [7, 15, 5], 15, false),
        (4, [3, 8, 5], 4, true),
    ];
    let sources =
        blocks.map(|(state, cell, light, fluid)| TerrainSource::new(state, cell, light, fluid));
    // The pack's ids of five states: 100 and up as a model, 200 and up as a fluid.
    let table: Vec<i32> = (0..5)
        .flat_map(|state| [100 + state, 200 + state])
        .collect();
    // One source a slot, at its quad's first: every fourth among whole vertices, the first
    // slots (the records, in the quads' order) of the compact layout.
    let unknown = TerrainSource::default();
    let wide_sources: Vec<_> = sources
        .iter()
        .flat_map(|&source| [source, unknown, unknown, unknown])
        .collect();
    let mut compact_sources = vec![TerrainSource::default(); records.len()];
    compact_sources[..sources.len()].copy_from_slice(&sources);
    let origin = f32::from_bits((1. / UV_STEPS).to_bits());
    let packed_range = packed::PACKED_BASE..packed::PACKED_BASE + 16;
    let mut failures = Vec::new();
    for expression in ["mc_Entity", "at_midBlock"] {
        for shadow in [false, true] {
            let flags = [true, shadow];
            let wide = sample_sourced(
                &device,
                &queue,
                &atlas,
                bytemuck::cast_slice(&solid),
                0..16,
                origin,
                flags,
                expression,
                bytemuck::cast_slice(&wide_sources),
                &table,
            );
            let compact = sample_sourced(
                &device,
                &queue,
                &atlas,
                bytemuck::cast_slice(&records),
                packed_range.clone(),
                origin,
                flags,
                expression,
                bytemuck::cast_slice(&compact_sources),
                &table,
            );
            if wide != compact {
                failures.push(format!(
                    "{expression} shadow={shadow}: {wide:?} != {compact:?}"
                ));
            }
            // And what they agree on is the block, not "unknown" twice.
            for (quad, (state, _, light, fluid)) in blocks.iter().enumerate() {
                let base = if *fluid { 200 } else { 100 };
                let id = (base + state) as f32;
                for corner in &compact[4 * quad..4 * quad + 4] {
                    let right = if expression == "mc_Entity" {
                        corner[0] == id && corner[1] == f32::from(u8::from(*fluid))
                    } else {
                        corner[3] == f32::from(*light)
                    };
                    if !right {
                        failures.push(format!(
                            "{expression} shadow={shadow}: quad {quad} reads {corner:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn sample(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    atlas: &PackAtlas,
    vertices: &[TerrainVertex],
    scale: f32,
    flags: [bool; 2],
) -> Vec<[f32; 4]> {
    sample_probe(
        device,
        queue,
        atlas,
        vertices,
        scale,
        flags,
        "vec4(mc_midTexCoord, gl_MultiTexCoord0.xy)",
    )
}

/// What the pack's vertex stage sets `probe` to (`expression`), for each of `vertices`.
fn sample_probe(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    atlas: &PackAtlas,
    vertices: &[TerrainVertex],
    scale: f32,
    flags: [bool; 2],
    expression: &str,
) -> Vec<[f32; 4]> {
    let page = bytemuck::cast_slice(vertices);
    let drawn = 0..vertices.len() as u32;
    sample_page(device, queue, atlas, page, drawn, scale, flags, expression)
}

/// `sample_probe` of the vertex indices `drawn` of an arena page holding `page`.
#[allow(clippy::too_many_arguments)]
fn sample_page(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    atlas: &PackAtlas,
    page: &[u8],
    drawn: std::ops::Range<u32>,
    scale: f32,
    flags: [bool; 2],
    expression: &str,
) -> Vec<[f32; 4]> {
    // No quad's source is known, and the pack has no ids.
    let unknown = [0u32; 2];
    let sources = bytemuck::bytes_of(&unknown);
    sample_sourced(
        device,
        queue,
        atlas,
        page,
        drawn,
        scale,
        flags,
        expression,
        sources,
        &[],
    )
}

/// `sample_page` with the page's source records, one a slot (`arena::write_sources`), and the
/// pack's ids of the states, a model's and a fluid's each (`pack_post::terrain_tables`).
#[allow(clippy::too_many_arguments)]
fn sample_sourced(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    atlas: &PackAtlas,
    page: &[u8],
    drawn: std::ops::Range<u32>,
    scale: f32,
    [flat, shadow]: [bool; 2],
    expression: &str,
    sources: &[u8],
    table: &[i32],
) -> Vec<[f32; 4]> {
    let count = drawn.len() as u32;
    let pp = |code| horizon_shaders::Preprocessed {
        code,
        ..Default::default()
    };
    let vertex = pp(format!(
        "attribute vec2 mc_midTexCoord;\nattribute vec4 at_tangent;\n\
         attribute vec4 at_midBlock;\nattribute vec4 mc_Entity;\nvarying vec4 probe;\nvoid main() {{\n\
         probe = {expression};\n\
         gl_Position = vec4((float(gl_VertexID - {}) + 0.5) * {} - 1.0, 0.0, 0.0, 1.0);\n}}",
        drawn.start,
        2. / count as f32,
    ));
    // Integer attachments retain the float bits without requiring float-render-target
    // capabilities on the adapter that the test runner picks.
    let fragment = pp(
        "varying vec4 probe;\nlayout(location = 0) out uvec4 midpoint_bits;\n\
        void main() { midpoint_bits = floatBitsToUint(probe); }"
            .into(),
    );
    let inputs = atlas.terrain_inputs(flat);
    let program = if shadow {
        horizon_shaders::translate_shadow(&vertex, &fragment, &inputs)
    } else {
        horizon_shaders::translate_gbuffers(
            &vertex,
            &fragment,
            horizon_shaders::Target::Buffers,
            &inputs,
        )
    }
    .unwrap();
    let storage = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let entries = [
        vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
        vec![storage(0), storage(1)],
        vec![],
        vec![storage(0), storage(1), storage(2)],
    ];
    let layouts: Vec<_> = entries
        .iter()
        .map(|entries| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries,
            })
        })
        .collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &layouts.iter().map(Some).collect::<Vec<_>>(),
        ..Default::default()
    });
    let module = |module| {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
        })
    };
    let vs = module(program.vertex);
    let fs = module(program.fragment);
    let vertex_layouts = [
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &crate::gfx::TERRAIN_ATTRIBUTES,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![5 => Float32x4],
        }),
    ];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &vs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            // Flat draws pull their vertices from the page (`PULL_FLAT`), as in the game.
            buffers: if flat { &[] } else { &vertex_layouts[..] },
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::PointList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &fs,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Uint,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let buffer = |contents: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents,
            usage,
        })
    };
    let uniform = buffer(
        &vec![0; program.interface.uniform_size.max(16) as usize],
        wgpu::BufferUsages::UNIFORM,
    );
    let slots = buffer(bytemuck::bytes_of(&0u32), wgpu::BufferUsages::STORAGE);
    let origin = buffer(
        bytemuck::bytes_of(&[0., 0., 0., scale]),
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
    );
    let ids = buffer(sources, wgpu::BufferUsages::STORAGE);
    let data = buffer(
        page,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
    );
    let rects = buffer(
        bytemuck::cast_slice(&super::super::pack_post::terrain_tables(
            table,
            &atlas.rects_table(),
        )),
        wgpu::BufferUsages::STORAGE,
    );
    let bindings = [
        vec![(0, &uniform)],
        vec![(0, &slots), (1, &origin)],
        vec![],
        vec![(0, &ids), (1, &data), (2, &rects)],
    ];
    let groups: Vec<_> = bindings
        .iter()
        .zip(&layouts)
        .map(|(buffers, layout)| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries: &buffers
                    .iter()
                    .map(|&(binding, buffer)| wgpu::BindGroupEntry {
                        binding,
                        resource: buffer.as_entire_binding(),
                    })
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: count,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Uint,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(count) * 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let view = output.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        for (k, group) in groups.iter().enumerate() {
            pass.set_bind_group(k as u32, group, &[]);
        }
        if !flat {
            pass.set_vertex_buffer(0, data.slice(..));
            pass.set_vertex_buffer(1, origin.slice(..));
        }
        pass.draw(drawn, 0..1);
    }
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &read,
            layout: wgpu::TexelCopyBufferLayout::default(),
        },
        output.size(),
    );
    queue.submit([encoder.finish()]);
    read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = read.slice(..).get_mapped_range().unwrap();
    bytemuck::cast_slice::<u8, [f32; 4]>(&mapped).to_vec()
}
