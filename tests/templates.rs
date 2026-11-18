//! Every `new_sound` template parses, renders, keeps its knobs, and the
//! door slam passes `layers` (impact) like the approved slam it is.

#![allow(clippy::unwrap_used)]

use serde_json::json;
use sfx_analysis::{CheckCx, Expect, run};
use sfx_graph::{RenderOpts, SoundDoc};

#[test]
fn templates_render_and_the_door_slam_passes_layers() {
    for (name, body) in sounder::session::TEMPLATES {
        let doc: SoundDoc = serde_json::from_str(body).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!doc.macros.is_empty(), "{name}: no knobs");
        let opts = RenderOpts::default();
        let r = doc.render(&opts).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(r.peak_db > -30.0, "{name}: silent");
        let e: Expect = serde_json::from_value(json!({"layers": {"preset": "impact"}})).unwrap();
        let res = run(
            &e,
            &CheckCx {
                doc: &doc,
                rendered: &r,
                opts: &opts,
            },
        )
        .unwrap();
        for c in &res {
            println!(
                "{name}: {} {}: {}",
                if c.pass { "PASS" } else { "FAIL" },
                c.name,
                c.detail
            );
        }
        if *name == "door_slam" {
            assert!(res.iter().all(|c| c.pass), "door_slam fails layers");
        }
    }
}
