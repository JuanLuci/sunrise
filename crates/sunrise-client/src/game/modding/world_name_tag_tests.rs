//! `WorldDrawContext::name_tag` (#4171): a mod's label hides behind a wall unless it is
//! see-through, shrinks with distance as a name tag does, is refused whole for text too deep
//! or a position that is not finite, draws as vanilla's name tags in its light and the
//! player's text background opacity, and draws the same from a compiled wasm mod.
use super::host::draw::GameWorldDrawContext;
use super::native_entity_refs::tests::with_client_game;
use glam::{DVec3, Mat4, Vec3};
use mod_api::{DrawError, Hooks, NameTagOptions, Text, WorldDrawContext};
use sunrise_client::block_entity_render::vertex_light;
use sunrise_client::{mesher::Vertex, world_geometry::WorldGeometry};

#[path = "../../../../sunrise-mod-wasm/tests/fixture/src/world_name_tags.rs"]
mod scene;

/// Four blocks in front of the label's anchor (0.5 above (0.5, 70, 0.5)), facing south (+Z).
const CAMERA: [f64; 3] = [0.5, 70.5, -3.5];

/// The view the tests draw with: the camera looking south, reversed depth as the game's.
fn view_proj() -> Mat4 {
    let projection = Mat4::perspective_rh(70f32.to_radians(), 1.0, 512.0, 0.05);
    projection * Mat4::look_to_rh(Vec3::ZERO, Vec3::Z, Vec3::Y)
}

/// The game's camera at [`CAMERA`]'s facing: south, first person.
fn face_south(game: &mut super::Game<'_>) {
    game.yaw = 0.0;
    game.pitch = 0.0;
    game.posed_rotation = None;
    game.camera_type = sunrise_client::camera::CameraType::FirstPerson;
}

/// One view's labels at `at`, the game's camera facing south: what it draws with the name
/// tags, then the see-through copies.
fn labels(
    game: &mut super::Game<'_>,
    at: [f64; 3],
    text: &Text,
    options: NameTagOptions,
) -> (Result<(), DrawError>, [Vec<Vertex>; 2]) {
    face_south(game);
    let mut draw = WorldGeometry::default();
    draw.reset(CAMERA, Mat4::IDENTITY, [0.5, 0.5]);
    let result = GameWorldDrawContext::new(game, &mut draw).name_tag(at, text, options);
    (result, draw.labels().map(<[Vertex]>::to_vec))
}

/// The alphas of a label's text quads and of its backdrop quads (black), each sorted and
/// without repeats.
fn alphas(vertices: &[Vertex]) -> (Vec<u32>, Vec<u32>) {
    let (mut text, mut backdrop) = (Vec::new(), Vec::new());
    for v in vertices {
        let out = if v.color & 0xFF_FFFF == 0 {
            &mut backdrop
        } else {
            &mut text
        };
        out.push(v.color >> 24);
    }
    for list in [&mut text, &mut backdrop] {
        list.sort_unstable();
        list.dedup();
    }
    (text, backdrop)
}

/// `vertices` with every light cleared, as bytes: two labels that differ only in light match.
fn without_light(vertices: &[Vertex]) -> Vec<u8> {
    let cleared: Vec<Vertex> = vertices.iter().map(|v| Vertex { light: 0, ..*v }).collect();
    bytemuck::cast_slice(&cleared).to_vec()
}

fn bytes(vertices: &[Vertex]) -> &[u8] {
    bytemuck::cast_slice(vertices)
}

/// An opaque grey wall two blocks in front of the camera, in the white texel.
fn wall(game: &super::Game<'_>) -> Vec<Vertex> {
    let white = game.gui.atlas.sprites["sunrise/white"];
    let uv = [(white.u0 + white.u1) * 0.5, (white.v0 + white.v1) * 0.5];
    // Counterclockwise as seen from the camera.
    [[4.0, -4.0], [-4.0, -4.0], [-4.0, 4.0], [4.0, 4.0]]
        .map(|[x, y]| Vertex {
            pos: [x, y, 2.0],
            uv,
            layer: 0,
            color: 0xFF80_8080,
            light: vertex_light(0xF0),
            normal: [0; 4],
            glint_uv: [0.0; 2],
        })
        .to_vec()
}

/// The offscreen pixels of `world` (the entity pass) then `see_through` (no depth test), under
/// a lightmap whose brightness follows the light level, so light shows in the pixels.
fn pixels(game: &super::Game<'_>, world: &[Vertex], see_through: &[Vertex]) -> Vec<u8> {
    use sunrise_client::gfx::entity_pixel_test::{graded_lightmap, render_see_through_lit};
    render_see_through_lit(
        world,
        see_through,
        [game.gui.atlas.width, game.gui.atlas.height],
        &game.gui.atlas.pixels(),
        view_proj(),
        &graded_lightmap(),
    )
    .expect("software Vulkan label pixels")
}

fn brightness(pixels: &[u8]) -> u64 {
    pixels.iter().map(|&c| u64::from(c)).sum()
}

#[test]
#[ignore = "daily"]
fn a_label_hides_behind_a_wall_unless_it_is_see_through() {
    with_client_game(|game| {
        let text = Text::literal("Label");
        let at = [0.5, 70.0, 0.5];
        let (ok, [world, through]) =
            labels(game, at, &text, NameTagOptions::new(false, true, 0xF0));
        assert_eq!(ok, Ok(()));
        assert!(!world.is_empty() && through.is_empty());
        let empty = pixels(game, &[], &[]);
        assert_ne!(pixels(game, &world, &[]), empty, "visible without a wall");
        let wall = wall(game);
        let only_wall = pixels(game, &wall, &[]);
        let behind = [wall.clone(), world].concat();
        assert_eq!(
            pixels(game, &behind, &[]),
            only_wall,
            "hidden behind the wall"
        );

        let (ok, [world, through]) = labels(game, at, &text, NameTagOptions::new(true, true, 0xF0));
        assert_eq!(ok, Ok(()));
        assert!(!world.is_empty() && !through.is_empty());
        let behind = [wall, world].concat();
        let shown = pixels(game, &behind, &through);
        assert_ne!(shown, only_wall, "the dim copy over the wall");
    });
}

/// Default options without `see_through`: one depth-tested translucent copy, the faded text
/// (0x7F) on the backdrop (0x3F), in the caller's light exactly: light 0 is not raised to 2.
#[test]
#[ignore = "daily"]
fn without_see_through_a_label_is_one_translucent_copy_in_the_callers_light() {
    with_client_game(|game| {
        let text = Text::literal("Label");
        let drawn = |game: &mut super::Game<'_>, light| {
            let options = NameTagOptions::new(false, true, light);
            let (ok, [world, through]) = labels(game, [0.5, 70.0, 0.5], &text, options);
            assert_eq!(ok, Ok(()));
            assert!(through.is_empty(), "one copy");
            world
        };
        let [dark, two, full] = [0x00, 0x22, 0xFF].map(|light| drawn(game, light));
        assert_eq!(alphas(&dark), (vec![0x7F], vec![0x3F]));
        for (label, light) in [(&dark, 0x00), (&two, 0x22), (&full, 0xFF)] {
            assert!(
                label.iter().all(|v| v.light == vertex_light(light)),
                "light {light:#04x} as given"
            );
            assert!(label.iter().all(|v| v.layer == 0));
        }
        // The same label but for its light, which alone sets its brightness.
        assert_eq!(without_light(&dark), without_light(&full));
        let [dark, two, full] =
            [&dark, &two, &full].map(|label| brightness(&pixels(game, label, &[])));
        assert!(dark < two && two < full, "{dark} < {two} < {full}");
        let wall = wall(game);
        let label = drawn(game, 0x00);
        assert_eq!(
            pixels(game, &[wall.clone(), label].concat(), &[]),
            pixels(game, &wall, &[]),
            "depth tested: hidden behind the wall"
        );
    });
}

/// `see_through` with light 0: the copy in the world is opaque white with no backdrop at the
/// brightness of light 2; the faded copy (0x7F on 0x3F) is unlit and does not change with
/// light; behind a wall only the faded copy and its backdrop show.
#[test]
#[ignore = "daily"]
fn a_see_through_label_is_lit_at_least_2_in_the_world_and_unlit_over_it() {
    with_client_game(|game| {
        let text = Text::literal("Label");
        let drawn = |game: &mut super::Game<'_>, light| {
            let options = NameTagOptions::new(true, true, light);
            let (ok, copies) = labels(game, [0.5, 70.0, 0.5], &text, options);
            assert_eq!(ok, Ok(()));
            copies
        };
        let [world, through] = drawn(game, 0x00);
        assert_eq!(alphas(&world), (vec![0xFF], vec![]), "opaque, no backdrop");
        assert!(world.iter().all(|v| v.light == vertex_light(0x22)));
        assert_eq!(alphas(&through), (vec![0x7F], vec![0x3F]));
        assert!(through.iter().all(|v| v.light == 0 && v.layer == 2));
        let [world_two, _] = drawn(game, 0x22);
        assert_eq!(bytes(&world), bytes(&world_two), "light 0 drawn as light 2");
        assert_eq!(pixels(game, &world, &[]), pixels(game, &world_two, &[]));
        let [_, through_full] = drawn(game, 0xFF);
        assert_eq!(
            bytes(&through),
            bytes(&through_full),
            "the faded copy ignores light"
        );
        assert_eq!(
            pixels(game, &[], &through),
            pixels(game, &[], &through_full)
        );
        let wall = wall(game);
        let only_wall = pixels(game, &wall, &[]);
        let behind = pixels(game, &[wall.clone(), world].concat(), &through);
        assert_eq!(
            behind,
            pixels(game, &wall, &through),
            "behind the wall only the faded copy and its backdrop"
        );
        assert_ne!(behind, only_wall);
    });
}

/// The text background option moved off chat only, at 0.5: backdrop 127, faded text 159, for
/// the one translucent copy and for the see-through copy alike.
#[test]
#[ignore = "daily"]
fn the_text_background_opacity_sets_the_backdrop_and_the_faded_text() {
    with_client_game(|game| {
        use sunrise_client::options_file::V;
        game.settings
            .file
            .set("backgroundForChatOnly", V::Bool(false));
        game.settings.text_background_opacity = 0.5;
        let text = Text::literal("Label");
        let at = [0.5, 70.0, 0.5];
        let (ok, [world, _]) = labels(game, at, &text, NameTagOptions::new(false, true, 0xF0));
        assert_eq!(ok, Ok(()));
        assert_eq!(alphas(&world), (vec![159], vec![127]));
        let (ok, [world, through]) = labels(game, at, &text, NameTagOptions::new(true, true, 0xF0));
        assert_eq!(ok, Ok(()));
        assert_eq!(alphas(&world), (vec![0xFF], vec![]));
        assert_eq!(alphas(&through), (vec![159], vec![127]));
        // Chat only again: the default 0.25 whatever the slider says.
        game.settings
            .file
            .set("backgroundForChatOnly", V::Bool(true));
        let (_, [world, _]) = labels(game, at, &text, NameTagOptions::new(false, true, 0xF0));
        assert_eq!(alphas(&world), (vec![0x7F], vec![0x3F]));
    });
}

/// A label refused for capacity after its layout rasterized a new glyph still uploads that
/// glyph's pixels: a later accepted label with the glyph needs none of its own and samples
/// atlas texels that are queued.
#[test]
#[ignore = "daily"]
fn glyphs_of_a_label_refused_for_capacity_still_upload() {
    with_client_game(|game| {
        face_south(game);
        // The default font's runes are already packed bitmap glyphs, not runtime
        // uploads. Force Unifont and an empty per-game cache so this layout must
        // rasterize them, independently of any text drawn during game setup.
        game.gui.atlas.font.uniform = true;
        game.gui.atlas.font.reset_cache();
        let text = Text::literal("\u{16A0}\u{16A2}\u{16A6}");
        let at = [0.5, 70.0, 0.5];
        let options = NameTagOptions::new(false, true, 0xF0);
        let mut draw = WorldGeometry::default();
        draw.reset(CAMERA, Mat4::IDENTITY, [0.5, 0.5]);
        let quad = mod_api::WorldQuad::new([[0.5, 70.5, 0.0]; 4], !0, mod_api::WorldDepth::Tested);
        let capacity = draw.usage().quad_capacity as usize;
        draw.quads(&vec![quad; capacity - 1]).unwrap();
        game.text_patches.borrow_mut().clear();
        let refused = GameWorldDrawContext::new(game, &mut draw).name_tag(at, &text, options);
        assert_eq!(refused, Err(DrawError::CapacityExceeded));
        assert!(draw.labels().iter().all(|label| label.is_empty()));
        let queued: Vec<[u32; 4]> = game
            .text_patches
            .borrow()
            .iter()
            .map(|p| [p.x, p.y, p.width, p.height])
            .collect();
        assert!(
            !queued.is_empty(),
            "the refused label's new glyphs still upload"
        );

        draw.reset(CAMERA, Mat4::IDENTITY, [0.5, 0.5]);
        let accepted = GameWorldDrawContext::new(game, &mut draw).name_tag(at, &text, options);
        assert_eq!(accepted, Ok(()));
        assert_eq!(
            game.text_patches.borrow().len(),
            queued.len(),
            "the glyphs were cached by the refused label"
        );
        let size = [game.gui.atlas.width as f32, game.gui.atlas.height as f32];
        let glyphs: Vec<&[Vertex; 4]> = draw.labels()[0]
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|quad| quad[0].color & 0xFF_FFFF != 0)
            .collect();
        assert!(!glyphs.is_empty());
        for quad in glyphs {
            // The centre of the glyph's texels, in atlas pixels.
            let [u, v] = quad
                .iter()
                .fold([0.0f32; 2], |[u, v], c| [u + c.uv[0], v + c.uv[1]]);
            let (x, y) = (u / 4.0 * size[0], v / 4.0 * size[1]);
            assert!(
                queued.iter().any(|&[px, py, w, h]| {
                    (px as f32..(px + w) as f32).contains(&x)
                        && (py as f32..(py + h) as f32).contains(&y)
                }),
                "glyph texel ({x}, {y}) is in a queued patch: {queued:?}"
            );
        }
    });
}

/// The shared name-tag tessellation's styles: bold counts in the centring width, plain text has
/// no drop shadow or outline (a quad a glyph), and a shadow is drawn only for a style that sets
/// a shadow colour.
#[test]
#[ignore = "daily"]
fn a_label_keeps_the_name_tag_styles() {
    with_client_game(|game| {
        let drawn = |game: &mut super::Game<'_>, style: mod_api::text::TextStyle| {
            let text = Text::literal("Bold").with_style(style);
            let options = NameTagOptions::new(false, false, 0xF0);
            let (ok, [world, _]) = labels(game, [0.5, 70.0, 0.5], &text, options);
            assert_eq!(ok, Ok(()));
            world
        };
        let width = |label: &[Vertex]| {
            let (lo, hi) = label
                .iter()
                .map(|v| v.pos[0])
                .fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
            hi - lo
        };
        let plain = drawn(game, Default::default());
        assert_eq!(plain.len() / 4, 4, "a quad a glyph: no shadow, no outline");
        let mut bold = mod_api::text::TextStyle::default();
        bold.bold = Some(true);
        let bold = drawn(game, bold);
        assert!(width(&bold) > width(&plain), "bold is wider");
        // Centred on the anchor whatever its width.
        for label in [&plain, &bold] {
            let (lo, hi) = label
                .iter()
                .map(|v| v.pos[0])
                .fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
            assert!(((lo + hi) / 2.0).abs() < 0.03, "centred: {lo}..{hi}");
        }
        let mut shadowed = mod_api::text::TextStyle::default();
        shadowed.shadow_color = Some(0xFF00_0000);
        assert!(
            drawn(game, shadowed).len() > plain.len(),
            "a shadow under the glyphs"
        );
    });
}

#[test]
#[ignore = "daily"]
fn labels_four_times_as_far_are_a_quarter_the_size_on_screen() {
    with_client_game(|game| {
        let text = Text::literal("Size");
        let width = |game: &mut super::Game<'_>, z: f64| {
            // The anchor is 0.5 below the label; the label `z` blocks ahead of the camera.
            let at = [CAMERA[0], CAMERA[1] - 0.5, CAMERA[2] + z];
            let (ok, [world, _]) = labels(game, at, &text, NameTagOptions::new(false, false, 0xF0));
            assert_eq!(ok, Ok(()));
            let xs = world
                .iter()
                .map(|v| view_proj().project_point3(Vec3::from(v.pos)).x);
            let (lo, hi) = xs.fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
            hi - lo
        };
        let (near, far) = (width(game, 4.0), width(game, 16.0));
        assert!(near > 0.0);
        assert!((near / far - 4.0).abs() < 1e-3, "{near} / {far}");
    });
}

#[test]
#[ignore = "daily"]
fn a_label_too_deep_or_not_finite_is_refused_and_draws_nothing() {
    with_client_game(|game| {
        let mut too_deep = Text::literal("leaf");
        for _ in 1..513 {
            too_deep = Text::literal("parent").append(too_deep);
        }
        let options = NameTagOptions::new(true, true, 0xF0);
        let (refused, drawn) = labels(game, [0.5, 70.0, 0.5], &too_deep, options);
        assert_eq!(refused, Err(DrawError::TextTooDeep));
        assert!(drawn.iter().all(Vec::is_empty));
        for at in [[f64::NAN, 70.0, 0.5], [0.5, f64::INFINITY, 0.5]] {
            let (refused, drawn) = labels(game, at, &Text::literal("x"), options);
            assert_eq!(refused, Err(DrawError::NonFinite));
            assert!(drawn.iter().all(Vec::is_empty));
        }
        // Drawn half a block above its anchor: in the plane four blocks ahead, facing the camera.
        let (_, [world, _]) = labels(game, [0.5, 70.0, 0.5], &Text::literal("x"), options);
        let centre = world
            .iter()
            .fold(DVec3::ZERO, |sum, v| sum + Vec3::from(v.pos).as_dvec3())
            / world.len() as f64;
        assert!((centre.z - 4.0).abs() < 0.01, "{centre}");
    });
}

/// The wasm fixture's scene, run natively.
struct Native;
#[mod_api::__subscriptions]
impl Hooks for Native {
    fn render_level_stage(
        &self,
        _: &dyn mod_api::ClientHost,
        event: &mut mod_api::RenderLevelStageEvent<'_>,
    ) {
        scene::render_level_stage(event);
    }
}

/// The scene drawn by a whole world view (`fill_world_geometry`, every stage): the name-tag
/// vertices it adds to the entities, its see-through vertices, and their pixels in front of
/// a wall that hides the labels that are not see-through.
fn scene_frame(game: &mut super::Game<'_>) -> ([Vec<Vertex>; 2], Vec<u8>, Vec<u32>) {
    face_south(game);
    game.world.game_time = scene::SCENE_TIME;
    game.see_through_vertices.clear();
    let mut draw = WorldGeometry::default();
    let mut entities = Vec::new();
    game.fill_world_geometry(false, CAMERA, Mat4::IDENTITY, 0.0, &mut draw, &mut entities);
    let see_through = std::mem::take(&mut game.see_through_vertices);
    let open = pixels(game, &entities, &see_through);
    let walled = pixels(game, &[wall(game), entities.clone()].concat(), &see_through);
    // Each call's result, from the scene's marker lines: `code` across, the call's index up.
    let mut results: Vec<(f32, u32)> = draw
        .batch()
        .lines
        .iter()
        .map(|line| (line[1], line[0].round() as u32))
        .collect();
    results.sort_by(|a, b| a.0.total_cmp(&b.0));
    let results = results.into_iter().map(|(_, code)| code).collect();
    ([entities, see_through], [open, walled].concat(), results)
}

/// The same `name_tag` calls from the compiled wasm fixture (through the generated import)
/// and from a native mod draw the same vertices and the same pixels (#4171's done-when).
#[test]
#[ignore = "daily"]
fn a_wasm_mods_labels_draw_the_same_pixels_as_a_native_mods() {
    crate::game::in_own_process(
        "game::world_name_tag_tests::a_wasm_mods_labels_draw_the_same_pixels_as_a_native_mods",
        || {
            sunrise_guest_stack::run_on_guest_stack(|| {
                sunrise_mod_wasm::copy_probe::with_load_budget(|| {
                    let native: mod_api::Mods =
                        vec![mod_api::ModEntry::new("wasm_fixture", Box::new(Native))].into();
                    let mut expected = None;
                    mod_platform::testing::with_mods(native, || {
                        with_client_game(|game| expected = Some(scene_frame(game)));
                    });
                    let (expected_vertices, expected_pixels, native_results) = expected.unwrap();
                    assert_eq!(
                        native_results,
                        scene::RESULT_CODES,
                        "native name_tag results"
                    );
                    let [world, through] = &expected_vertices;
                    // Hidden (one copy), Through (two) and Bold (one) drew; NaN was refused.
                    assert!(!world.is_empty() && !through.is_empty());

                    let mods: mod_api::Mods = wasm_mods().into();
                    let mut actual = None;
                    mod_platform::testing::with_mods(mods.clone(), || {
                        crate::game::api_values::install_parts(&mods).unwrap();
                        let base = std::path::PathBuf::from(
                            std::env::var_os("SUNRISE_TEST_RESOURCES").unwrap(),
                        );
                        let root = super::mods::content_root(&base, &Default::default()).unwrap();
                        super::native_entity_refs::tests::with_game_role(
                            root.prepared().to_owned(),
                            crate::game::execution_role::ExecutionRole::Client,
                            |game| actual = Some(scene_frame(game)),
                        );
                    });
                    let (actual_vertices, actual_pixels, wasm_results) = actual.unwrap();
                    assert_eq!(wasm_results, scene::RESULT_CODES, "wasm name_tag results");
                    for (copy, (native, wasm)) in ["world", "see-through"]
                        .iter()
                        .zip(expected_vertices.iter().zip(&actual_vertices))
                    {
                        assert_eq!(bytes(native), bytes(wasm), "the {copy} vertices");
                    }
                    assert!(expected_pixels == actual_pixels, "the same pixels");
                })
            })
        },
    );
}

/// The compiled wasm fixture (`SUNRISE_WASM_FIXTURE`, on `SUNRISE_WASM_STD`), loaded.
fn wasm_mods() -> Vec<mod_api::ModEntry> {
    use sunrise_mod_package::manifest::Package;
    use sunrise_mod_wasm::{WasmPackage, load};
    let path = std::path::PathBuf::from(std::env::var_os("SUNRISE_WASM_FIXTURE").unwrap());
    let manifest = Package::read(&path).unwrap().manifest;
    let info = mod_api::PackageInfo::new(
        manifest.id.clone(),
        manifest.version.clone(),
        mod_api::PackageKind::Code,
        mod_api::Side::Both,
        vec![manifest.id.clone()],
        vec![],
    );
    let loaded = load(
        &std::fs::read(std::env::var_os("SUNRISE_WASM_STD").unwrap()).unwrap(),
        vec![WasmPackage {
            manifest,
            module: Package::read_wasm(&path).unwrap(),
        }],
    );
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    assert_eq!(loaded.mods.len(), 1);
    loaded.initialize(&[info]).unwrap();
    loaded.mods
}
