# Bambietta Server

A **Minecraft Java Edition 1.20.4** (protocol 765) server written in Rust, inspired by
[PrismarineJS/flying-squid](https://github.com/PrismarineJS/flying-squid). It serves vanilla clients
over TCP and web clients over **WebSocket** (built for Whitefox Client 2) on the **same port**. It
also answers the **UDP Query** protocol.

## Downloads

Prebuilt binaries for **Windows x64** and **Termux/Android** (aarch64, statically linked with musl)
are attached to each release on the [GitHub Releases](../../releases) page. To build from source,
see [Usage](#usage) and [Termux (Android)](#termux-android).

## Usage

```sh
cargo run --release              # uses ./config.json (created if missing)
cargo run --release -- other.json
```

- Vanilla 1.20.4 client: *Multiplayer → Add Server → `localhost`*.
- Web client: `new WebSocket('ws://localhost:25565')`. The protocol contract is in
  [docs/WEBSOCKET.md](docs/WEBSOCKET.md) and there is a browser example in
  [examples/ws-status.html](examples/ws-status.html).

Logging: set `RUST_LOG=debug` to also see packets that are ignored.

## Features

| Area | Status |
|---|---|
| Transport | TCP, `ws://` and `wss://` (TLS) on the same port with automatic detection, HTTP(S) status page, optional WebSocket-only port, zlib compression, UDP Query (GameSpy4) |
| Protection | Per-IP connection limits (concurrent and per 10 s), kick on packet flooding, chat anti-spam (ops exempt) |
| Login | Offline mode with vanilla UUIDs, 1.20.2+ configuration phase with the official 1.20.4 registries and tags (water, ladders, etc. behave correctly in the vanilla client) |
| Server list | MOTD, version, online players and a sample of names |
| World generation | ~24 climate-based biomes (oceans, beaches, rivers, plains, forests, taiga, desert, savanna, jungle, swamp, hills, snowy peaks...) with their own surface and vegetation; caves (3D-noise caverns and tunnels, lava at the bottom), height-based ores, deepslate and stone variants; per-biome trees (oak, birch, spruce, dark oak, jungle, acacia, swamp) that cross chunk borders; villages with streets, a well, houses, farms and lamp posts. `flat` is also available |
| Structures | Dungeons (mossy room with a zombie, skeleton or spider spawner and loot chests), strongholds (9 in two rings like vanilla: corridors, crossing, library, storeroom, cells and the End portal room with its 12 frames, lava and a silverfish spawner), desert pyramids (hidden chamber with 4 chests and a TNT trap), pillager outposts (dark oak tower with a lookout chest; pillagers keep guarding it) and Nether fortresses (bridges on pillars, blaze spawner platforms, nether wart garden, chests; blazes and wither skeletons spawn only there). Chests use simplified vanilla loot tables; spawners work like vanilla (player within 16 blocks, up to 4 mobs every 10-40 s). Eyes of ender fly towards the nearest stronghold. `/locate stronghold\|dungeon\|village\|pyramid\|outpost\|fortress` |
| Lighting | Sky and block light computed per column and completed with light coming in from the four neighbouring chunks; no skylight in the Nether and the End |
| Dimensions | Overworld, Nether (netherrack, lava sea, quartz, gold, glowstone, soul sand valleys) and End (central island, obsidian pillars topped with bedrock, outer islands, exit portal). Faster lava and evaporating water in the Nether, beds explode outside the Overworld |
| Ender Dragon | Spawns on the first visit to the End with a crystal on top of each pillar; circles, charges, shoots dragon fireballs, perches on the portal, the head bites and the wings knock back; crystals heal it and explode when hit; boss bar; on death (10 s animation, 12000 experience) the exit portal activates and the egg appears. The state is saved in `level.dat` (`DragonFight`) |
| Portals | Nether portals with frames from 2x3 up to 21x21 lit with flint and steel, travel after 4 s (coordinates /8), finds or builds the destination portal; End portal with 12 frames and eyes of ender; End arrival platform; breaking the frame turns the portal off |
| Redstone | Levers, buttons, pressure plates (including weighted), redstone block, torches (inverter), repeaters with delay, 0-15 dust with connections, strong and weak power like vanilla; lamps, doors/trapdoors/fence gates, powered rails, note blocks, pistons (normal and sticky, up to 12 blocks) and TNT (with chain explosions) |
| World format | **Anvil**, same as vanilla 1.20.4: `region/*.mca`, `DIM-1/region`, `DIM1/region`, `level.dat` (with chests, furnaces, brewing stands, signs, banners, biomes and dragon state), maps in `data/`. Worlds can be opened with the vanilla server or with MCA Selector/Amulet, and Bambietta opens vanilla worlds. Players in `playerdata/*.json`, mobs in `mobs.json`; autosave every 60 s and on shutdown; worlds in the old format are migrated automatically |
| Players | See each other (spawning, movement, head, sneaking, sprinting, arm swing, held item), tab list with ping and game mode, tab header/footer |
| Visibility | Each client only receives nearby entities (players within its view distance, mobs within 80 blocks, items/orbs/arrows within 64); they spawn and despawn as they enter or leave range |
| Beds | Sleep at night (not with monsters nearby), skip the night when everyone is asleep, respawn point next to the bed |
| Blocks | Placement with orientation (axes, `facing`, stairs, double slabs, wall torches/signs, sign rotation), 2-block doors and beds, opening doors/trapdoors/fence gates, levers, spawn eggs |
| Signs and books | Signs (standing and hanging) that open the editor when placed, front and back text, dyes, glow ink and wax (locks editing); the text is stored in the chunk and visible to everyone. Book and quill (save pages and sign as a written book) and reading written books |
| Workstations | Brewing stand (all vanilla recipes, blaze powder, 20 s, gunpowder and dragon's breath), anvil (repair with material or another item, combine enchantments with their incompatibilities and rarity-based costs, rename, "Too Expensive!" at 40 levels, accumulated repair cost, anvil damage), grindstone (removes non-curse enchantments and returns experience), smithing table (netherite upgrades and armor trims), stonecutter (all 201 recipes, in the client's order), loom (patterns with and without a banner pattern item, up to 6 layers) and cartography table (zoom out, copy and lock maps) |
| Potions and effects | Drinkable, splash and lingering potions (effect depends on distance; healing/harming are inverted for undead mobs), milk, honey, golden apples and spoiled food. Speed, Slowness (with client attributes), Strength, Weakness, Resistance, Fire Resistance, Water Breathing, Invisibility, Regeneration, Poison, Wither, Hunger, Absorption, Jump Boost and Slow Falling (also accounted for in the anti-cheat). Saved with the player; `/effect` |
| Maps | Empty maps turn into centered maps like vanilla, drawn around whoever holds them with colors and height shading, player marker; saved in `data/map_<id>.dat` |
| Vehicles | Boats of every wood type (including chest boats and bamboo rafts): place them on water, board, steer (the client sends `vehicle_move`), paddles, dismount with shift and break them by hitting. Minecarts on rails with vanilla physics: straights, curves, slopes, powered rails that accelerate or brake, and pushing in the look direction. Saved in `vehicles.json` |
| Fire | Spreads and burns blocks with vanilla flammability, ages and burns out on its own; eternal on netherrack (and on bedrock in the End); lava ignites nearby flammable blocks; players and mobs catch fire (water puts it out) and it can be punched out |
| Survival | Validated vanilla break times (tool, Efficiency, hardness, harvestability), drops from `blockLoot` with Silk Touch and Fortune, placing consumes from the inventory, maximum reach |
| Tools and armor | Vanilla durability (`Damage` in NBT) with Unbreaking, break animation, armor points and toughness, Protection/Feather Falling/Blast/Projectile/Fire Protection, Mending |
| Enchanting | Enchanting table with bookshelves and the vanilla algorithm (costs, weights, incompatibilities, books), Sharpness/Smite/Bane of Arthropods, Knockback, Fire Aspect, Looting, `/enchant` |
| Experience | Levels and bar with the vanilla formula, orbs with physics and attraction, experience from ores, mobs and furnaces, lost on death |
| Inventory | Server-authoritative with vanilla `window_click` semantics (clicks, shift, number keys, Q, drags, double click, creative clone), armor only in its slot, `stateId` resync like vanilla, saved per player |
| Crafting | 2x2 grid and 3x3 crafting table with all 1485 recipes of 1.20.4 (including tag-based and mirrored variants), returns buckets/bottles |
| Chests and furnaces | Chests (single and double, merged when placed next to each other), trapped chests and barrels shared between players (animated lid), furnaces with fuels, vanilla smelting (10 s per item), lit block state and experience on extraction; contents are stored in the chunk and dropped when broken |
| Fluids | Flowing water and lava (levels, falling, search for the nearest drop, infinite water sources, drying up without a source), obsidian/cobblestone/stone when they mix, buckets to pick up and pour |
| Mobs | Cow, pig, sheep, chicken, zombie, skeleton (arrows), creeper (explosion with crater) and spider; wandering, fleeing, chasing and attacking AI; burn in sunlight, fall damage, drops from `entityLoot` (cooked if they die burning), milking and shearing, sounds; natural spawning of monsters at night and animals on grass; saved in `world/mobs.json` |
| Mob AI | Monsters chasing a player follow an A* path over blocks (walk, step up one block, drop up to three or any height into water, swim, climb ladders and vines, never over fences or walls, avoiding lava, fire and cactus), with shortcuts on open ground and re-planning when stuck, so they go around walls instead of pushing into them; mobs only jump what they can actually clear and walk along anything taller. Skeletons shoot only with line of sight, draw the bow first, keep their distance, circle around the player and never shoot into a raised shield. On hard they also back off when the player closes in, move round to the side a shield does not cover, and against a player holding a bow they take cover and only step out to shoot. Monsters only notice a player they can see (line of sight), follow it up to 32 blocks once noticed, remember it for about 6 s after losing sight of it and then go and look where they last saw it; with nobody around they patrol. Zombies notice players from 32 blocks and break wooden doors in their way (12 s on hard, 18 s on normal, 24 s on easy for one zombie; several at the same door break it faster; iron doors resist). Spiders are calm in the light unless hit. Land mobs keep out of the water and swim back to the bank if they fall in. Skeletons, pillagers, vindicators and creepers keep away from hazards (lava edges, stalactites, stalagmites, big drops) both when planning a path and when dodging. Creepers swim only when there is no dry way, go round to the player's back before lighting the fuse, run from cats and ocelots, and blow up wolves too. Skeletons run from wolves; wolves (wild or tamed) hunt skeletons above all and bite them for double damage; cats hunt creepers, stray cats fight each other, wild wolves go for cats and anything attacked by another mob fights back. Shortly before sunrise zombies and skeletons look for shade (trees, roofs, caves) and stay there during the day, still attacking whoever comes close |
| Pillager patrols | A captain with the ominous banner and its escort (`/summon pillager patrulla`, or natural spawning near players in the Overworld). The captain leads the patrol and the rest follow in formation; when one of them spots a player the whole patrol attacks: two guards stand between the captain and the player, the others flank it from the sides at crossbow range, the captain stays back, and nobody shoots with an ally in the line of fire. Crossbows load visibly. They take turns to shoot depending on the difficulty (one at a time on easy, two on normal, all of them on hard) and on hard they use the skeletons' tactics (the guards stay with the captain). Each patrol brings a vindicator (iron axe) with a pillager escort: with nobody in sight it breaks down the wooden door of nearby buildings and goes in to search them; in a fight it charges through doors. Killing the captain drops the banner and gives Bad Omen, and the patrol falls apart: each pillager fights on its own like a skeleton |
| Weather | Rain and thunderstorms that come and go like vanilla (saved in `level.dat`, `/weather clear\|rain\|thunder [seconds]`); lightning during storms (damage and fire, charges creepers, turns pigs into zombified piglins; `/summon lightning_bolt`); rain puts out fires and burning players and mobs, and undead do not burn in it; snow piles up and water freezes in cold biomes; sleeping is allowed in a thunderstorm and clears the weather; monsters spawn during storms |
| Farming | Hoe on grass or dirt makes farmland, moist near water or in the rain, dry farmland turns back to dirt and jumping on it tramples it. Random ticks (3 per section and tick around players) grow wheat, carrots, potatoes, beetroot, pumpkin and melon stems (and their fruit), saplings into trees, sugar cane, cactus, sweet berries and nether wart; bone meal speeds them up and sprouts grass and flowers; plants can only be placed where they can live |
| Animals | Breeding with their food (wheat, carrots/potatoes/beetroot, seeds): hearts, a partner, a baby that grows up in 20 minutes (faster if fed), experience; animals follow a player holding their food. Wolves (forests and taigas): tamed with bones, they sit or stand when used, follow their owner (teleporting when left behind), fight whatever the owner hits or whatever hurts the owner, heal with meat; wild ones defend their pack. Cats (villages; tamed with raw cod or salmon) sit, follow or stay by the nearest bed within 20 blocks, and climb walls; ocelots (jungles) run from players until fed fish |
| Nether and End mobs | Zombified piglin (neutral, gets angry as a group), piglin (attacks unless you wear gold armor; bartering with gold ingots using the vanilla table), blaze (flies and shoots bursts of 3 fireballs), ghast (explosive fireball that can be punched back), magma cube (jumps and splits on death), wither skeleton (Wither effect), hoglin (launches you into the air), strider (walks on lava), enderman (gets angry when looked at, teleports, dodges projectiles and hates water) and endermite. Silverfish and cave spiders (poison). Natural spawning in the Nether based on the ground (blazes and wither skeletons only in fortresses), endermen in the End and at night in the Overworld |
| Villagers | 13 professions with the vanilla trades per level (random enchanted books), trading window, experience and level-ups (novice to master), restocking; they appear the first time someone approaches a generated village |
| Projectiles | Arrows, fireballs (blaze, ghast, dragon), splash potions, ender pearls (teleport with damage and endermites) and bottles o' enchanting; they hit players and mobs |
| Dropped items | Dropping (Q, outside the inventory, on death), vanilla physics (gravity, friction, bounce), stack merging, pickup delay, despawn after 5 min |
| Health and hunger | Fall damage (with ground computed on the server; slime, hay and beds cushion falls), void, lava, drowning, vanilla hunger/saturation/exhaustion and regeneration, eating (32 ticks), death with message, respawn |
| PvP | Weapon damage with enchantments, critical hits, knockback, 10 ticks of invulnerability, weapon wear |
| Ranged combat | Bow (power based on draw time, crits, Power, Punch, Flame, Infinity), crossbow (load and fire, Quick Charge, fanned Multishot, Piercing), normal, tipped (1/8 of the effect) and spectral arrows; arrows stick and can be picked up; trident (throw, sticks and can be picked up, Loyalty brings it back, Riptide propels you in water). Vanilla damage: speed x base damage |
| Shield | Blocks everything coming from the front (hits, arrows, fireballs, explosions) after being raised for 5 ticks, wears down, knocks back attackers; an axe hit disables it for 5 s |
| Fishing | Casting the hook, bobbing, bites with bubbles and splash (Lure makes them sooner), catching fish, junk or treasure with the vanilla tables (Luck of the Sea improves treasure, open water only), experience, hooking players and mobs and reeling them in, rod wear |
| Anti-cheat | Speed, jumps over 10 blocks, flying/hovering, passing through blocks (real collision shapes), fake on-ground to avoid damage, fast breaking, reach; corrects with a teleport and waits for confirmation |
| Chat and commands | `/help /list /msg /tell /me /spawn /ping /where /gamemode /gm /tp /time /say /kick /give /clear /kill /heal /food /enchant /xp /effect /summon /killmobs /setblock /fill /dimension /save-all` (`/setblock` and `/fill` accept `block[prop=value,...]`; `/give` accepts NBT, e.g. `/give splash_potion{Potion:"minecraft:healing"}`; `/summon` accepts mobs, boats and minecarts, `/summon villager librarian`, `/summon magma_cube 4`, `/summon pillager patrulla`, `/summon lightning_bolt`) |
| Other | Day/night cycle, keep-alive with timeout kick; a log warning if a tick takes longer than 100 ms (with the part that was slow); saving writes files without freezing the game |

Out of scope for now: online mode (encryption + Mojang authentication), smoker and blast furnace,
comparators, observers, hoppers and dispensers; mineshafts, jungle temples, ocean monuments, bastions, end cities and other structures.

## Termux (Android)

The binary is statically linked for Linux ARM64, so it runs as-is on Termux:

```sh
# On Windows/Linux, with Zig and cargo-zigbuild:
rustup target add aarch64-unknown-linux-musl
cargo install cargo-zigbuild
cargo zigbuild --release --target aarch64-unknown-linux-musl
# On the phone (copy the binary into Termux):
chmod +x bambietta-server && ./bambietta-server
```

On first run it creates `config.json` and the world folder in the directory it is run from. The
game data is embedded in the executable.

## Configuration (`config.json`)

| Key | Default | Description |
|---|---|---|
| `motd` | — | Server list text (supports `§`) |
| `host` / `port` | `0.0.0.0` / `25565` | TCP + WebSocket listener |
| `ws-port` | `null` | Additional WebSocket-only port |
| `ws-allowed-origins` | `[]` | Allowed WebSocket origins (empty = all) |
| `max-players` | `20` | |
| `gameMode` | `1` | 0 survival, 1 creative, 2 adventure, 3 spectator |
| `difficulty` | `1` | |
| `view-distance` | `8` | Maximum radius in chunks (the client may request less) |
| `generation` | `noise` | `{ "name": "noise" \| "flat", "options": { "seed", "layers" } }` (only affects the Overworld) |
| `worldFolder` | `world` | World folder (Anvil format; a vanilla 1.20.4 world works) |
| `compression-threshold` | `256` | `-1` disables it |
| `ws-compression-threshold` | `null` | Threshold for WebSocket only |
| `kickTimeout` | `30000` | ms without keep-alive before kicking |
| `everybody-op` | `true` | Everyone can use admin commands |
| `anti-cheat` | `true` | Movement validation (speed, flying, noclip) |
| `spawn-monsters` / `spawn-animals` | `true` / `true` | Natural mob spawning |
| `enable-query` / `query-port` | `true` / main port | UDP Query |
| `online-mode` | `false` | Not supported; a warning is logged and offline mode is used |
| `tls-cert` / `tls-key` | `null` | PEM certificate and key for `wss://` on the same port |
| `tls-self-signed` | `false` | Generates a self-signed certificate in `tls/` (development only) |
| `tls-hostnames` | `[]` | Extra names/IPs for the self-signed certificate |
| `max-connections-per-ip` | `5` | Concurrent connections per IP (localhost is exempt; 0 = no limit) |
| `connections-per-ip-per-10s` | `15` | New connections per IP every 10 s |
| `max-packets-per-second` | `500` | Packets per second before kicking (0 = no limit) |

## Project layout

```
src/
  net/          TCP/WebSocket transport, framing and compression
  protocol/     VarInt, positions, NBT, 1.20.4 packet ids
  server/       session (handshake/status/login/config), play, blocks, inventory, items,
                health/hunger, effects, experience, enchanting, containers, brewing stand,
                workstations, signs and books, maps, fluids, fire, mobs, villagers, dragon,
                projectiles, vehicles, explosions, sounds, beds, portals, redstone, entity
                tracking, anti-cheat, player data, commands, UDP query
  world/        dimensions, chunks (biomes, light with neighbours), generators (Overworld with
                biomes/caves/trees/villages, Nether, End), Anvil, collisions
  data.rs       embedded registries (blocks, items, biomes, entities)
  gameplay.rs   survival data (food, tools, collisions, loot, recipes, enchantments, armor,
                furnaces)
  item.rs       item stacks and the Slot format
data/           generated from minecraft-data with tools/gen_data.py (and stations.json with
                tools/gen_stations.py from the vanilla server: stonecutter, smithing, loom,
                piglin bartering)
tools/smoke/    smoke tests with mineflayer (TCP + WebSocket) and a UDP Query client
```

To regenerate `data/`:

```sh
git clone https://github.com/PrismarineJS/minecraft-data
python tools/gen_data.py minecraft-data/data
# Official tags: data generator of the vanilla 1.20.4 server.jar
java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --server --reports --output gen
python tools/gen_tags.py gen
python tools/gen_stations.py gen/data/minecraft
```

The ids of blocks (all 26,644 states), items, entities, block entities, menus and particles were
verified against the reports of the official 1.20.4 server.

### Vanilla compatibility

The world format was tested in both directions with the official 1.20.4 `server.jar`:

- a world created by Bambietta (platforms, active Nether and End portals) opens in vanilla without
  errors and the blocks are where they should be (`vcheck.mjs` connected to the vanilla server);
- a world generated and saved by vanilla opens in Bambietta with identical blocks (726/726 samples
  with `vsample.mjs` on both servers).

## Tests

```sh
cargo test
cd tools/smoke && npm install
node smoke.mjs 127.0.0.1 25565     # TCP bot + WebSocket bot: status, chunks, chat, /tp, place and break
node survival.mjs 127.0.0.1 25565  # survival (requires "gameMode": 0): drops, crafting, food, falling, PvP, death, anti-cheat
node movement.mjs 127.0.0.1 25565  # legitimate movement: the anti-cheat must not correct anything
node vanilla.mjs 127.0.0.1 25565   # durability, enchanting, experience, (double) chests, bed, visibility, furnaces, water and mobs
node tls.mjs 127.0.0.1 25565       # wss:// + ws:// + TCP on the same port and flood kick (requires TLS)
node mobsave.mjs set|check ...     # mob persistence (set, restart the server, check)
node world.mjs 127.0.0.1 25565 <world folder>   # biomes, Nether and End portals, dimensions and Anvil files
node redstone.mjs 127.0.0.1 25565  # lever, dust, repeater, inverter, button, pistons and TNT
node ranged.mjs 127.0.0.1 25565    # bow, crossbow, trident, shield and fishing
node walls.mjs 127.0.0.1 25565     # mobs against walls and steps, zombie pathfinding around a wall
node perception.mjs 127.0.0.1 25565 # line of sight, spiders and light, chasing, patrolling, water
node doors.mjs 127.0.0.1 25565     # zombies breaking doors (requires "difficulty": 3)
node structures.mjs 127.0.0.1 25565 # dungeons, strongholds and eyes of ender, pyramids, outposts, fortresses (noise world)
node ecology.mjs 127.0.0.1 25565   # creepers from behind, hazards, wolves vs skeletons, cats vs creepers, cats by beds
node undead.mjs 127.0.0.1 25565    # undead looking for shade before sunrise, skeleton shooting
node pillagers.mjs 127.0.0.1 25565 # pillager patrol: formation, roles in combat, banner and Bad Omen
node skeletons.mjs 127.0.0.1 25565 # skeleton tactics on hard (requires "difficulty": 3)
node weather.mjs 127.0.0.1 25565   # rain, thunderstorms, lightning, rain putting out fire, snow
node farming.mjs 127.0.0.1 25565   # hoe, seeds, bone meal, harvest, saplings, trampling, growth
node animals.mjs 127.0.0.1 25565   # breeding, babies, following food, taming wolves
node features.mjs 127.0.0.1 25565  # potions, workstations, maps, signs, books, boats, minecarts,
                                   # fire, Nether/End mobs, villagers, piglins and the dragon (~8 min)
node bench.mjs 127.0.0.1 25565     # load time of the spawn area and of a new area
node vcheck.mjs / vsample.mjs      # cross-check against the vanilla server (see above)
node query.mjs 127.0.0.1 25565     # UDP Query
```
