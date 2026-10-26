# Rex Café

A pixel-art café, drawn in 24-bit half-blocks, where every coding agent on your Rex server is a Clawd. A Rex axolotl tends the bar, and everything on screen is driven by Rex events.

- Agents appear when they report status to Rex with the Program Status Protocol (OSC 7501). Claude Code 2.1.310+ does this on its own.
- A Clawd walks in when its agent starts, fetches coffee at the start of each turn, works at its table, raises a `!` when it needs you, and walks out when the agent exits.
- Clicking a Clawd, or its Legend row, jumps the Rex app to that agent's pane. Clicking the door opens a new agent beside the café.

## Run

```sh
nix run                  # inside a Rex pane at least 140×48
nix run . -- --demo      # made-up agents, no Rex needed
```

| Key | Does |
| --- | --- |
| `tab` / arrows | select a Clawd |
| `enter` | jump to the selected agent's pane |
| `n` | open a new agent (`--spawn-cmd` to change, default `claude`) |
| `r` | re-sync from Rex |
| `q` | quit |

## Develop

```sh
nix develop
cargo test                                         # unit + snapshot tests
cargo run -- --dump /tmp/frames --at 0,20,36       # demo frames as .ppm + .txt
cargo insta review                                 # after an intended art or choreography change
```

- `GLOSSARY.md` defines the café's language: Agent, Clawd, Status, Activity, and the rest.
- `docs/adr/` records why the café talks to Rex over its WebSocket and treats Rex as its only source of truth.
- Sprites are palette-indexed text grids in `assets/sprites/`.
