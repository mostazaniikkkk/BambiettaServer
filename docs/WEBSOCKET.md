# Bambietta WebSocket transport (contract for Whitefox Client 2)

Bambietta speaks the unmodified **Minecraft Java 1.20.4 (protocol 765)** protocol.
WebSocket only replaces the TCP socket: the bytes are exactly the same.

## Connecting

- URL: `ws://<host>:<port>/`. The main port (`25565` by default) accepts **vanilla TCP and
  WebSocket at the same time**: the server looks at the first bytes and, if they are an HTTP
  `GET `, performs the upgrade. If `ws-port` is configured, that extra port accepts WebSocket only.
- The path does not matter.
- Subprotocol: optional. If the client requests one (`new WebSocket(url, ['minecraft'])`), the
  server confirms the first one in the list, so the browser does not abort the connection.
- `Origin`: all origins are accepted unless `ws-allowed-origins` in `config.json` has entries.
- Use `ws.binaryType = 'arraybuffer'`. Text messages are ignored.

## Framing

Binary messages form a **byte stream**, just like TCP:

```
stream = frame*
frame  = VarInt(length) + data
```

- **Client → server:** you may send one frame per message, several frames together, or a frame
  split across several messages. The server reassembles them.
- **Server → client:** each message carries **one or more complete frames** (the server batches
  whatever it has queued, for example a batch of chunks). The client should still treat it as a
  stream and extract frames with an accumulating buffer (see `examples/ws-status.html`).

## Compression

Same as vanilla: after `login/compress` (`0x03`) frames become
`VarInt(length) + VarInt(uncompressed length | 0) + data (zlib if > 0)`.

- `compression-threshold` (default `256`) applies to both TCP and WebSocket.
- `ws-compression-threshold` overrides it for WebSocket only. With `-1` the server
  **does not send** `compress` and everything travels uncompressed, which is the easiest way to
  get a client started.
- In the browser, zlib can be decompressed with `new DecompressionStream('deflate')` (zlib
  format) or with pako.

## Authentication and encryption

- **Offline** mode only: there is no `encryption_begin`. The UUID is the vanilla offline one
  (`MD5("OfflinePlayer:" + name)`, version 3).
- **`wss://` on the same port.** The server recognizes the TLS handshake and negotiates it itself
  (rustls). With `tls-cert`/`tls-key` (PEM, for example from Let's Encrypt) it is ready for
  production. With `"tls-self-signed": true` it generates a self-signed certificate in `tls/`. For
  development, open `https://host:port` in the browser once, accept the certificate warning, and
  from then on `wss://` works. That URL returns a status page, and so does `http://host:port`.
- If you prefer a proxy in front (Caddy, nginx, Cloudflare Tunnel), plain `ws://` still works.
- If an `https://` page tries to open `ws://`, the browser blocks it (mixed content), so production
  deployments must use `wss://`.

## Login flow expected by the server

1. `handshake` (`0x00`): protocol **765**, next state `2`. With any other version the login is
   rejected with a message.
2. `login_start` (`0x00`): name (`[A-Za-z0-9_]{1,16}`) and UUID (ignored).
3. ← `compress` (if enabled) and ← `login_success`.
4. → `login_acknowledged` (`0x03`).
5. Configuration: ← `registry_data`, ← `feature_flags`, ← `tags` (official tags), ← `custom_payload` (brand),
   ← `finish_configuration`; → `finish_configuration` (`0x02`).
6. Play: ← `login`, `abilities`, `update_health`, `declare_commands`, `player_info`,
   `spawn_position`, `update_time`, `game_event 13` (start waiting for chunks),
   `set_center_chunk`, batches of `chunk_batch_start` / `map_chunk`* / `chunk_batch_finished`, and
   finally `synchronize_player_position`. The client must reply with `teleport_confirm` and answer
   every `keep_alive` (if they do not arrive within `kickTimeout` ms, the server kicks the client).

Text components (chat, kick, tab) arrive as **network NBT** (1.20.3+), in a `{text: "..."}`
compound that may contain `§` color codes.

Chunks are sent about 50 ms after `login`, as in vanilla. The client must have its world ready
when it processes `login`.

## Survival: what the server validates

- **Authoritative inventory.** In `window_click` the client must send the `stateId` of the last
  `window_items`/`set_slot` it received, plus the `changedSlots` and the cursor it predicts. If
  the prediction matches, the server sends nothing back, except the crafting result, which
  arrives via `set_slot`. If it does not match or the `stateId` is stale, a full `window_items`
  is sent.
- **Movement.** If the server rejects a position (speed, flying, passing through blocks), it
  sends `synchronize_player_position` and ignores movement until it receives the matching
  `teleport_confirm`. The client's `onGround` is not used for fall damage.
- **Breaking/placing.** The server answers each `block_dig`/`block_place` `sequence` (it sends
  `acknowledge_player_digging`). If it rejects the action, the client reverts its prediction when
  it receives the ack, and the hand slot is resent.
- **Death.** The client receives `update_health` with 0 and `death_combat_event`. To respawn, the
  client sends `client_command` (action 0) and receives `respawn`, new chunks and its position.

## If you use `minecraft-protocol` (PrismarineJS) in the client

You can hand it the WebSocket as a stream (`ws` in Node or a Duplex adapter in the browser).
**Watch out:** in offline mode, `createClient` calls `connect()` before registering its listeners,
so the `connect` event has to be emitted on the next tick:

```js
const stream = createWebSocketStream(ws)
const client = mc.createClient({
  username, version: '1.20.4', auth: 'offline',
  connect: c => { c.setSocket(stream); setImmediate(() => c.emit('connect')) }
})
```

`tools/smoke/smoke.mjs` does exactly this with mineflayer and serves as a working reference.

## Packets the client must support for full survival

Besides the basics, the server uses:

- `open_window` with the menu types `generic_9x3` (2, chests and barrels), `crafting` (12),
  `enchantment` (13) and `furnace` (14), `craft_progress_bar` for furnace progress and
  enchantment offers, `block_action` for chest lids.
- `spawn_entity` for mobs, items and arrows, `spawn_entity_experience_orb`, `entity_teleport`,
  `entity_head_rotation`, `entity_metadata` (health, on fire, raised arms, creeper fuse, sheared
  sheep), `entity_equipment`, `damage_event`, `entity_status` (3 = death, 9 = finished eating,
  47-52 = tool/armor breaking), `entity_velocity` (knockback).
- `explosion` (with affected blocks, knockback, and inline particles and sound), `sound_effect` and
  `entity_sound_effect` with the sound **by name** (holder 0 + `minecraft:...`), `experience`,
  `collect`.
- Items with NBT: `Damage`, `Enchantments` and `StoredEnchantments` (books), as in vanilla 1.20.4.

## Packets for the newer features (potions, workstations, vehicles, mobs...)

For everything to render and work, the client also needs:

- **On join:** `declare_recipes` with the stonecutter recipes (`minecraft:stonecutting`) and the
  smithing recipes (`smithing_transform` and `smithing_trim`). The client's stonecutter lists the
  recipes for the input sorted by the result's translation key, and the pressed button
  (`container_button_click`, the `enchant_item` packet in minecraft-data) is the index in that list.
  The loom uses the same packet with the index of the pattern among those in the
  `minecraft:no_item_required` tag (or those of the inserted banner pattern); tags arrive during
  configuration.
- **Windows:** `open_window` with `anvil` (8), `brewing_stand` (11), `grindstone` (15), `loom` (18),
  `merchant` (19), `smithing` (21), `cartography_table` (23) and `stonecutter` (24). The anvil sends
  the cost in property 0 (`craft_progress_bar`) and expects `name_item`. The brewing stand uses
  properties 0 (brew time) and 1 (fuel). The villager sends `trade_list` after `open_window` and
  expects `select_trade`; when an offer is selected the server moves the payment from the
  inventory like vanilla.
- **Results the client does not predict** (crafting, workstations, shift-click): these arrive as
  `set_slot` even if the prediction for the rest of the click was correct.
- **Signs:** placing one triggers `open_sign_entity` (`isFrontText`); the client replies with
  `update_sign` (plain text, 4 lines). The text reaches everyone via `block_entity_data` (type 7 or
  8) and in the chunk's block entity NBT: `front_text`/`back_text` with `messages` (JSON text),
  `color` and `has_glowing_text`, plus `is_waxed`. Banners carry `Patterns`.
- **Books:** the book and quill is opened by the client itself; saving and signing use `edit_book`
  (hotbar slot 0-8 or 40). Using a written book triggers `open_book`.
- **Effects:** `entity_effect` (id from the `mob_effect` registry, starting at 0),
  `remove_entity_effect` and `update_attributes` for `generic.movement_speed` (Speed/Slowness).
  Player metadata: index 0 with 0x01 (on fire) and 0x20 (invisible), 10 particle color,
  15 absorption.
- **Maps:** `map` with id, scale, locked, the player marker and the redrawn area
  (columns/rows/x/z and colors). It is sent in full the first time the player holds that map.
- **Vehicles:** `spawn_entity` of `boat`/`chest_boat`/`minecart`, `set_passengers` when mounting
  and dismounting (empty list on dismount). In a boat the client sends `vehicle_move` (the server
  only corrects it if it jumps more than 10 blocks), `steer_boat` and `steer_vehicle`; flag 0x02 of
  `steer_vehicle` dismounts. Minecarts are moved by the server with `entity_teleport`.
- **Dragon:** `spawn_entity` of `ender_dragon`; the client numbers its 8 parts with the following
  ids, and hitting a part (`use_entity` with that id) damages the dragon. Phase in metadata
  16 (9 = dying, with health at 0). `boss_bar` (add, health, remove) while in the End.
- **Projectiles:** `spawn_entity` of `small_fireball`, `fireball`, `dragon_fireball`, `potion`,
  `ender_pearl` and `experience_bottle` (with the item in metadata 8); `set_cooldown` after
  throwing a pearl; `world_event` 2002/2007 when a potion breaks.

## Ranged weapons, shield and fishing

- **Use and release:** bow, crossbow, trident and shield start with `use_item` and end with
  `block_dig` status 5 (release). Bow power comes from the ticks between the two; the crossbow
  stays loaded (`Charged` and `ChargedProjectiles` in its NBT) and fires on the next `use_item`.
  The client must send its look direction before `use_item` (as vanilla does, in the same tick).
- **Active hand:** player metadata 8 (0x01 active, 0x02 off hand), so others can see the drawn
  bow, the raised shield or someone eating.
- **Arrows and tridents:** `spawn_entity` of `arrow`/`spectral_arrow`/`trident` with the owner in
  the data field; metadata 8 (0x01 critical), 9 piercing, 10 tipped-arrow color or trident
  loyalty, and 11 enchantment glint. Picking them up sends `collect`.
- **Shield:** `entity_status` 29 when blocking a hit and 30 when disabled by an axe, with a
  shield `set_cooldown` (100 ticks). Trident Riptide arrives as an `entity_velocity` for the
  player themselves.
- **Fishing:** `spawn_entity` of `fishing_bobber` with the owner's id in the data field (without
  it the client does not draw the line); metadata 8 (hooked entity + 1) and 9 (biting). On a bite,
  6 `bubble` and 6 `fishing` particles appear above the bobber along with the splash sound; the
  client reels in with another `use_item`.

## Limits

- `max-packets-per-second` (500 by default): a client that exceeds it is kicked with
  «Demasiados paquetes» ("Too many packets"). A normal client sends about 20-60 per second.
- Connections per IP: 5 concurrent and 15 new ones every 10 s (localhost is exempt).
- Entities are only sent when they are nearby. After a long-distance `/tp`, the surrounding ones
  appear in under 0.1 s, and whatever is now out of range arrives as `entity_destroy`.

## Dimensions

- A player may join directly into the Nether or the End if they disconnected there: `login`
  carries `worldType`/`worldName` = `minecraft:the_nether` or `minecraft:the_end`.
- When changing dimension (portal, `/dimension`, death outside the Overworld) the client receives
  `respawn` with the new dimension and `dataKept = 0x03`. Then come abilities, health, inventory,
  time, `game_event 13`, the chunks of the new dimension and `synchronize_player_position`.
  The client must discard all chunks and entities from the previous dimension.
- Chunk height: Overworld 24 sections (-64..320); Nether and End 16 sections (0..256).
  The Nether and the End have no skylight: the skylight masks arrive empty.
- Biomes are in each section's palette (4x4x4 cells, ids from `registry_data`).
- After a `respawn`, no more chunks from the previous dimension are sent: if the server was
  preparing chunks for the old view, it discards them.
