# Rex Café

A pixel-art café, drawn in 24-bit half-blocks, where every coding agent on your Rex server is a Clawd. Rex's T-rex tends the bar, and everything on screen is driven by Rex events.

- Agents appear when they report status to Rex with the Program Status Protocol (OSC 7501). Claude Code 2.1.310+ does this on its own.
- A Clawd walks in when its agent starts, fetches coffee at the start of each turn, works at its table, raises a `!` when it needs you, and walks out when the agent exits.
- Clicking a Clawd, or its Legend row, jumps the Rex app to that agent's pane. Clicking the door opens a new agent beside the café.
- It rains outside and lo-fi plays while the café is open. Each drink the T-rex makes sounds like a real one: the grind, knock, and tamp, the pump humming as the shot trickles into the cup, and the steam wand for milk drinks. Cups clink onto saucers, Clawds stir their drinks, and a drip machine and a kettle gurgle and pour in the background. Every 25 seconds to 5 minutes lightning flashes over the city and thunder follows, sooner and louder the closer the strike. All of it is synthesized on the fly.

## Run

```sh
nix run                  # inside a Rex pane at least 140×48; it grows to fill bigger ones
nix run . -- --demo      # made-up agents, no Rex needed
```

| Key | Does |
| --- | --- |
| `tab` / arrows | select a Clawd |
| `enter` | jump to the selected agent's pane |
| `n` | open a new agent (`--spawn-cmd` to change, default `claude`) |
| `m` | mute or unmute (`--quiet` starts muted for this run) |
| `v` | volume menu: master, music, rain (and thunder), and coffee sliders, saved to `~/.config/rex-cafe/settings.json` |
| `r` | re-sync from Rex |
| `q` | quit |

## Settings

The café keeps its settings in `~/.config/rex-cafe/settings.json` (or under `$XDG_CONFIG_HOME`). The volume menu writes the levels; everything else is yours to edit, and is read when the café starts:

```json
{
  "levels": { "master": 1.0, "music": 1.0, "rain": 1.0, "coffee": 1.0 },
  "muted": false,
  "lightning": { "min_secs": 25.0, "max_secs": 300.0 }
}
```

- `lightning` sets the random wait between strikes. It can't go below 2 seconds.
- Missing keys take their defaults, and a file that won't parse is ignored.

## Develop

```sh
nix develop                                        # or `direnv allow` to load it on cd
cargo test                                         # unit + snapshot tests
cargo run -- --dump /tmp/frames --at 0,20,36       # demo frames as .ppm + .txt
cargo run -- --sprites /tmp/sprites                # every sprite, enlarged
cargo run -- --dump-audio /tmp/cafe.wav            # a minute of the café's sound, at your levels
cargo insta review                                 # after an intended art or choreography change
```

- `GLOSSARY.md` defines the café's language: Agent, Clawd, Status, Activity, and the rest.
- `docs/adr/` records why the café talks to Rex over its WebSocket and treats Rex as its only source of truth.
- Sprites are palette-indexed text grids in `assets/sprites/`.
