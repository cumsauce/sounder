//! Built-in parts: layer-sized sounds an agent assembles with the `sound`
//! node instead of rebuilding them (a wood body, a noise crack, a low thump).
//! Embedded at build time from `parts/`; `catalog search` lists them beside
//! the library's own sounds, and a library sound of the same name wins.

use serde::Deserialize;
use sfx_graph::SoundDoc;

use crate::catalog::CatalogEntry;

/// The roles a part can fill in a layered sound (`catalog save` `role`).
pub const ROLES: &[&str] = &[
    "pre", "crack", "body", "weight", "after", "bed", "texture", "whole",
];

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub role: String,
    pub material: String,
    pub description: String,
    pub tags: Vec<String>,
    /// Where it came from and what was approved by ear.
    pub origin: String,
    pub doc: SoundDoc,
}

const FILES: &[&str] = &[
    include_str!("../parts/wood_brush.json"),
    include_str!("../parts/noise_crack.json"),
    include_str!("../parts/wood_panel_body.json"),
    include_str!("../parts/low_thump.json"),
    include_str!("../parts/wood_knock.json"),
    include_str!("../parts/wooden_crate_drop.json"),
    include_str!("../parts/large_pipe_drop.json"),
    include_str!("../parts/large_metal_container_hit.json"),
    include_str!("../parts/metal_plate_gong_hit.json"),
    include_str!("../parts/metal_bell_hit.json"),
    include_str!("../parts/metal_into_small_glass.json"),
    include_str!("../parts/small_object_taps.json"),
    include_str!("../parts/mouse_click_tap.json"),
    include_str!("../parts/rattlesnake_rattle.json"),
    include_str!("../parts/scissors_snip.json"),
    include_str!("../parts/grass_swish.json"),
    include_str!("../parts/gravel_crunch.json"),
    include_str!("../parts/gravel_step.json"),
    include_str!("../parts/grass_step.json"),
    include_str!("../parts/concrete_step.json"),
    include_str!("../parts/all_purpose_step.json"),
    include_str!("../parts/splash_close.json"),
    include_str!("../parts/small_motor_spin.json"),
    include_str!("../parts/small_frog.json"),
    include_str!("../parts/cave_drips.json"),
    include_str!("../parts/rain_on_stone.json"),
    include_str!("../parts/underwater_bed.json"),
    include_str!("../parts/wind_in_trees.json"),
    include_str!("../parts/lava_bubbling.json"),
    include_str!("../parts/surf_shingle.json"),
    include_str!("../parts/creature_whine.json"),
    include_str!("../parts/creature_snarl.json"),
    include_str!("../parts/creature_growl.json"),
    include_str!("../parts/creature_roar.json"),
    include_str!("../parts/keyboard_button_hit.json"),
    include_str!("../parts/brick_or_rock_drop.json"),
    include_str!("../parts/small_brick_drop.json"),
    include_str!("../parts/heavy_stone_hit.json"),
    include_str!("../parts/alarm_whoop.json"),
    include_str!("../parts/bells_ring.json"),
    include_str!("../parts/bird_call.json"),
    include_str!("../parts/breath_out.json"),
    include_str!("../parts/breath_swell_pre.json"),
    include_str!("../parts/button_latch_press.json"),
    include_str!("../parts/cricket_chirp.json"),
    include_str!("../parts/crunch_after.json"),
    include_str!("../parts/dead_low_hit_body.json"),
    include_str!("../parts/deep_bubble_pop.json"),
    include_str!("../parts/dense_plastic_hit.json"),
    include_str!("../parts/door_creak.json"),
    include_str!("../parts/double_small_click_crack.json"),
    include_str!("../parts/dull_soft_crack.json"),
    include_str!("../parts/engine_rev.json"),
    include_str!("../parts/error_beep.json"),
    include_str!("../parts/fabric_sweep_pre.json"),
    include_str!("../parts/fan_hum.json"),
    include_str!("../parts/fast_pats_after.json"),
    include_str!("../parts/fire_crackle_bed.json"),
    include_str!("../parts/fire_crackle_swell.json"),
    include_str!("../parts/flute_note.json"),
    include_str!("../parts/glass_bowl_body.json"),
    include_str!("../parts/glass_pane_body.json"),
    include_str!("../parts/glassware_clink_after.json"),
    include_str!("../parts/highway_air_bed.json"),
    include_str!("../parts/hollow_wood_hit_body.json"),
    include_str!("../parts/large_bowl_clink_after.json"),
    include_str!("../parts/laser_zap.json"),
    include_str!("../parts/latch_click_after.json"),
    include_str!("../parts/liquid_drops.json"),
    include_str!("../parts/long_low_boom.json"),
    include_str!("../parts/low_blunt_crunch_after.json"),
    include_str!("../parts/low_sweep_pre.json"),
    include_str!("../parts/marble_in_glass_after.json"),
    include_str!("../parts/medium_click_body.json"),
    include_str!("../parts/medium_hit_body.json"),
    include_str!("../parts/medium_object_snaps.json"),
    include_str!("../parts/metal_container_body.json"),
    include_str!("../parts/oil_bubbling_bed.json"),
    include_str!("../parts/paper_scrape_pre.json"),
    include_str!("../parts/pitter_patter.json"),
    include_str!("../parts/plastic_bag_rustle.json"),
    include_str!("../parts/plastic_ceramic_hit_body.json"),
    include_str!("../parts/resonant_clink_body.json"),
    include_str!("../parts/ruler_twang.json"),
    include_str!("../parts/sheet_rub_texture.json"),
    include_str!("../parts/siren_wail.json"),
    include_str!("../parts/small_bright_click_crack.json"),
    include_str!("../parts/small_click_crack.json"),
    include_str!("../parts/small_metallic_hit_body.json"),
    include_str!("../parts/small_objects_crunch_texture.json"),
    include_str!("../parts/small_plastic_objects_drop.json"),
    include_str!("../parts/small_plastic_objects_scatter.json"),
    include_str!("../parts/small_plastic_tick.json"),
    include_str!("../parts/snapping_after.json"),
    include_str!("../parts/spark_arcs.json"),
    include_str!("../parts/splash_with_bubbles.json"),
    include_str!("../parts/spring_mechanism_ticking.json"),
    include_str!("../parts/storm_wind_bed.json"),
    include_str!("../parts/string_pluck.json"),
    include_str!("../parts/swipe_and_tap.json"),
    include_str!("../parts/switch_click.json"),
    include_str!("../parts/thin_metal_sheet_body.json"),
    include_str!("../parts/tight_thump.json"),
    include_str!("../parts/tin_foil_hit_body.json"),
    include_str!("../parts/ui_confirm.json"),
    include_str!("../parts/underwater_bubbles.json"),
    include_str!("../parts/utensil_on_plate_after.json"),
    include_str!("../parts/whoosh_sweep.json"),
    include_str!("../parts/windchime_after.json"),
    include_str!("../parts/wood_box_body.json"),
];

/// Every built-in part. A file that does not parse is a bug in the repo
/// (`tests/parts.rs` parses each), so it is skipped here, not a panic.
pub fn builtin() -> Vec<Part> {
    FILES
        .iter()
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect()
}

pub fn find(name: &str) -> Option<Part> {
    builtin().into_iter().find(|p| p.doc.name == name)
}

impl Part {
    /// The catalog entry a search shows for this part.
    pub fn entry(&self) -> CatalogEntry {
        CatalogEntry {
            name: self.doc.name.clone(),
            description: self.description.clone(),
            tags: self.tags.clone(),
            prompt: None,
            kind: self.doc.kind,
            length: 0.0,
            lufs: 0.0,
            macros: self.doc.macros.iter().map(|m| m.name.clone()).collect(),
            role: Some(self.role.clone()),
            material: Some(self.material.clone()),
            approved: true,
            builtin: true,
        }
    }
}
