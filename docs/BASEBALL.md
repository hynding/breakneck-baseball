# Baseball research notes

The reference document for real-world baseball facts the game models. When a
feature needs a rule, a dimension, or a convention, check here first; if the
answer isn't covered in enough detail, research it online and **add it here**
with the source. Code that models a fact from this document should say so in
a comment ("per docs/BASEBALL.md").

All game units are metres (1 Bevy unit ≈ 1 m); imperial originals are kept
alongside because that's how the sources state them.

## Field geometry

| Fact | Real | Metric | In the game |
|---|---|---|---|
| Base path (base to base) | 90 ft | 27.43 m | `field::BASE_DISTANCE` |
| Home → pitching rubber | 60 ft 6 in | 18.44 m | `field::PITCH_DISTANCE` |
| Home → second (diamond diagonal) | 127 ft 3 3/8 in | 38.79 m | `variant.rs` base positions |
| Foul lines to fence | ~330 ft | ~100.6 m | `FieldSpec::fence_line` |
| Straightaway centre to fence | ~400 ft | ~122 m | `FieldSpec::fence_center` |
| Foul-pole height | varies, ~45 ft+ | 15 m | `field::spawn_foul_poles` |

## The mound (dimensions.com, umpirebible.com)

- Overall: **18 ft diameter** (5.49 m, radius 2.74 m), centre 59 ft from the
  rear point of home plate.
- Height: rubber elevated **10 in** (0.254 m) above the field.
- Pitching rubber: **24 in × 6 in** (0.61 m × 0.152 m), its front edge
  exactly 60 ft 6 in from the rear point of the plate.
- Level table at the top: 60 in × 34 in (1.52 m × 0.86 m).
- Slope: starting 6 in in front of the rubber, down **1 in per foot** for at
  least 6 ft toward the plate.

In the game: `field::spawn_stadium_mound` builds the 2.74 m-radius mound at
0.25 m height with a shallow lower skirt approximating the slope, topped by
the white 0.61 × 0.152 m rubber.

## Bases and the batter's box (mlb.com base-sizes glossary, baseball-reference)

- Since the 2023 rule change bases are **18 in square** (0.457 m), up from
  15 in — "the pizza box". First/third sit inside the corners of the square;
  second base's centre sits on the corner point.
- Bases are white, ~3–5 in tall in practice (bags).
- Home plate: five-sided, **17 in wide** (0.432 m) across the front, sides
  8.5 in, pointed back extending toward the catcher.
- Batter's boxes: **4 ft × 6 ft** (1.22 × 1.83 m), drawn 6 in off each side
  of the plate; the batter stands *facing the plate*, side-on to the
  pitcher.

In the game: `field::spawn_bases` uses 0.457 m bags and a 0.43 m plate slab;
the batter rig stands in the right-handed box at x ≈ +0.7 facing −X (the
plate), per `player::spawn_players`.

## Dirt, grass, and mowing (turface.com, mightygrass.com)

- The **grass line** (where infield dirt meets outfield grass) is an arc of
  roughly **95 ft radius from the centre of the mound** in pro parks.
- Around each base, fields cut a **sliding pit / cutout of ~13 ft radius**
  (3.96 m) out of the infield grass.
- The home-plate circle is the same ~13 ft radius (26 ft diameter) of dirt.
- On-deck circles: 5 ft diameter.
- Outfield grass is mowed in alternating light/dark **stripes** (the lawn
  "checkerboard"/banding look comes purely from bending the blades in
  opposite directions).

In the game: the stadium ground uses a procedurally generated striped-grass
texture and the infield diamond / mound / cutout circles use a speckled dirt
texture (`field.rs`, runtime images — no asset files, same philosophy as the
procedural audio and jerseys).

## People and presentation

- Umpire crew: 4 in the majors — plate, first, second, third. The **plate
  umpire crouches directly behind the catcher**, peering over his shoulder.
- The classic centre-field "pitcher cam" is TV; the *catcher's-eye* view is
  low, just over the catcher's helmet, looking out at the pitcher with the
  strike zone in the bottom of frame — the game's duel view.
- Jerseys: surname arched over the number on the back; number repeated on
  the chest and sleeves/shoulders.
- Batting order: 9 slots, rotating; a substituted player's replacement bats
  in the same slot.

## Rules modeled elsewhere

Count thresholds (4 balls / 3 strikes / 3 outs), the strike zone, tag-ups,
double plays, hit-by-pitch, dropped third strike, steals, leadoffs and
pickoffs are modeled deterministically in `src/game/rules.rs` — see
CLAUDE.md's architecture notes for how each maps.

## Sources

- [Dimensions.com — Baseball pitcher's mound](https://www.dimensions.com/element/baseball-pitchers-mound)
- [UmpireBible — Pitcher's mound & field dimensions](https://www.umpirebible.com/index.php/rules-pitching/pitcher-s-mound-field-dimensions)
- [MLB.com — Base sizes (2023 rule change)](https://www.mlb.com/glossary/rules/base-sizes)
- [MLB.com — 2023 rule changes](https://www.mlb.com/news/mlb-2023-rule-changes-pitch-timer-larger-bases-shifts)
- [Baseball-Reference Bullpen — Batter's box](https://www.baseball-reference.com/bullpen/Batter's_box)
- [Turface Athletics — Field layouts & dimensions](https://www.turface.com/education/resource-library/baseball-softball-field-layouts-dimensions)
- [Mighty Grass — Baseball field dimensions guide](https://www.mightygrass.com/baseball-field-dimensions-guide/)
