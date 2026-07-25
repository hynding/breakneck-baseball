//! Validates the committed player.glb against the CLIP_TABLE contract.
//! Pure gltf-crate parsing — no Bevy app, so it runs in milliseconds.

use std::collections::HashSet;

use breakneck_baseball::game::model_assets::{
    ATTACH_BONES, CLIP_TABLE, JERSEY_MATERIAL, MAX_BONES, MAX_GLB_BYTES, MAX_TRIANGLES, PLAYER_GLB,
};

#[test]
fn player_glb_satisfies_contract() {
    let bytes = std::fs::read(PLAYER_GLB).unwrap_or_else(|e| {
        panic!("{PLAYER_GLB} unreadable ({e}) — run tools/build_player.py then tools/export_glb.py")
    });
    assert!(
        bytes.len() <= MAX_GLB_BYTES,
        "player.glb is {} bytes (ceiling {MAX_GLB_BYTES}) — the wasm deploy pays for this",
        bytes.len()
    );

    let (doc, _buffers, _images) =
        gltf::import_slice(&bytes).expect("player.glb failed to parse as glTF");

    // Animation set must EXACTLY match the table (missing → T-pose; extra → dead weight).
    let clip_names: HashSet<&str> = doc.animations().filter_map(|a| a.name()).collect();
    let expected: HashSet<&str> = CLIP_TABLE.iter().map(|(_, name)| *name).collect();
    assert_eq!(
        clip_names, expected,
        "animations must exactly match CLIP_TABLE"
    );

    let material_names: Vec<String> = doc
        .materials()
        .filter_map(|m| m.name().map(str::to_owned))
        .collect();
    assert!(
        material_names.iter().any(|n| n == JERSEY_MATERIAL),
        "missing material {JERSEY_MATERIAL}; found {material_names:?}"
    );

    let skin = doc.skins().next().expect("model must be skinned");
    let bone_names: HashSet<String> = skin
        .joints()
        .filter_map(|j| j.name().map(str::to_owned))
        .collect();
    assert!(
        bone_names.len() <= MAX_BONES,
        "{} bones (budget {MAX_BONES})",
        bone_names.len()
    );
    for bone in ATTACH_BONES {
        assert!(bone_names.contains(*bone), "missing attachment bone {bone}");
    }

    let tris: usize = doc
        .meshes()
        .flat_map(|m| {
            m.primitives()
                .map(|p| p.indices().map_or(0, |i| i.count() / 3))
                .collect::<Vec<_>>()
        })
        .sum();
    assert!(
        tris > 0 && tris <= MAX_TRIANGLES,
        "{tris} triangles (budget {MAX_TRIANGLES})"
    );
}
