//! The JSON recipes in AGENTS.md render as written. A block is tested when the
//! line before its ```json fence is `<!-- recipe -->` (3 s) or
//! `<!-- recipe N -->` (N seconds). An `output` is added when the block has
//! none (`common::render`). Each must render without error, finite and not
//! silent.

use serde_json::Value;

fn recipes() -> Vec<(usize, f32, Vec<Value>)> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../AGENTS.md");
    let text = std::fs::read_to_string(path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
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
                "AGENTS.md line {}: a recipe marker must be followed by a ```json fence",
                i + 1
            );
            let start = i + 2;
            let end = (start..lines.len())
                .find(|&j| lines[j].trim() == "```")
                .unwrap();
            let body = lines[start..end].join("\n");
            let nodes: Vec<Value> = serde_json::from_str(&body)
                .unwrap_or_else(|e| panic!("AGENTS.md line {}: bad JSON: {e}", start + 1));
            out.push((start + 1, seconds, nodes));
            i = end;
        }
        i += 1;
    }
    out
}

#[test]
fn every_marked_recipe_in_agents_md_renders() {
    let all = recipes();
    assert!(all.len() >= 14, "found only {} recipes", all.len());
    for (line, seconds, nodes) in all {
        let a = std::panic::catch_unwind(|| crate::common::render(&nodes, seconds))
            .unwrap_or_else(|_| panic!("AGENTS.md line {line}: the recipe does not render"));
        let peak = a
            .channels
            .iter()
            .flatten()
            .fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(
            a.channels.iter().flatten().all(|x| x.is_finite()),
            "AGENTS.md line {line}: non-finite samples"
        );
        assert!(peak > 1e-4, "AGENTS.md line {line}: silent (peak {peak})");
        println!("AGENTS.md line {line}: {seconds} s, peak {peak:.3}");
    }
}
