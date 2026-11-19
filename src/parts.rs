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
    include_str!("../parts/breath_swell_pre.json"),
    include_str!("../parts/button_latch_press.json"),
    include_str!("../parts/crunch_after.json"),
    include_str!("../parts/dead_low_hit_body.json"),
    include_str!("../parts/dense_plastic_hit.json"),
    include_str!("../parts/double_small_click_crack.json"),
    include_str!("../parts/dull_soft_crack.json"),
    include_str!("../parts/fabric_sweep_pre.json"),
    include_str!("../parts/fast_pats_after.json"),
    include_str!("../parts/glass_bowl_body.json"),
    include_str!("../parts/glass_pane_body.json"),
    include_str!("../parts/glassware_clink_after.json"),
    include_str!("../parts/hollow_wood_hit_body.json"),
    include_str!("../parts/large_bowl_clink_after.json"),
    include_str!("../parts/latch_click_after.json"),
    include_str!("../parts/long_low_boom.json"),
    include_str!("../parts/low_blunt_crunch_after.json"),
    include_str!("../parts/low_sweep_pre.json"),
    include_str!("../parts/marble_in_glass_after.json"),
    include_str!("../parts/medium_click_body.json"),
    include_str!("../parts/medium_hit_body.json"),
    include_str!("../parts/metal_container_body.json"),
    include_str!("../parts/paper_scrape_pre.json"),
    include_str!("../parts/plastic_ceramic_hit_body.json"),
    include_str!("../parts/resonant_clink_body.json"),
    include_str!("../parts/small_bright_click_crack.json"),
    include_str!("../parts/small_click_crack.json"),
    include_str!("../parts/small_metallic_hit_body.json"),
    include_str!("../parts/small_plastic_objects_drop.json"),
    include_str!("../parts/small_plastic_objects_scatter.json"),
    include_str!("../parts/small_plastic_tick.json"),
    include_str!("../parts/snapping_after.json"),
    include_str!("../parts/swipe_and_tap.json"),
    include_str!("../parts/switch_click.json"),
    include_str!("../parts/thin_metal_sheet_body.json"),
    include_str!("../parts/tight_thump.json"),
    include_str!("../parts/tin_foil_hit_body.json"),
    include_str!("../parts/ui_confirm.json"),
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
