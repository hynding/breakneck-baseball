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
  pitcher. Lengthwise the box is **centred** on home plate's centre, not
  offset toward the pitcher: groundskeeperu.com's field-layout guide puts
  the box's back line 3 ft from the plate's centre, and the box is 6 ft
  long — i.e. the front line sits the same 3 ft ahead, symmetric either
  way. (A pre-research assumption for this task guessed a small forward
  offset toward the pitcher; the sourced groundskeeping template disagrees,
  so the game keeps the box centred — see `field.rs`'s `BOX_HALF_LENGTH`.)
- Foul lines run in chalk from home plate through first and third base and
  on to the outfield fence. Per the Official Baseball Rules (Rule 2.03,
  "the first and third base bags shall be entirely within the infield" —
  i.e. fair territory) and groundskeeperu.com's field-layout guide ("the
  foul edge of the foul line will line up exactly with the foul edge of the
  base"): the bases sit *in* fair territory, and the line's fair-side edge
  runs along the bag's *outer* edge — not through the bag's centre.
- Chalk/paint line width: the Official Baseball Rules (2.01) call for lines
  "not less than two nor more than four inches in width"; groundskeeping
  guides describe the same 2–4 in range in practice, with pro crews often
  striping toward the wider end (~4 in). The game uses **3 in** (0.076 m),
  the middle of that range.

In the game: `field::spawn_bases` uses 0.457 m bags and a 0.43 m plate slab;
the batter rig stands in the right-handed box at x ≈ +0.7 facing −X (the
plate), per `player::spawn_players`. `field::spawn_chalk_lines` paints both
batter's-box outlines and the two foul lines (home → first/third → fence,
direction taken from the live `FieldSpec::base_positions`, not a hardcoded
45°) as flat chalk-white quads on the ground; `field::foul_line_span` offsets
each line perpendicular to that direction by the bag's half-width
(`BASE_HALF_WIDTH`, 0.229 m) so its fair-side edge kisses the bag's outer
edge rather than passing through its centre.

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

## Baserunning after contact (coaching conventions)

How a base runner *breaks off the bat* is one of baseball's oldest read-and-react
skills. The conventions the game encodes (in `rules::runner_break`, driving the
runner-rig choreography — never the call):

1. **Two outs — run on contact.** With two down there is nothing to lose by
   getting caught off base (a caught fly ends the inning anyway), so every
   runner goes immediately on any batted ball; the two-out lead is even
   lengthened for exactly this reason ("in a 2 out, the runner must think about
   an increased distance on the primary lead" — Baseball Training World).
2. **Fewer than two outs, ground ball — forced runners go, unforced runners
   read.** A *forced* runner (one the batter-runner pushes ahead — see the force
   play below) has to advance, so he breaks on contact. An *unforced* runner
   advances only if the ball gets through or past the infield, reading the
   fielders' depth ("knowing when to run home on a ground ball with 0 or 1 outs
   is very difficult, and the depth of the shortstop and second baseman will let
   you know if they are willing to give up a run for the out or not" — QC
   Baseball). In this game's ground-out model every runner advances a base on a
   grounder (`rules::advance_trailing`), so the "read" is the unforced runner
   edging halfway and committing once the ball is down — `rules::landed_past_infield`
   judges "got through or past the infield" against the same infield-gather
   radius the live-throw race already uses for "an out at first is only
   contested on infield balls" (`INFIELD_GATHER_RADIUS`, scaled by the park's
   `hit_scale`); a fair ball that drops but stays inside that radius is read
   like a catch — the runner retreats.
3. **Fewer than two outs, catchable fly — go halfway.** "The tag-up rule is the
   primary reason behind the strategy of base runners advancing halfway on a
   fly ball. Baserunners study the fielder and advance only far enough from the
   base to ensure that they can return safely if the ball is caught" — i.e. edge
   out, continue if it drops, retreat if it is caught. The same
   `rules::landed_past_infield` through-the-infield read applies here too: a
   shallow catchable fly that falls in still sends the runner back.
4. **Fewer than two outs, deep fly — tag up.** On a catch a runner must retouch
   his starting base ("tag up") before advancing; a deep fly gives him the time
   to tag and take the next base — the **sacrifice fly**. "On long fly ball outs,
   runners can often gain a base. On short fly balls, runners rarely attempt to
   advance after tagging up, due to the high risk of being thrown out"
   (Wikipedia, *Tag up*). The deep-fly distance mirrors `TAG_UP_MIN_DIST`, the
   same threshold the sac-fly rule already uses.
5. **The batter always runs on contact** (fair-ball assumption); the engine
   resets him on a foul.

**Force play:** "When a runner is bumped over to the next base by the advancing
batter or by another runner who was bumped by the advancing batter, that runner
is considered to have been forced to advance." The batter-runner always forces
first base, and the force extends up the bases as long as they are continuously
occupied behind the lead runner (`rules::is_forced`). With two outs a force out
ends the inning the instant it is recorded, so no run scores on the play
(Baseball Rules Academy).

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
- [Wikipedia — Tag up](https://en.wikipedia.org/wiki/Tag_up)
- [Baseball Training World — The 9 fundamentals of base running](https://baseballtrainingworld.com/the-9-fundamentals-of-base-running-in-baseball/)
- [QC Baseball — Baserunning: tagging up](http://www.qcbaseball.com/skills/baserunning-tagging-up.aspx)
- [Baseball Rules Academy — 5.06 Running the bases](https://baseballrulesacademy.com/official-rule/mlb/5-06-running-the-bases/)
- [Baseball Rules Academy — 2.01 Layout of the field](https://baseballrulesacademy.com/official-rule/mlb/2-01-layout-field/) (line width, materials)
- [Bat Flip Sports — How to chalk a baseball field like a pro](https://batflipsports.com/how-to-chalk-a-baseball-field/) (2–4 in chalk-line width)
- [Fox Valley Paint — How to stripe a baseball field like a pro](https://foxvalleypaint.com/how-to-stripe-a-baseball-field/) (practical ~4 in foul-line width)
