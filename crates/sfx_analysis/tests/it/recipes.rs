//! The JSON recipes in AGENTS.md and docs/ render as written. A block is tested when the
//! line before its ```json fence is `<!-- recipe -->` (3 s) or
//! `<!-- recipe N -->` (N seconds). An `output` is added when the block has
//! none (`common::render`). Each must render without error, finite and not
//! silent.

use serde_json::Value;

fn recipes() -> Vec<(String, f32, Vec<Value>)> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");
    let mut out = Vec::new();
    for file in ["AGENTS.md", "docs/graph.md", "docs/nodes/README.md"] {
        let text = std::fs::read_to_string(format!("{root}{file}")).unwrap();
        recipes_in(file, &text, &mut out);
    }
    assert!(out.len() >= 20, "only {} recipes found", out.len());
    out
}

fn recipes_in(file: &'static str, text: &str, out: &mut Vec<(String, f32, Vec<Value>)>) {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i].trim();
        if let Some(rest) = l
            .strip_prefix("<!-- recipe")
            .and_then(|r| r.strip_suffix("-->"))
        {
            let seconds = rest.trim().parse::<f32>().unwrap_or(3.0);
            assert_eq!(
                lines.get(i + 1).map(|l| l.trim()),
                Some("```json"),
                "{file} line {}: a recipe marker must be followed by a ```json fence",
                i + 1
            );
            let start = i + 2;
            let end = (start..lines.len())
                .find(|&j| lines[j].trim() == "```")
                .unwrap();
            let body = lines[start..end].join("\n");
            let nodes: Vec<Value> = serde_json::from_str(&body)
                .unwrap_or_else(|e| panic!("{file} line {}: bad JSON: {e}", start + 1));
            out.push((format!("{file} line {}", start + 1), seconds, nodes));
            i = end;
        }
        i += 1;
    }
}

#[test]
fn every_marked_recipe_in_agents_md_renders() {
    let all = recipes();
    assert!(all.len() >= 14, "found only {} recipes", all.len());
    for (at, seconds, nodes) in all {
        let a = std::panic::catch_unwind(|| crate::common::render(&nodes, seconds))
            .unwrap_or_else(|_| panic!("{at}: the recipe does not render"));
        let peak = a
            .channels
            .iter()
            .flatten()
            .fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(
            a.channels.iter().flatten().all(|x| x.is_finite()),
            "{at}: non-finite samples"
        );
        assert!(peak > 1e-4, "{at}: silent (peak {peak})");
        println!("{at}: {seconds} s, peak {peak:.3}");
    }
}
