# DEUSMOBILE

A first-person stealth / action immersive sim set in a rain-soaked cyberpunk harbour, 2071.
Built in Rust with [Bevy 0.16](https://bevyengine.org) - native Windows today, with WebAssembly
(browser) and mobile as possible future targets.

You are a VEIL operative ("Ghost"). Halvorsen-Kade is moving the LAZARUS research core out of
its Port Meridian biolab before dawn. Get in, take the core, reach the extraction point.
Light is the core mechanic: stay in shadow and guards will walk right past you.

## Running

```bash
cargo run --release
```

The first build downloads and compiles Bevy (several minutes). After that, builds are fast.
Run from the project folder so the game finds `assets/` (the level is also embedded as a fallback).

Dev / test flags (`cargo run --release -- <flags>`):

| flag | effect |
|---|---|
| `--start` | skip the menu |
| `--quality low\|medium\|high\|ultra` | graphics preset |
| `--tp X,Z[,YAW,PITCH]` | start at a world position in meters (cell × 2 + 1) |
| `--shot file.png [--shot-delay 5]` | save a screenshot and quit |
| `--flashlight`, `--vision` | start with the flashlight / vision aug on |
| `--panel inv\|skills\|goals\|term\|keypad\|talk\|pause` | open a UI screen at start |
| `--skirmish` | start an offline team match against bots |
| `--host` | host an online team match (TCP port 27777) |
| `--join ADDR` | join an online team match (`192.168.1.20`, `my.host.net:27777`, ...) |
| `--name NAME`, `--team orange\|blue`, `--team-size N` | team match settings |

## Controls

| key | action |
|---|---|
| WASD | move |
| Shift | sprint (loud) |
| C / hold Ctrl | crouch (quiet, harder to see, needed for vents) |
| Space | jump / climb onto crates |
| Q / E | lean |
| F | use / pick up / talk. **Hold** on a locked door to pick the lock |
| Left / right mouse | fire / aim |
| R | reload |
| 1-0, mouse wheel | belt slots (weapons equip, consumables are used) |
| H | quick medkit |
| L | flashlight (makes you much more visible) |
| F1-F6 | augmentations: Vision, Cloak, Speed, Ballistic, Regeneration, Radar Transparency |
| Tab / I | inventory, augmentations, skills, goals, notes |
| Esc | pause / settings |

## Team match: Orange vs Blue (online and offline)

Two teams of stealth operatives on a variant of the harbour map (`assets/levels/harbor_teams.txt`):
**Orange** starts on the south pier, **Blue** in the north-east alley. Each round picks one of two
objectives at random:

- **Steal the prototype.** The prototype appears at one of several possible spots (`steal_spot`).
  Find it, pick it up and carry it into your team's extraction zone (the coloured holo floor and beam
  at your base). Kill the carrier and it drops where they fell; if nobody touches a dropped prototype
  for 30 seconds it is moved to a new random spot.
- **Hack 2 of 3 terminals.** Three terminals appear at random spots (`hack_spot`). Hold **F** on a
  terminal to hack it for your team (about 7 s, faster with the Computer skill; hacking an
  enemy-held terminal takes longer). The first team holding two terminals wins the round.

Objective locations are random every round and are **not marked until someone on your team has
actually seen them** (line of sight, within 15 m). Then they appear on your team's HUD. Light is
still everything: other players are only as visible as the light on them, flashlights give you away,
footsteps are audible (crouch to be silent), the cloak hides you completely from the enemy team, and
a baton strike from behind is a silent takedown. Shoot out lights or use light switches to make
darkness. No friendly fire. Dead players respawn at their base after 5 seconds. A round lasts
8 minutes (on time-out, the team holding more terminals wins, otherwise it is a draw).

- **Offline**: main menu > *PLAY OFFLINE (BOTS)*. Bots fill both teams up to the chosen team size.
  They search the possible objective spots, crawl through vents, open doors, notice you based on how
  lit you are, and fight. They cannot see cloaked players.
- **Host**: main menu > *HOST ONLINE*. Friends join with your IP address. On a LAN that is your
  local IP; over the internet forward **TCP port 27777** on your router (and allow the game through
  the Windows firewall). Bots fill the empty slots and step aside when people join.
- **Join**: type the host's address in the *Join* field (click it, type, Enter) and press
  *JOIN ONLINE GAME*. Everyone must run the same version of the game and of `harbor_teams.txt`.

In the match: **P** shows the scoreboard, **Esc** opens the menu (switch team, leave). Online, the
game never pauses.

## Gameplay systems

- **Stealth driven by light.** Every lamp, street light, flashlight, alarm beacon and camera cone
  feeds a visibility value (the *LIGHT EXPOSURE* meter). Guards build suspicion based on how lit
  you are, distance, movement and crouching. Shoot out lights or use light switches to make darkness.
- **Guards** patrol routes, react to noise (footsteps on metal, gunshots, breaking lights),
  get suspicious, investigate, search, call for help, open doors, notice bodies and lights going
  out, and fight back. Each one carries a red visor, and some carry volumetric flashlights.
- **Non-lethal options.** The stun baton drops an unaware guard from behind instantly. Tranquilizer
  darts put guards to sleep after a few seconds, or instantly with a headshot. Unconscious and dead
  guards can be searched for ammo.
- **Security cameras** sweep with a visible light cone and raise the alarm. **Turrets** go live
  during an alarm (the server room turret is always live). A security terminal can disable
  cameras, switch turrets off, or set them to **target hostiles**.
- **Locks, keypads and hacking.** Use lockpicks and multitools (skill reduces how many you need),
  keycards, keypad codes found in notes or from conversations, and terminal logins found on
  datacubes - or hack terminals and get out before the trace completes.
- **Inventory grid** with a belt, **skills** (Firearms, Melee, Computer, Electronics, Lockpicking,
  Medicine, Athletics) upgraded with points from objectives and notes, and **augmentations**
  powered by bioelectric energy (an augmentation canister lets you install a new one).
- **Aim focus.** The crosshair spreads when you move and tightens while you hold still; the
  Firearms skill makes it settle faster and tighter.
- **Infolink** transmissions from your handler, guard barks as subtitles, branching dialogue
  with an informant, readable datacubes and emails, objectives and an end-of-mission rating.

### Mission routes (spoilers)

There is no single solution. Some of the ways in:

- Warehouse: lockpick the south door, use the vent grate on the south wall, or get the east-door
  keypad code from the informant in the pier shack.
- Office block: the warehouse vent leads up into the south office, or go through the front door.
- Lab: the security office console opens the lab door. Its login is on a datacube, or you can hack
  it. The security office key is in the break room. You can also take the server room (keypad code
  in the warehouse foreman's email) and crawl through its vent into the lab, if you can deal with
  the turret.

## Graphics

HDR rendering with TonyMcMapface tonemapping, bloom, volumetric fog and volumetric light shafts,
shadowed spot lights, SSAO, TAA, wet asphalt with mirror-like puddles, lit rain that is visible
in light beams, emissive signage and screens, night-vision and cloak grading. Pick a preset in
the menu or pause screen (LOW disables the expensive passes).

The volumetric light shafts use a patched copy of Bevy 0.16.1's volumetric fog shader
(`src/shaders/volumetric_fog.wgsl`, registered in `render.rs`). Stock Bevy picks the clustered
light list once per pixel at the depth where the fog ray starts; with the camera inside the fog
that is the nearest few metres, so shafts of lamps further away were missing in tile-shaped
rectangles that flickered as you moved. The patch looks the cluster up at every ray-march step.
Remove the override when upgrading Bevy (or re-apply the patch to the new shader).

All textures are **procedurally generated** at startup (albedo, normal and roughness/metal maps
with mipmaps), and all sounds are synthesized, so the game currently ships no art files.

### Replacing textures with your own art

Drop a PNG into `assets/textures/` named after the surface and it overrides the generated one:

`concrete, asphalt, floortile, carpet, metalfloor, walllab, walloffice, wallmetal, wallext,
wallboundary, ceiling, ceilingmetal, roof, vent, crate, container, counter, rack, trim,
waterbed, door, door_sec, grate, water`

Add `<name>_normal.png` for a normal map (OpenGL convention). Textures should tile; most cover
2 × 2 m of world space (asphalt, boundary walls and water cover 4 m or more; vents and metal
floors 1 m). Only use art you have the rights to ship.

## Editing the level

The mission is the text file `assets/levels/mission01.txt`: an ASCII grid (one character = one
2 m cell), followed by `[meta]` lines (locks, keypad codes, items, terminals, patrol routes, ...)
and `[notes]` (datacube and email texts). The full legend is at the top of `src/level.rs`.
`tools/build_map.py` regenerates the file from row segments and prints a coordinate check.

## Code layout

| file | what |
|---|---|
| `main.rs` | app setup, plugins, dev flags |
| `game.rs` | shared types: states, settings, events, world requests/events, network protocol |
| `net.rs`, `wire.rs` | online play: host-authoritative TCP transport (length-prefixed bincode frames) |
| `teams.rs` | team match: rounds, objectives, discovery, respawns, team HUD, markers, scoreboard |
| `bots.rs` | team-match bots (simulated by the host / offline) |
| `avatars.rs` | other players in the world: animated operatives, hit boxes, flashlights, footsteps |
| `level.rs` | map parsing, cell column model, merged level meshes |
| `collision.rs` | grid DDA raycasts, swept box movement with step-up |
| `materials.rs` | procedural PBR textures, material and mesh library |
| `world.rs` | spawning, doors, lights, pickups, terminals, mission flow, light registry |
| `player.rs` | first-person controller, augmentations, visibility, damage |
| `weapons.rs` | firing, spread, reload, view model |
| `npc.rs` | guards, cameras, turrets, NPC models, A* pathfinding |
| `interact.rs` | use/focus, lockpicking, keypads, terminals, dialogue |
| `hud.rs`, `menu.rs` | HUD and all screens |
| `effects.rs`, `audio.rs`, `render.rs` | particles/rain, synthesized sound, camera and post-processing |

### Networking model

Host-authoritative: gameplay never changes shared state directly. It sends a `WorldRequest`;
the authority (the host, or the local game offline) validates it (`AuthRequest` -> `authority_*` /
`match_requests` systems) and emits `WorldEv` events that every peer applies. Clients send their
own player state at 30 Hz; the host broadcasts everyone's state at 20 Hz, plus every world event,
effect and damage message. Late joiners receive a snapshot of the match (`MatchNet`) and a log of
state-changing events (doors, lights...). Bots are simulated on the host and replicated like
players. The campaign is single-player; the guard / device snapshot types (`GuardNet`,
`DeviceNet`) are kept for a future co-op campaign.

## Roadmap ideas

- Co-op campaign (2-4 players) on top of the team-match networking; revive downed teammates
- More missions, verticality (stairs, catwalks, rooftops), swimming
- Real 3D models and texture sets, voice-acted infolinks
- Browser (WebGPU) and mobile builds with touch controls
